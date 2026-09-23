use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::font::decode_pua;
use crate::html::html_to_text;
use crate::model::{Book, Chapter, ChapterBody, GENDER_FEMALE, GENDER_MALE, RankCategory};

pub(super) fn check_biz(json: &Value, what: &str) -> Result<()> {
    if let Some(code) = json.get("code") {
        let n = code.as_i64().or_else(|| code.as_u64().map(|u| u as i64)).unwrap_or(-1);
        if n != 0 {
            let msg = json.get("message").and_then(Value::as_str).unwrap_or("未知错误");
            return Err(anyhow!("{what}失败：{msg}"));
        }
    }
    Ok(())
}

/// novel.snssdk.com 搜索接口的 ret_data 条目：全字段明文，不带 PUA；没有章数和字数。
pub(super) fn parse_snssdk_book(v: &Value) -> Option<Book> {
    let id = json_str(v, &["book_id"], "");
    if id.is_empty() {
        return None;
    }
    Some(Book {
        book_id: id,
        title: json_str(v, &["title", "book_name"], ""),
        author: json_str(v, &["author"], ""),
        abstract_text: json_str(v, &["abstract"], ""),
        category: json_str(v, &["category"], ""),
        chapter_count: 0,
        last_chapter_title: String::new(),
        thumb_url: json_str(v, &["thumb_url"], ""),
        word_count: 0,
        read_count: 0,
        creation_status: json_i64(v, &["creation_status"]),
    })
}

/// 签名版搜索接口 `/api/author/search/search_book/v1` 的书目。书名、作者等字段带 PUA 字符，需要还原。
pub(super) fn parse_search_book(v: &Value) -> Option<Book> {
    let id = json_str(v, &["book_id", "bookId"], "");
    if id.is_empty() {
        return None;
    }
    Some(Book {
        book_id: id,
        title: decode_pua(&json_str(v, &["book_name", "bookName", "original_book_name"], "")),
        author: decode_pua(&json_str(v, &["author", "author_name"], "")),
        abstract_text: decode_pua(&json_str(v, &["book_abstract", "abstract"], "")),
        category: decode_pua(&json_str(v, &["category"], "")),
        chapter_count: json_str(v, &["serial_count"], "0").parse().unwrap_or(0),
        last_chapter_title: decode_pua(&json_str(v, &["last_chapter_title"], "")),
        thumb_url: json_str(v, &["thumb_url"], ""),
        word_count: json_u64(v, &["word_count", "word_number"]),
        read_count: json_u64(v, &["read_count"]),
        creation_status: json_i64(v, &["creation_status"]),
    })
}

/// 书籍页 `__INITIAL_STATE__.page`。
pub(super) fn parse_page_book(page: &Value, book_id: &str) -> Option<Book> {
    let title = json_str(page, &["bookName", "book_name"], "");
    if title.is_empty() {
        return None;
    }
    Some(Book {
        book_id: json_str(page, &["bookId", "book_id"], book_id),
        title,
        author: json_str(page, &["authorName", "author"], ""),
        abstract_text: json_str(page, &["abstract"], ""),
        category: page_category(page),
        chapter_count: json_u64(page, &["chapterTotal"]) as u32,
        last_chapter_title: json_str(page, &["lastChapterTitle"], ""),
        thumb_url: json_str(page, &["thumbUrl", "thumbUri"], ""),
        word_count: json_u64(page, &["wordNumber"]),
        read_count: json_u64(page, &["readCount"]),
        creation_status: json_i64(page, &["creationStatus"]),
    })
}

/// `category` 常为空，`categoryV2` 是 JSON 字符串数组，取各项 Name 用「/」连接。
fn page_category(page: &Value) -> String {
    let plain = json_str(page, &["category"], "");
    if !plain.is_empty() {
        return plain;
    }
    let raw = json_str(page, &["categoryV2"], "");
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&raw) else {
        return String::new();
    };
    items.iter().filter_map(|i| i.get("Name").and_then(Value::as_str)).collect::<Vec<_>>().join("/")
}

pub(super) fn parse_directory(data: &Value) -> Vec<Chapter> {
    let mut chapters = Vec::new();
    if let Some(vlist) = data.get("chapterListWithVolume").and_then(Value::as_array) {
        for vol in vlist {
            if let Some(items) = vol.as_array() {
                for item in items {
                    push_chapter(&mut chapters, item);
                }
            } else if let Some(items) = vol.get("chapterList").or_else(|| vol.get("chapter_list")).and_then(Value::as_array) {
                for item in items {
                    push_chapter(&mut chapters, item);
                }
            } else {
                push_chapter(&mut chapters, vol);
            }
        }
    }
    if chapters.is_empty()
        && let Some(flat) = data.get("chapterList").and_then(Value::as_array)
    {
        for item in flat {
            push_chapter(&mut chapters, item);
        }
    }
    chapters
}

