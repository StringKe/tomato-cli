use std::time::Duration;

use image::DynamicImage;

use crate::auth::{self, QrTicket};
use crate::model::{Book, Chapter, ChapterBody, Progress, RankCategory, ShelfItem, User};

use super::{App, OpenBook, Overlay, Screen};

/// 补全书架条目时相邻两次阅读页请求的间隔，避免对同一站点连发几十个请求。
const HYDRATE_GAP: Duration = Duration::from_millis(200);
/// 榜单每页条数。
const RANK_PAGE: u32 = 20;

pub(super) enum WorkerMsg {
    /// 一页搜索结果：(书目, 是否还有下一页, 下一页 offset)。append 为真表示追加到现有列表末尾。
    SearchDone { result: Result<(Vec<Book>, bool, u32), String>, append: bool },
    OpenDone { book: Book, chapters: Result<Vec<Chapter>, String> },
    RankCats(Result<Vec<RankCategory>, String>),
    /// 一页榜单。带上请求时的分类和 offset，到达时若用户已切换分类或刷新则丢弃，避免串页。
    RankPage { category_id: String, gender: u8, offset: u32, result: Result<(Vec<Book>, u32), String> },
    ChapterDone(Result<ChapterBody, String>),
    ShelfDone(Result<Vec<ShelfItem>, String>),
    /// 阅读页补全的作者和最新章标题，逐本到达。
    ShelfMeta { book_id: String, author: String, last_chapter_title: String, creation_status: Option<i64> },
    ShelfMetaDone,
    CoverDone { book_id: String, image: Result<DynamicImage, String> },
    /// 一章预读结束（成功与否都发），用于清掉 prefetching 标记。
    Prefetched(String),
    QrReady(Result<QrTicket, String>),
    LoginDone(Result<User, String>),
    ProgressDone(Result<Vec<Progress>, String>),
    UpdateHint(Option<String>),
    FontmapDone(Result<usize, String>),
}

impl WorkerMsg {
    /// 后台静默任务的消息，不影响页脚的忙碌标记。
    fn is_quiet(&self) -> bool {
        matches!(self, WorkerMsg::ShelfMeta { .. } | WorkerMsg::ShelfMetaDone | WorkerMsg::CoverDone { .. } | WorkerMsg::Prefetched(_))
    }
}

/// 搜索框里直接输入的 book_id 或书籍页链接（`https://fanqienovel.com/page/123?x=y`）。
pub(super) fn book_id_from_input(input: &str) -> Option<String> {
    let q = input.trim();
    let digits = if let Some(rest) = q.split("/page/").nth(1) { rest.chars().take_while(char::is_ascii_digit).collect::<String>() } else { q.to_string() };
    if digits.len() >= 8 && digits.chars().all(|c| c.is_ascii_digit()) { Some(digits) } else { None }
}

