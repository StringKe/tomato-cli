//! 番茄网页端接口。经实测（2026-09-16），原生客户端可用的路径：
//! - 书架列表 `/reading/bookapi/bookshelf/info/v:version/`，只有 book_id 和分组；
//! - 书架详情 `POST /api/bookshelf/multidetail`，给书名、封面、章数、最近阅读章；
//! - 目录 `/api/reader/directory/detail`、用户 `/api/user/info/v2`、进度 `/api/reader/book/progress`；
//! - 书籍页 `/page/{book_id}` 和阅读页 `/reader/{item_id}` 的 SSR 数据，给作者、简介、最新章和正文。
//!
//! 不需要登录也不需要签名的接口：
//! - 搜索 `GET https://novel.snssdk.com/api/novel/channel/homepage/search/search/v1/?aid=1967&q={q}&offset={n}`，
//!   `data.ret_data[]` 给 book_id、title、author、abstract、category、creation_status、score、thumb_url，全字段明文；`data.has_more` 和 `data.offset` 翻页。备用域名 `https://api-lf.fanqiesdk.com` 同路径。
//! - 榜单 `GET https://fanqienovel.com/api/rank/category/list?app_id=2503&rank_list_type=3&offset={n}&limit={m}&category_id={id}&rank_version=&gender={g}&rankMold=2`，
//!   `data.book_list[]` 给 bookId、bookName、author、abstract、creationStatus、firstChapterItemId、lastChapterItemId、lastChapterTitle、read_count、wordNumber、thumbUri，`data.total_num` 为 100。
//!   category_id 必填（-1 / 0 返回空），分类列表来自 `/rank` 页面 SSR 的 `rank.rankCategoryTypeList.{male,female}[].{id,name}`；male 分组的分类只在 gender=1 下有数据，female 分组只在 gender=0 下有数据。
//!   bookName、author、abstract、lastChapterTitle 都混有阅读页字体的 PUA，走 decode_pua。
//! - 分类库 `/api/library`、分类列表 `/api/category_list`、书籍简况 `/api/simple/info`：状态 UNKNOWN，本轮未接入。
//!
//! 需要 a_bogus 签名的接口：网页搜索 `/api/author/search/search_book/v1` 和章节 `/api/reader/full`，都走 `Client::signed_get`。
//! 服务端接受抖音 web 通用的 bdms 长格式签名；签名绑定 a_bogus 之前的整段 query 串、User-Agent 和 browser_info 常量，不绑定时间戳、cookie、msToken、Referer/Origin，可重放。
//! 签名不被接受时服务端返回 HTTP 200 空 body。
//! UA 策略独立于签名：mac / Windows 的 Chrome / Safari / Firefox 通过，Linux Chrome 和 curl 被拒，所以 UA 常量固定写死，不按平台生成。
//!
//! 登录墙：`/api/reader/directory/detail` 的章节在前 10 章之外几乎全部 `isChapterLock=true`，`needPay` 全 0。
//! 阅读页 SSR 对锁定章节未登录只给约 600 到 700 字节的截断预览且 `chapterData.isChapterLock=true`，登录后给全文且该字段为 false。
//!
//! 书籍页 `/page/{book_id}` 对出版付费书返回 HTTP 404（bookName 为空），与「不存在」是两种情况。
//!
//! 已失效：novel.snssdk.com 上的 `/api/novel/book/directory/list/v1/`、`/api/novel/book/reader/full/v1/`、`/api/novel/book/detail/v1/` 路径均不再返回数据。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use reqwest::header::{HeaderMap, SET_COOKIE};
use serde_json::Value;

use crate::model::{Book, Chapter, ChapterBody, Progress, ShelfItem, User};
use crate::store::{self, cookie_header, merge_set_cookie};

mod abogus;
mod parse;
mod ssr;
use abogus::a_bogus;
use parse::{app_query, check_biz, json_i64, json_str, json_u64, parse_chapter, parse_directory, parse_page_book, parse_search_book, parse_snssdk_book};
pub use parse::passport_query;