fn push_chapter(out: &mut Vec<Chapter>, item: &Value) {
    let item_id = json_str(item, &["itemId", "item_id"], "");
    if item_id.is_empty() {
        return;
    }
    out.push(Chapter {
        item_id,
        title: json_str(item, &["title"], "未命名章节"),
        need_pay: item.get("needPay").and_then(Value::as_i64).unwrap_or(0) > 0,
        published: json_u64(item, &["firstPassTime", "first_pass_time"]),
    });
}

/// 章节接口和阅读页 SSR 的 chapterData 字段名一致。
pub(super) fn parse_chapter(d: &Value, item_id: &str) -> Result<ChapterBody> {
    let html = d.get("content").and_then(Value::as_str).unwrap_or("");
    if html.is_empty() {
        return Err(anyhow!("章节正文为空，可能需要登录或购买"));
    }
    Ok(ChapterBody {
        item_id: json_str(d, &["itemId", "item_id"], item_id),
        book_id: json_str(d, &["bookId", "book_id"], ""),
        book_name: json_str(d, &["bookName", "book_name"], ""),
        title: json_str(d, &["title"], "未命名章节"),
        content: html_to_text(html),
        pre_item_id: json_str(d, &["preItemId", "pre_item_id"], ""),
        next_item_id: json_str(d, &["nextItemId", "next_item_id"], ""),
        need_pay: d.get("needPay").and_then(Value::as_i64).unwrap_or(0) > 0,
        locked: d.get("isChapterLock").or_else(|| d.get("is_chapter_lock")).and_then(Value::as_bool).unwrap_or(false),
    })
}

pub(super) fn json_str(v: &Value, keys: &[&str], fallback: &str) -> String {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
            return s.to_string();
        }
        if let Some(n) = v.get(*k).and_then(Value::as_i64) {
            return n.to_string();
        }
        if let Some(n) = v.get(*k).and_then(Value::as_u64) {
            return n.to_string();
        }
    }
    fallback.to_string()
}

/// 番茄接口数字常以字符串下发，两种都接受。
pub(super) fn json_u64(v: &Value, keys: &[&str]) -> u64 {
    for k in keys {
        match v.get(*k) {
            Some(Value::Number(n)) => return n.as_u64().unwrap_or(0),
            Some(Value::String(s)) => {
                if let Ok(n) = s.trim().parse::<u64>() {
                    return n;
                }
            }
            _ => {}
        }
    }
    0
}