impl App {
    pub(super) fn drain_worker(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            if !msg.is_quiet() {
                self.busy = false;
            }
            match msg {
                WorkerMsg::SearchDone { result, append } => match result {
                    Ok((books, has_more, next_offset)) => {
                        let added = books.len();
                        if append {
                            self.search_results.extend(books);
                        } else {
                            self.search_results = books;
                            self.search_list.select(if self.search_results.is_empty() { None } else { Some(0) });
                        }
                        self.search_has_more = has_more;
                        self.search_offset = next_offset;
                        self.status = if append { format!("再加载 {added} 本，共 {} 本", self.search_results.len()) } else { format!("找到 {} 本", self.search_results.len()) };
                    }
                    Err(e) => self.status = e,
                },
                WorkerMsg::OpenDone { book, chapters } => match chapters {
                    Ok(chs) if !chs.is_empty() => {
                        let index = self.state.progress.get(&book.book_id).and_then(|p| chs.iter().position(|c| c.item_id == p.item_id)).unwrap_or(0);
                        let mut list = ratatui::widgets::ListState::default();
                        list.select(Some(index));
                        self.spawn_cover(&book.book_id, &book.thumb_url);
                        let resume = std::mem::take(&mut self.resume_on_open) && self.state.progress.contains_key(&book.book_id);
                        self.open = Some(OpenBook { book, chapters: chs, list, filter: String::new(), demo_bodies: Vec::new() });
                        self.push(Screen::Book);
                        if resume {
                            self.jump_to_index(index);
                        }
                    }
                    Ok(_) => self.status = "目录为空".into(),
                    Err(e) => self.status = e,
                },
                WorkerMsg::RankCats(result) => match result {
                    Ok(cats) => {
                        self.rank_cats = cats;
                        self.rank_cat_idx = 0;
                        self.spawn_rank_page(0);
                    }
                    Err(e) => self.status = e,
                },
                WorkerMsg::RankPage { category_id, gender, offset, result } => self.apply_rank_page(&category_id, gender, offset, result),
                WorkerMsg::ShelfMeta { book_id, author, last_chapter_title, creation_status } => {
                    if let Some(item) = self.state.shelf.iter_mut().find(|b| b.book_id == book_id) {
                        if item.author.is_empty() {
                            item.author = author;
                        }
                        if item.last_chapter_title.is_empty() {
                            item.last_chapter_title = last_chapter_title;
                        }
                        if item.creation_status.is_none() {
                            item.creation_status = creation_status;
                        }
                    }
                }
                WorkerMsg::ShelfMetaDone => self.persist(),
                WorkerMsg::Prefetched(item_id) => {
                    self.prefetching.remove(&item_id);
                    // 预读结束不需要重绘，但这里统一 mark 代价很小；继续预读队列里剩下的。
                    self.schedule_prefetch();
                }
                WorkerMsg::CoverDone { book_id, image } => match image {
                    Ok(img) => {
                        let protocol = self.picker.new_resize_protocol(img);
                        self.covers.insert(book_id, protocol);
                    }
                    Err(_) => {
                        self.cover_requested.remove(&book_id);
                    }
                },
                WorkerMsg::ChapterDone(result) => match result {
                    Ok(body) => self.apply_chapter(body),
                    // 登录墙的错误来自 api 层。拉取失败时用户停在目录页或阅读页，这两处 l 不是登录，p 打开的资料浮层里 l 才是。
                    Err(e) if e.contains("登录") => self.status = format!("{e}，按 p 打开资料后按 l 登录"),
                    Err(e) => self.status = e,
                },
                WorkerMsg::ShelfDone(result) => match result {
                    Ok(items) => {
                        self.merge_remote_shelf(items);
                        self.sync_shelf_select();
                        self.persist();
                        self.status = format!("书架 {} 本，{} 个文件夹", self.state.shelf.len(), crate::store::all_folders(&self.state).len());
                        self.spawn_hydrate_shelf();
                    }
                    Err(e) => self.status = e,
                },
                WorkerMsg::QrReady(result) => match result {
                    Ok(ticket) => {
                        self.login_qr = ticket.payload.clone();
                        self.login_msg = "用抖音或番茄小说 App 扫码".into();
                        if self.screen() != Screen::Login {
                            self.push(Screen::Login);
                        }
                        self.spawn_login_poll(ticket);
                    }
                    Err(e) => {
                        self.login_msg = e;
                        if self.screen() != Screen::Login {
                            self.push(Screen::Login);
                        }
                    }
                },
                WorkerMsg::LoginDone(result) => match result {
                    Ok(user) => {
                        self.status = format!("已登录 {}", user.name);
                        self.state.user = Some(user);
                        self.state.cookies = self.client.cookies();
                        let _ = crate::store::save(&self.state);
                        self.overlay = Overlay::None;
                        self.screens = vec![Screen::Shelf];
                        self.spawn_shelf_refresh();
                        self.spawn_progress_pull();
                    }
                    Err(e) => {
                        self.login_msg = e.clone();
                        self.status = e;
                    }
                },
                WorkerMsg::ProgressDone(Ok(list)) => {
                    for p in list {
                        if p.book_id.is_empty() {
                            continue;
                        }
                        let newer = self.state.progress.get(&p.book_id).map(|old| p.updated_ms > old.updated_ms).unwrap_or(true);
                        if newer {
                            crate::store::upsert_progress(&mut self.state, p);
                        }
                    }
                    let _ = crate::store::save(&self.state);
                }
                WorkerMsg::ProgressDone(Err(e)) => self.status = e,
                WorkerMsg::UpdateHint(v) => {
                    if let Some(ver) = &v {
                        self.status = format!("退出后执行 tomato update 安装 {ver}");
                    }
                    self.update_hint = v;
                }
                WorkerMsg::FontmapDone(result) => match result {
                    Ok(n) => self.status = format!("字表已生成 {n} 条"),
                    Err(e) => self.status = e,
                },
            }
            self.mark();
        }
    }

    pub(super) fn spawn_restore_session(&mut self) {
        self.busy = true;
        self.status = "正在恢复登录…".into();
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = client.user_info().await.map_err(|e| e.to_string()).and_then(|u| u.ok_or_else(|| "登录已过期".into()));
            let _ = tx.send(WorkerMsg::LoginDone(result));
        });
    }

    pub(super) fn spawn_search(&mut self) {
        let q = self.search_input.trim().to_string();
        if q.is_empty() {
            return;
        }
        if let Some(id) = book_id_from_input(&q) {
            self.spawn_open(Book::from_id(&id));
            return;
        }
        self.search_query = q.clone();
        self.search_offset = 0;
        self.search_has_more = false;
        self.status = "搜索中…".into();
        self.spawn_search_page(q, 0, false);
    }

    /// 选中项已是最后一条且还有下一页时加载下一页；否则按普通列表向下移动。
    pub(super) fn search_down(&mut self) {
        let n = self.search_results.len();
        if n > 0 && self.search_list.selected() == Some(n - 1) && self.search_has_more {
            if !self.busy {
                self.status = "加载更多…".into();
                self.spawn_search_page(self.search_query.clone(), self.search_offset, true);
            }
            return;
        }
        Self::move_list(&mut self.search_list, n, 1);
        self.mark();
    }

    fn spawn_search_page(&mut self, query: String, offset: u32, append: bool) {
        self.busy = true;
        self.mark();
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = client.search(&query, offset).await.map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::SearchDone { result, append });
        });
    }

    pub(super) fn spawn_open(&mut self, book: Book) {
        self.busy = true;
        self.status = format!("打开 {}…", book.title);
        self.mark();
        let client = self.client.clone();
        let tx = self.tx.clone();
        let id = book.book_id.clone();
        self.rt.spawn(async move {
            // 书籍页一次给出元数据和目录；页面解析失败时保留已知元数据，目录走接口兜底。
            let (book, chapters) = match client.book_page(&id).await {
                Ok((book, chapters)) if !chapters.is_empty() => (book, Ok(chapters)),
                Ok((book, _)) => (book, client.directory(&id).await.map_err(|e| e.to_string())),
                Err(page_err) => (book, client.directory(&id).await.map_err(|api_err| format!("{page_err}；目录接口：{api_err}"))),
            };
            let _ = tx.send(WorkerMsg::OpenDone { book, chapters });
        });
    }

    pub(super) fn spawn_rank_categories(&mut self) {
        self.busy = true;
        self.status = "加载榜单分类…".into();
        self.mark();
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = client.rank_categories().await.map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::RankCats(result));
        });
    }

    /// 拉当前分类从 offset 起的一页；offset 为 0 表示重新加载。
    pub(super) fn spawn_rank_page(&mut self, offset: u32) {
        let Some(cat) = self.rank_category() else {
            self.status = "当前分组没有榜单分类".into();
            self.mark();
            return;
        };
        let category_id = cat.id.clone();
        let gender = cat.gender;
        self.busy = true;
        self.status = if offset == 0 { "加载榜单…".into() } else { "加载更多…".into() };
        self.mark();
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = client.rank_list(&category_id, gender, offset, RANK_PAGE).await.map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::RankPage { category_id, gender, offset, result });
        });
    }

    /// 书架详情接口不给作者和最新章标题，逐本用阅读页 SSR 补齐，结果边到边显示。
    fn spawn_hydrate_shelf(&mut self) {
        let targets: Vec<(String, String)> = self.state.shelf.iter().filter(|b| b.needs_meta()).map(|b| (b.book_id.clone(), b.last_chapter_item_id.clone())).collect();
        if targets.is_empty() {
            return;
        }
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            for (i, (book_id, item_id)) in targets.into_iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(HYDRATE_GAP).await;
                }
                if let Ok(meta) = client.reader_meta(&item_id).await
                    && tx.send(WorkerMsg::ShelfMeta { book_id, author: meta.author, last_chapter_title: meta.chapter_title, creation_status: meta.creation_status }).is_err()
                {
                    return;
                }
            }
            let _ = tx.send(WorkerMsg::ShelfMetaDone);
        });
    }

    /// 加载封面：先看磁盘缓存，没有再下载并写入缓存。读盘和解码都放 spawn_blocking，避免阻塞网络任务。
    pub(crate) fn spawn_cover(&mut self, book_id: &str, url: &str) {
        if !self.state.settings.show_covers || url.is_empty() || self.covers.contains_key(book_id) || !self.cover_requested.insert(book_id.to_string()) {
            return;
        }
        let client = self.client.clone();
        let tx = self.tx.clone();
        let book_id = book_id.to_string();
        let url = url.to_string();
        self.rt.spawn(async move {
            let id = book_id.clone();
            let image = async {
                let cached = tokio::task::spawn_blocking({
                    let id = id.clone();
                    move || crate::cache::cover_get(&id)
                })
                .await
                .map_err(|e| e.to_string())?;
                let bytes = match cached {
                    Some(bytes) => bytes,
                    None => {
                        let bytes = client.fetch_bytes(&url).await.map_err(|e| e.to_string())?;
                        let _ = crate::cache::cover_put(&id, &bytes);
                        bytes
                    }
                };
                tokio::task::spawn_blocking(move || image::load_from_memory(&bytes).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
            }
            .await;
            let _ = tx.send(WorkerMsg::CoverDone { book_id, image });
        });
    }

    /// 绘制时记下的封面需求（书架里可见的卡片）在这一帧结束后统一发起。
    pub(super) fn flush_cover_requests(&mut self) {
        for (book_id, url) in std::mem::take(&mut self.cover_wanted) {
            self.spawn_cover(&book_id, &url);
        }
    }

    pub(super) fn spawn_chapter(&mut self, item_id: String) {
        self.ensure_reader_shell();
        let local = self.reader.as_ref().and_then(|r| r.demo_bodies.iter().find(|b| b.item_id == item_id).cloned()).or_else(|| crate::cache::get(&item_id));
        if let Some(body) = local {
            self.apply_chapter(body);
            self.mark();
            return;
        }
        self.busy = true;
        self.status = "拉取章节…".into();
        self.mark();
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = client.chapter(&item_id).await.map_err(|e| e.to_string());
            if let Ok(body) = &result {
                let _ = crate::cache::put(body);
            }
            let _ = tx.send(WorkerMsg::ChapterDone(result));
        });
    }

    /// 从当前章往后挑出还没缓存、也不在拉取中的章节，一次只跑一个后台任务（顺序拉，章与章之间留间隔）。
    pub(super) fn schedule_prefetch(&mut self) {
        let want = self.state.settings.prefetch as usize;
        if want == 0 || !self.prefetching.is_empty() {
            return;
        }
        let Some(r) = self.reader.as_ref() else { return };
        if r.book.book_id == "demo" {
            return;
        }
        let pending: Vec<String> = r.chapters.iter().skip(r.index + 1).take(want).map(|c| c.item_id.clone()).filter(|id| !crate::cache::has(id)).collect();
        if pending.is_empty() {
            return;
        }
        self.prefetching.extend(pending.iter().cloned());
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            for (i, item_id) in pending.into_iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(HYDRATE_GAP).await;
                }
                if let Ok(body) = client.chapter(&item_id).await {
                    let _ = crate::cache::put(&body);
                }
                if tx.send(WorkerMsg::Prefetched(item_id)).is_err() {
                    return;
                }
            }
        });
    }

    pub(super) fn spawn_shelf_refresh(&mut self) {
        if self.state.user.is_none() {
            self.status = "未登录，显示本地书架".into();
            self.mark();
            return;
        }
        self.busy = true;
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = client.bookshelf().await.map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::ShelfDone(result));
        });
    }

    pub(super) fn spawn_login(&mut self) {
        self.busy = true;
        self.login_msg = "正在获取二维码…".into();
        self.login_qr.clear();
        self.overlay = Overlay::None;
        self.push(Screen::Login);
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = auth::start_qr(&client).await.map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::QrReady(result));
        });
    }

    fn spawn_login_poll(&mut self, ticket: QrTicket) {
        self.busy = true;
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = async {
                let redirect = auth::poll_qr(&client, &ticket).await.map_err(|e| e.to_string())?;
                auth::finalize(&client, &redirect).await.map_err(|e| e.to_string())
            }
            .await;
            let _ = tx.send(WorkerMsg::LoginDone(result));
        });
    }

    fn spawn_progress_pull(&mut self) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = client.pull_progress().await.map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::ProgressDone(result));
        });
    }

    pub(super) fn spawn_update_check(&mut self) {
        let tx = self.tx.clone();
        self.rt.spawn_blocking(move || {
            let hint = crate::update::check_newer().ok().flatten();
            let _ = tx.send(WorkerMsg::UpdateHint(hint));
        });
    }

    pub(super) fn spawn_fontmap_refresh(&mut self) {
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = crate::font::init_if_needed().await.map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::FontmapDone(result));
        });
    }

    pub(super) fn spawn_fontmap_regenerate(&mut self) {
        self.busy = true;
        self.status = "正在生成字表…".into();
        self.mark();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = crate::font::regenerate().await.map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::FontmapDone(result));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::book_id_from_input;

    #[test]
    fn accepts_book_id_and_page_url() {
        assert_eq!(book_id_from_input("7143038691944959011"), Some("7143038691944959011".into()));
        assert_eq!(book_id_from_input(" https://fanqienovel.com/page/7143038691944959011?enter_from=stack-room "), Some("7143038691944959011".into()));
        assert_eq!(book_id_from_input("fanqienovel.com/page/7143038691944959011"), Some("7143038691944959011".into()));
        assert_eq!(book_id_from_input("1234"), None);
        assert_eq!(book_id_from_input("斗破苍穹"), None);
    }
}