pub const HOST: &str = "https://fanqienovel.com";
/// 无签名搜索接口所在域名，以及主域名失败时再试一次的备用域名。
const SEARCH_HOST: &str = "https://novel.snssdk.com";
const SEARCH_HOST_BACKUP: &str = "https://api-lf.fanqiesdk.com";
const SEARCH_PATH: &str = "/api/novel/channel/homepage/search/search/v1/";
/// 无签名搜索接口每页条数，签名接口兜底时按它把 offset 换算成页码。
const SEARCH_PAGE: u32 = 10;
/// 网页端签名版搜索接口，需要 a_bogus，只在无签名接口两个域名都失败时兜底。
const SEARCH_BOOK: &str = "/api/author/search/search_book/v1";
const DIRECTORY: &str = "/api/reader/directory/detail";
const CHAPTER: &str = "/api/reader/full";
const USER_INFO: &str = "/api/user/info/v2";
const READ_PROGRESS: &str = "/api/reader/book/progress";
const UPDATE_PROGRESS: &str = "/api/reader/book/update_progress";
const BOOKSHELF_INFO: &str = "/reading/bookapi/bookshelf/info/v:version/";
const BOOKSHELF_DETAIL: &str = "/api/bookshelf/multidetail";
const BOOKSHELF_ADD: &str = "/reading/bookapi/bookshelf/add/v";
const BOOKSHELF_DELETE: &str = "/reading/bookapi/bookshelf/delete/v";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36";
/// 任意两次请求之间的最小间隔。预读和书架补全各自还有更长的间隔，这里是所有请求共用的兜底。
const REQUEST_GAP: Duration = Duration::from_millis(300);

/// 未登录时撞到锁定章节。`chapter` 靠类型识别它并跳过接口兜底，不比较文案。
#[derive(Debug)]
struct LoginWall;

impl std::fmt::Display for LoginWall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("该章节需要登录后阅读")
    }
}

impl std::error::Error for LoginWall {}

/// `request` 遇到 HTTP 200 空 body。签名接口靠类型判断签名被拒。
#[derive(Debug)]
struct EmptyBody;

impl std::fmt::Display for EmptyBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("空响应，接口可能要求浏览器签名")
    }
}

impl std::error::Error for EmptyBody {}

/// 阅读页 SSR 里能拿到、而书架详情接口缺失的字段。
pub struct ReaderMeta {
    pub author: String,
    pub chapter_title: String,
    pub creation_status: Option<i64>,
}

/// 非 2xx 响应。保留状态码，调用方据此区分 404 等需要单独文案的情况。
#[derive(Debug)]
struct HttpError {
    status: reqwest::StatusCode,
    body: String,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}: {}", self.status, self.body)
    }
}

impl std::error::Error for HttpError {}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    cookies: Arc<Mutex<BTreeMap<String, String>>>,
    /// 上一次请求发出的时间，所有克隆共享，用来限制请求频率。
    last_request: Arc<tokio::sync::Mutex<Instant>>,
    /// 正在等待放行的前台请求数，后台请求看到它非零就让路。
    fg_waiting: Arc<AtomicUsize>,
    /// 后台任务（预读、书架补全）用的克隆置真：排队时让给前台，用户等着看的请求不排在它后面。
    background: bool,
}

impl Client {
    pub fn new(cookies: BTreeMap<String, String>) -> Result<Self> {
        // reqwest 开的是 rustls-no-provider，进程里必须先装好 ring；重复安装只会返回 Err，忽略即可。
        let _ = rustls::crypto::ring::default_provider().install_default();
        // 建连单独限时：域名被拦或网络半通时 SYN 无回应，不然每个候选主机都要等满 20 秒总超时。
        let http = reqwest::Client::builder().user_agent(UA).connect_timeout(Duration::from_secs(8)).timeout(Duration::from_secs(20)).redirect(reqwest::redirect::Policy::limited(8)).build().context("创建 HTTP 客户端")?;
        // 起点往前推一个间隔，第一次请求不用等。
        let last = Instant::now().checked_sub(REQUEST_GAP).unwrap_or_else(Instant::now);
        Ok(Self { http, cookies: Arc::new(Mutex::new(cookies)), last_request: Arc::new(tokio::sync::Mutex::new(last)), fg_waiting: Arc::new(AtomicUsize::new(0)), background: false })
    }

    /// 给后台任务用的克隆，共享 cookie 和节流状态，只是排队时让前台先走。
    pub fn background(&self) -> Self {
        Self { background: true, ..self.clone() }
    }