pub(super) fn json_i64(v: &Value, keys: &[&str]) -> Option<i64> {
    for k in keys {
        match v.get(*k) {
            Some(Value::Number(n)) => return n.as_i64(),
            Some(Value::String(s)) => {
                if let Ok(n) = s.trim().parse::<i64>() {
                    return Some(n);
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn app_query() -> String {
    "aid=1967&app_name=novelapp&version_code=57700&update_version_code=57700&device_platform=web&iid=0".into()
}

pub fn passport_query() -> String {
    "aid=2503&app_name=novelapp&version_code=57700&device_platform=web&channel=novel&sdk_version=1.6.1&passport_sdk_version=2.0.0&new_user=0".into()
}

/// /rank 页面 SSR 的 `rank.rankCategoryTypeList`：male / female 两组 `{id, name}`，按页面分组给 gender。
pub(super) fn parse_rank_categories(list: &Value) -> Vec<RankCategory> {
    let mut out = Vec::new();
    for (key, gender) in [("male", GENDER_MALE), ("female", GENDER_FEMALE)] {
        for c in list.get(key).and_then(Value::as_array).into_iter().flatten() {
            let id = json_str(c, &["id"], "");
            let name = json_str(c, &["name"], "");
            if !id.is_empty() && !name.is_empty() {
                out.push(RankCategory { id, name, gender });
            }
        }
    }
    out
}

/// 榜单接口 `book_list` 条目。书名、简介、最新章标题混有阅读页字体的 PUA，走 decode_pua；没有章数。
pub(super) fn parse_rank_book(v: &Value) -> Option<Book> {
    let id = json_str(v, &["bookId", "book_id"], "");
    if id.is_empty() {
        return None;
    }
    Some(Book {
        book_id: id,
        title: decode_pua(&json_str(v, &["bookName", "book_name"], "")),
        author: decode_pua(&json_str(v, &["author"], "")),
        abstract_text: decode_pua(&json_str(v, &["abstract"], "")),
        category: json_str(v, &["category"], ""),
        chapter_count: 0,
        last_chapter_title: decode_pua(&json_str(v, &["lastChapterTitle"], "")),
        thumb_url: json_str(v, &["thumbUri", "thumbUrl", "thumb_url"], ""),
        word_count: json_u64(v, &["wordNumber"]),
        read_count: json_u64(v, &["read_count", "readCount"]),
        creation_status: json_i64(v, &["creationStatus"]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_book_reads_category_v2_and_string_numbers() {
        let page = serde_json::json!({
            "bookId": "1", "bookName": "书", "authorName": "作者", "chapterTotal": 758, "wordNumber": "1585764",
            "category": "", "categoryV2": "[{\"Name\":\"都市\"},{\"Name\":\"系统\"}]", "creationStatus": "0", "lastChapterTitle": "尾声"
        });
        let b = parse_page_book(&page, "1").unwrap();
        assert_eq!(b.category, "都市/系统");
        assert_eq!(b.word_count, 1_585_764);
        assert_eq!(b.chapter_count, 758);
        assert_eq!(b.status_label(), "完结");
        let unknown = parse_page_book(&serde_json::json!({"bookName": "书"}), "2").unwrap();
        assert_eq!(unknown.status_label(), "");
    }

    #[test]
    fn snssdk_book_reads_plain_fields_and_string_status() {
        let v = serde_json::json!({"book_id": "7143038691944959011", "title": "十日终焉", "author": "杀虫队队员", "abstract": "简介", "category": "悬疑脑洞", "creation_status": "0", "thumb_url": "https://x/y.image", "score": "8.5"});
        let b = parse_snssdk_book(&v).unwrap();
        assert_eq!(b.title, "十日终焉");
        assert_eq!(b.category, "悬疑脑洞");
        assert_eq!(b.status_label(), "完结");
        assert_eq!(b.chapter_count, 0);
        assert!(parse_snssdk_book(&serde_json::json!({"title": "无 id"})).is_none());
    }

    #[test]
    fn chapter_reads_lock_flag() {
        let d = serde_json::json!({"itemId": "1", "title": "第一章", "content": "<p>正文</p>", "isChapterLock": true});
        assert!(parse_chapter(&d, "1").unwrap().locked);
        let open = serde_json::json!({"itemId": "2", "title": "第二章", "content": "<p>正文</p>"});
        assert!(!parse_chapter(&open, "2").unwrap().locked);
    }

    #[test]
    fn rank_categories_take_gender_from_group() {
        let list = serde_json::json!({"male": [{"id": "1141", "name": "西方奇幻"}, {"id": "", "name": "无 id"}], "female": [{"id": "1139", "name": "古风世情"}]});
        let cats = parse_rank_categories(&list);
        assert_eq!(cats.len(), 2);
        assert_eq!((cats[0].id.as_str(), cats[0].gender), ("1141", GENDER_MALE));
        assert_eq!((cats[1].name.as_str(), cats[1].gender), ("古风世情", GENDER_FEMALE));
    }

    #[test]
    fn rank_book_reads_camel_fields_and_string_numbers() {
        let v = serde_json::json!({"bookId": "7638213476862659609", "bookName": "书", "author": "作者", "abstract": "简介", "creationStatus": "1", "currentPos": "99", "lastChapterTitle": "第两百章", "readCount": "0", "read_count": "38620", "wordNumber": "663212", "thumbUri": "https://x/y.image"});
        let b = parse_rank_book(&v).unwrap();
        assert_eq!(b.title, "书");
        assert_eq!(b.status_label(), "连载");
        assert_eq!(b.word_count, 663_212);
        assert_eq!(b.read_count, 38_620);
        assert_eq!(b.thumb_url, "https://x/y.image");
        assert!(parse_rank_book(&serde_json::json!({"bookName": "无 id"})).is_none());
    }

    #[test]
    fn directory_flattens_volumes() {
        let data = serde_json::json!({"chapterListWithVolume": [[{"itemId": "a", "title": "一", "needPay": 0}], [{"itemId": "b", "title": "二", "needPay": 1}]]});
        let ch = parse_directory(&data);
        assert_eq!(ch.len(), 2);
        assert!(ch[1].need_pay);
    }
}