    pub fn cookies(&self) -> BTreeMap<String, String> {
        self.cookies.lock().expect("cookies lock").clone()
    }

    /// 是否带着会话 cookie。登录墙判断只看这一点，不额外发请求。
    fn has_session(&self) -> bool {
        let cookies = self.cookies.lock().expect("cookies lock");
        ["sessionid", "sessionid_ss", "sid_tt"].iter().any(|k| cookies.get(*k).is_some_and(|v| !v.is_empty()))
    }

    pub fn logout(&self) {
        self.cookies.lock().expect("cookies lock").clear();
    }

    pub fn apply_cookie_header(&self, raw: &str) {
        let mut cookies = self.cookies.lock().expect("cookies lock");
        for part in raw.split([';', '\n', '\r', '\t']) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            merge_set_cookie(&mut cookies, part);
        }
    }

    pub async fn login_with_cookie(&self, raw: &str) -> Result<User> {
        self.apply_cookie_header(raw);
        self.user_info().await?.ok_or_else(|| anyhow!("Cookie 无效或已过期"))
    }

    /// 无签名搜索接口，按 offset 翻页。返回 (书目, 是否还有下一页, 下一页 offset)。
    /// 主域名失败（含 HTTP 成功但业务 code 非 0）时换备用域名再试一次，两个域名都失败再用签名接口兜底；三条路径都失败时把三段错误合并返回。
    pub async fn search(&self, query: &str, offset: u32) -> Result<(Vec<Book>, bool, u32)> {
        let q = urlencoding::encode(query);
        let first = match self.search_at(SEARCH_HOST, &q, offset).await {
            Ok(page) => return Ok(page),
            Err(e) => e,
        };
        let second = match self.search_at(SEARCH_HOST_BACKUP, &q, offset).await {
            Ok(page) => return Ok(page),
            Err(e) => e,
        };
        match self.search_book(query, offset / SEARCH_PAGE).await {
            Ok((books, has_more)) => {
                let next = offset + books.len() as u32;
                Ok((books, has_more, next))
            }
            Err(third) => Err(anyhow!("搜索失败：{first}；备用域名：{second}；签名接口：{third}")),
        }
    }

    /// 某个域名上的一页无签名搜索结果。业务 code 非 0 也算失败，交给调用方换路径。
    async fn search_at(&self, host: &str, encoded_query: &str, offset: u32) -> Result<(Vec<Book>, bool, u32)> {
        let url = format!("{host}{SEARCH_PATH}?aid=1967&q={encoded_query}&offset={offset}");
        let json = self.get_json(&url, &[]).await?;
        check_biz(&json, "搜索")?;
        let data = &json["data"];
        let list = data.get("ret_data").and_then(Value::as_array);
        let books: Vec<Book> = list.into_iter().flatten().filter_map(parse_snssdk_book).collect();
        let has_more = data.get("has_more").and_then(Value::as_bool).unwrap_or(false) && !books.is_empty();
        let next = json_u64(data, &["offset"]) as u32;
        // 接口偶尔不回 offset，按本页条数自己推进，避免翻页停在同一页。
        let next = if next > offset { next } else { offset + books.len() as u32 };
        Ok((books, has_more, next))
    }

    /// 网页端签名搜索，page 从 0 起，每页 SEARCH_PAGE 条。返回 (书目, 是否还有下一页)。字段带 PUA，由 parse_search_book 还原。
    pub async fn search_book(&self, query: &str, page: u32) -> Result<(Vec<Book>, bool)> {
        let q = format!("filter=127%2C127%2C127%2C127&page_count={SEARCH_PAGE}&page_index={page}&query_type=0&query_word={}", urlencoding::encode(query));
        let json = self.signed_get(SEARCH_BOOK, &q).await?;
        check_biz(&json, "搜索")?;
        let data = &json["data"];
        let list = data.get("search_book_data_list").and_then(Value::as_array);
        let books: Vec<Book> = list.into_iter().flatten().filter_map(parse_search_book).collect();
        // 这个接口的翻页字段没有实测样本，parse 里也没有对应 key；有 has_more / hasMore 就用，缺失时按本页满 SEARCH_PAGE 条推断。
        let has_more = data.get("has_more").or_else(|| data.get("hasMore")).and_then(Value::as_bool).unwrap_or(books.len() as u32 >= SEARCH_PAGE);
        Ok((books, has_more))
    }

    /// 书籍页 SSR：一次拿到元数据和完整目录。
    pub async fn book_page(&self, book_id: &str) -> Result<(Book, Vec<Chapter>)> {
        let url = format!("{HOST}/page/{}", urlencoding::encode(book_id));
        let html = self.request("GET", &url, &[("Accept", "text/html")], None).await.map_err(|e| match e.downcast_ref::<HttpError>() {
            Some(h) if h.status == reqwest::StatusCode::NOT_FOUND => anyhow!("网页端不提供这本书（付费出版物或已下架）"),
            _ => e,
        })?;
        let state = ssr::initial_state(&html)?;
        let page = state.get("page").ok_or_else(|| anyhow!("书籍页缺少 page 数据"))?;
        let book = parse_page_book(page, book_id).ok_or_else(|| anyhow!("书籍不存在或已下架"))?;
        let chapters = parse_directory(page);
        Ok((book, chapters))
    }

    pub async fn directory(&self, book_id: &str) -> Result<Vec<Chapter>> {
        let url = format!("{HOST}{DIRECTORY}?bookId={}&enter_from=0", urlencoding::encode(book_id));
        let json = self.get_json(&url, &[]).await?;
        check_biz(&json, "目录")?;
        Ok(parse_directory(&json["data"]))
    }

    /// 正文优先走阅读页 SSR，章节接口只在页面解析失败时兜底。撞到登录墙时直接返回，章节接口同样不会给全文。
    pub async fn chapter(&self, item_id: &str) -> Result<ChapterBody> {
        match self.chapter_page(item_id).await {
            Ok(body) => Ok(body),
            Err(page_err) if page_err.downcast_ref::<LoginWall>().is_some() => Err(page_err),
            Err(page_err) => self.chapter_api(item_id).await.map_err(|api_err| anyhow!("{page_err}；接口兜底：{api_err}")),
        }
    }

    /// 锁定章节在未登录时只有截断预览，不能当正文用。登录状态下服务端会把 isChapterLock 置 false；即使仍为 true 也照样返回正文。
    async fn chapter_page(&self, item_id: &str) -> Result<ChapterBody> {
        let url = format!("{HOST}/reader/{}", urlencoding::encode(item_id));
        let html = self.request("GET", &url, &[("Accept", "text/html")], None).await?;
        let state = ssr::initial_state(&html)?;
        let d = state.pointer("/reader/chapterData").ok_or_else(|| anyhow!("阅读页缺少 chapterData"))?;
        let body = parse_chapter(d, item_id)?;
        if body.locked && !self.has_session() {
            return Err(LoginWall.into());
        }
        Ok(body)
    }

    async fn chapter_api(&self, item_id: &str) -> Result<ChapterBody> {
        let json = self.signed_get(CHAPTER, &format!("itemId={}", urlencoding::encode(item_id))).await?;
        check_biz(&json, "章节")?;
        let d = json.pointer("/data/chapterData").or_else(|| json.get("data")).cloned().ok_or_else(|| anyhow!("章节数据为空"))?;
        parse_chapter(&d, item_id)
    }

    /// 阅读页 SSR 的作者和章节标题，用于补全书架条目。
    pub async fn reader_meta(&self, item_id: &str) -> Result<ReaderMeta> {
        let url = format!("{HOST}/reader/{}", urlencoding::encode(item_id));
        let html = self.request("GET", &url, &[("Accept", "text/html")], None).await?;
        let state = ssr::initial_state(&html)?;
        let d = state.pointer("/reader/chapterData").ok_or_else(|| anyhow!("阅读页缺少 chapterData"))?;
        Ok(ReaderMeta { author: json_str(d, &["author"], ""), chapter_title: json_str(d, &["title"], ""), creation_status: json_i64(d, &["creationStatus"]) })
    }

    pub async fn user_info(&self) -> Result<Option<User>> {
        let url = format!("{HOST}{USER_INFO}");
        let json = match self.get_json(&url, &[]).await {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        let d = &json["data"];
        let id = json_str(d, &["id"], "");
        if id.is_empty() || id == "0" || id == "1" {
            return Ok(None);
        }
        Ok(Some(User {
            id,
            name: json_str(d, &["name"], ""),
            avatar: json_str(d, &["avatar"], ""),
            desc: json_str(d, &["desc"], ""),
            is_vip: d.get("isVip").and_then(Value::as_bool).unwrap_or(false),
        }))
    }

    /// 书架 = 列表接口（id 和分组）+ 详情接口（书名、封面、章数、最近阅读章）。作者和最新章标题由 reader_meta 另行补全。
    pub async fn bookshelf(&self) -> Result<Vec<ShelfItem>> {
        let url = format!("{HOST}{BOOKSHELF_INFO}?{}", app_query());
        let json = self.get_json(&url, &[]).await?;
        if json.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
            return Err(anyhow!("{}", json.get("message").and_then(Value::as_str).unwrap_or("获取书架失败")));
        }
        let list = json.pointer("/data/book_shelf_info").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut items: Vec<ShelfItem> = list
            .into_iter()
            .map(|b| ShelfItem { book_id: json_str(&b, &["book_id"], ""), group_name: json_str(&b, &["group_name"], ""), ..ShelfItem::default() })
            .filter(|b| !b.book_id.is_empty())
            .collect();
        if items.is_empty() {
            return Ok(items);
        }
        let progress: BTreeMap<String, String> = self.pull_progress().await.unwrap_or_default().into_iter().map(|p| (p.book_id, p.item_id)).collect();
        let books: Vec<Value> = items.iter().map(|b| serde_json::json!({"book_id": b.book_id, "item_id": progress.get(&b.book_id).cloned().unwrap_or_else(|| "0".into())})).collect();
        let detail = self.post_json(&format!("{HOST}{BOOKSHELF_DETAIL}"), &serde_json::json!({"books": books})).await?;
        check_biz(&detail, "书架详情")?;
        let details: BTreeMap<String, Value> = detail.pointer("/data/detail_list").and_then(Value::as_array).cloned().unwrap_or_default().into_iter().map(|d| (json_str(&d, &["book_id"], ""), d)).collect();
        for item in &mut items {
            let Some(d) = details.get(&item.book_id) else { continue };
            item.title = json_str(d, &["book_name", "original_book_name"], "");
            item.thumb_url = json_str(d, &["thumb_url"], "");
            item.chapter_count = json_u64(d, &["serial_count"]) as u32;
            item.creation_status = json_i64(d, &["creation_status"]);
            item.last_chapter_item_id = json_str(d, &["last_chapter_item_id"], "");
            item.last_read_item_id = progress.get(&item.book_id).cloned().unwrap_or_default();
            if !item.last_read_item_id.is_empty() {
                item.last_read_title = json_str(d, &["item_show_title", "title"], "");
            }
        }
        Ok(items)
    }

    pub async fn add_bookshelf(&self, book_id: &str) -> Result<()> {
        let url = format!("{HOST}{BOOKSHELF_ADD}?{}", app_query());
        let body = serde_json::json!({
            "add_book_source": 0,
            "identify_data": [{"asterisked": false, "book_id": book_id, "book_type": 0, "modify_time": store::now_ms()}],
        });
        let json = self.post_json(&url, &body).await?;
        let code = json.get("code").and_then(Value::as_i64).unwrap_or(0);
        if code != 0 {
            return Err(anyhow!("{}", json.get("message").and_then(Value::as_str).unwrap_or("加入书架失败")));
        }
        Ok(())
    }

    pub async fn remove_bookshelf(&self, book_id: &str) -> Result<()> {
        let url = format!("{HOST}{BOOKSHELF_DELETE}?{}", app_query());
        let body = serde_json::json!({
            "identify_data": [{"asterisked": false, "book_id": book_id, "book_type": 0, "modify_time": store::now_ms()}],
        });
        let json = self.post_json(&url, &body).await?;
        let code = json.get("code").and_then(Value::as_i64).unwrap_or(0);
        if code != 0 {
            return Err(anyhow!("{}", json.get("message").and_then(Value::as_str).unwrap_or("移出书架失败")));
        }
        Ok(())
    }

    pub async fn pull_progress(&self) -> Result<Vec<Progress>> {
        let url = format!("{HOST}{READ_PROGRESS}");
        let json = self.get_json(&url, &[]).await?;
        if json.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
            return Ok(Vec::new());
        }
        let list = json.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(list
            .into_iter()
            .map(|p| Progress {
                book_id: json_str(&p, &["book_id"], ""),
                item_id: json_str(&p, &["item_id"], ""),
                title: String::new(),
                line: 0,
                anchor: None,
                updated_ms: json_u64(&p, &["read_timestamp"]).saturating_mul(1000),
            })
            .filter(|p| !p.book_id.is_empty())
            .collect())
    }

    pub async fn push_progress(&self, progress: &Progress, order: u32) -> Result<()> {
        let url = format!("{HOST}{UPDATE_PROGRESS}");
        let body = serde_json::json!({
            "book_id": progress.book_id,
            "item_id": progress.item_id,
            "read_progress": order,
            "index": order,
            "read_timestamp": progress.updated_ms / 1000,
            "genre_type": 0,
        });
        let _ = self.post_json(&url, &body).await;
        Ok(())
    }

    /// 下载封面等二进制资源。
    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.http.get(url).header("Referer", format!("{HOST}/")).send().await.with_context(|| format!("GET {url}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(anyhow!("HTTP {status}"));
        }
        Ok(resp.bytes().await.context("读取响应")?.to_vec())
    }

    /// 带 a_bogus 的 GET。`query` 是已序列化、不含 a_bogus 的原始串，签名逐字节绑定它，拼接后不能再改顺序或编码。
    async fn signed_get(&self, path: &str, query: &str) -> Result<Value> {
        let url = format!("{HOST}{path}?{}", append_signature(query, &a_bogus(query, UA)));
        self.get_json(&url, &[]).await.map_err(|e| if e.downcast_ref::<EmptyBody>().is_some() { anyhow!("签名未被接受") } else { e })
    }

    pub async fn get_json(&self, url: &str, extra_headers: &[(&str, &str)]) -> Result<Value> {
        let text = self.request("GET", url, extra_headers, None).await?;
        serde_json::from_str(&text).with_context(|| format!("JSON: {}", text.chars().take(200).collect::<String>()))
    }

    pub async fn post_json(&self, url: &str, body: &Value) -> Result<Value> {
        let text = self.request("POST", url, &[("Content-Type", "application/json")], Some(body.to_string())).await?;
        serde_json::from_str(&text).with_context(|| format!("JSON: {}", text.chars().take(200).collect::<String>()))
    }

    pub async fn request(&self, method: &str, url: &str, extra_headers: &[(&str, &str)], body: Option<String>) -> Result<String> {
        let mut req = match method {
            "POST" => self.http.post(url),
            _ => self.http.get(url),
        };
        req = req.header("Accept", "application/json, text/plain, */*");
        // novel.snssdk.com 等其他域名收到 Origin: https://fanqienovel.com 会回 403，站外接口不带来源头。
        if url.starts_with(HOST) {
            req = req.header("Referer", format!("{HOST}/")).header("Origin", HOST);
        }
        if let Some(cookie) = cookie_header(&self.cookies.lock().expect("cookies lock")) {
            req = req.header("Cookie", cookie);
        }
        for (k, v) in extra_headers {
            req = req.header(*k, *v);
        }
        if let Some(body) = body {
            req = req.body(body);
        }
        self.throttle().await;
        let resp = req.send().await.with_context(|| format!("{method} {url}"))?;
        self.harvest_cookies(resp.headers());
        let status = resp.status();
        let text = resp.text().await.context("读取响应")?;
        if !status.is_success() {
            return Err(HttpError { status, body: text.chars().take(200).collect() }.into());
        }
        if text.is_empty() {
            return Err(EmptyBody.into());
        }
        Ok(text)
    }

    /// 所有请求共用最小间隔，前台请求优先于后台任务。锁只在算等待时间时持有，不在睡眠时持有：
    /// 后台任务睡着时前台可以插到它前面，而服务端看到的任意两次请求间隔仍不小于 REQUEST_GAP。
    async fn throttle(&self) {
        if !self.background {
            self.fg_waiting.fetch_add(1, Ordering::SeqCst);
        }
        loop {
            let wait = {
                let mut last = self.last_request.lock().await;
                if self.background && self.fg_waiting.load(Ordering::SeqCst) > 0 {
                    REQUEST_GAP
                } else {
                    let elapsed = last.elapsed();
                    if elapsed >= REQUEST_GAP {
                        *last = Instant::now();
                        break;
                    }
                    REQUEST_GAP - elapsed
                }
            };
            tokio::time::sleep(wait).await;
        }
        if !self.background {
            self.fg_waiting.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn harvest_cookies(&self, headers: &HeaderMap) {
        let mut cookies = self.cookies.lock().expect("cookies lock");
        for value in headers.get_all(SET_COOKIE) {
            if let Ok(s) = value.to_str() {
                merge_set_cookie(&mut cookies, s);
            }
        }
    }
}

/// 把签名追加为最后一个参数。签名是自定义 base64，含 `/`、`+`、`=`，必须整体 percent-encoding，否则服务端解出的签名和计算时不一致。
fn append_signature(query: &str, sign: &str) -> String {
    format!("{query}&a_bogus={}", urlencoding::encode(sign))
}

/// 榜单接口，不需要登录和签名。分类 id 只能从 /rank 页面 SSR 拿。
const RANK_LIST: &str = "/api/rank/category/list";

impl Client {
    /// /rank 页面 SSR 里的分类列表，男频女频各一组。
    pub async fn rank_categories(&self) -> Result<Vec<crate::model::RankCategory>> {
        let url = format!("{HOST}/rank");
        let html = self.request("GET", &url, &[("Accept", "text/html")], None).await?;
        let state = ssr::initial_state(&html)?;
        let list = state.pointer("/rank/rankCategoryTypeList").ok_or_else(|| anyhow!("榜单页缺少分类列表"))?;
        let cats = parse::parse_rank_categories(list);
        if cats.is_empty() {
            return Err(anyhow!("榜单分类为空"));
        }
        Ok(cats)
    }

    /// 某分类榜单的一页，返回 (书目, 榜单总数)。gender 见 RankCategory::gender。
    pub async fn rank_list(&self, category_id: &str, gender: u8, offset: u32, limit: u32) -> Result<(Vec<Book>, u32)> {
        let url = format!("{HOST}{RANK_LIST}?app_id=2503&rank_list_type=3&offset={offset}&limit={limit}&category_id={}&rank_version=&gender={gender}&rankMold=2", urlencoding::encode(category_id));
        let json = self.get_json(&url, &[]).await?;
        check_biz(&json, "榜单")?;
        let data = &json["data"];
        let books: Vec<Book> = data.get("book_list").and_then(Value::as_array).into_iter().flatten().filter_map(parse::parse_rank_book).collect();
        let total = json_u64(data, &["total_num"]) as u32;
        Ok((books, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_appended_last_and_percent_encoded() {
        assert_eq!(append_signature("itemId=1&x=y", "ab/c+d=="), "itemId=1&x=y&a_bogus=ab%2Fc%2Bd%3D%3D");
        let query = "filter=127%2C127%2C127%2C127&page_index=0&query_word=%E5%8D%81";
        let signed = append_signature(query, &a_bogus(query, UA));
        let (head, tail) = signed.split_once("&a_bogus=").unwrap();
        assert_eq!(head, query);
        assert!(!tail.contains(['/', '+', '=']));
        assert!((188..=192).contains(&urlencoding::decode(tail).unwrap().len()));
    }

    /// 后台请求先到队列却在睡，前台请求后到也要先放行，且两次放行之间仍隔满 REQUEST_GAP。
    #[test]
    fn foreground_request_goes_before_waiting_background() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let client = Client::new(BTreeMap::new()).unwrap();
            client.throttle().await;
            let bg = client.background();
            let fg = client.clone();
            let bg_task = tokio::spawn(async move {
                bg.throttle().await;
                Instant::now()
            });
            tokio::time::sleep(Duration::from_millis(20)).await;
            fg.throttle().await;
            let fg_done = Instant::now();
            let bg_done = bg_task.await.unwrap();
            assert!(fg_done < bg_done);
            assert!(bg_done.duration_since(fg_done) >= REQUEST_GAP - Duration::from_millis(5));
        });
    }
}
