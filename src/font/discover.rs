use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use anyhow::{bail, Context, Result};

const REFERER: &str = "https://fanqienovel.com/";
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
const MAX_DISCOVER: usize = 12;

// 只写公开章节/榜单页，从页面 @font-face 发现官方 WOFF2，不写死字体文件地址。
const SEED_PAGES: &[&str] = &[
    "https://fanqienovel.com/reader/7107551541946485797",
    "https://fanqienovel.com/reader/7107551538414881805",
    "https://fanqienovel.com/reader/6589911810021786126",
    "https://fanqienovel.com/reader/7590788018722587198",
    "https://fanqienovel.com/reader/7107551535822801956",
    "https://fanqienovel.com/rank",
    "https://fanqienovel.com/",
];

pub(super) fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder().user_agent(UA).timeout(Duration::from_secs(20)).redirect(reqwest::redirect::Policy::limited(8)).build().context("创建 HTTP 客户端")
}

async fn fetch_text(client: &reqwest::Client, url: &str) -> Result<String> {
    let resp = client.get(url).header("Referer", REFERER).header("Accept", "text/html,application/xhtml+xml,application/json;q=0.9,*/*;q=0.8").send().await.with_context(|| format!("请求 {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("HTTP {status}");
    }
    resp.text().await.context("读取正文")
}

pub(super) async fn fetch_font_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = client.get(url).header("Referer", REFERER).header("Accept", "font/woff2,font/woff,*/*").send().await.with_context(|| format!("请求 {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("HTTP {status}");
    }
    Ok(resp.bytes().await.context("读取字体")?.to_vec())
}

fn is_blocked_host(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.contains("github.com") || u.contains("githubusercontent.com") || u.contains("jsdelivr") || u.contains("zwb8926")
}

fn is_font_url(url: &str) -> bool {
    if is_blocked_host(url) {
        return false;
    }
    let u = url.to_ascii_lowercase();
    let abs = u.starts_with("https://") || u.starts_with("http://") || u.starts_with("//");
    let ext = u.contains(".woff2") || u.contains(".woff") || u.contains(".ttf") || u.contains(".otf");
    abs && ext
}

fn abs_url(raw: &str) -> String {
    let u = raw.trim();
    if u.starts_with("https://") || u.starts_with("http://") {
        u.to_string()
    } else if let Some(rest) = u.strip_prefix("//") {
        format!("https://{rest}")
    } else if u.starts_with('/') {
        format!("https://fanqienovel.com{u}")
    } else {
        format!("https://fanqienovel.com/{u}")
    }
}

fn font_url_rank(url: &str) -> (u8, u8, u8) {
    let u = url.to_ascii_lowercase();
    let woff2 = u.contains(".woff2") as u8;
    let regular = (!u.contains("-500.") && !u.contains("-700.")) as u8;
    let official = (u.contains("awesome-font") || u.contains("bytetos.com") || u.contains("byteimg.com") || u.contains("fqnovel")) as u8;
    (woff2, regular, official)
}

fn take_css_url(s: &str) -> Option<(&str, String)> {
    let i = s.find("url(")?;
    let mut t = s[i + 4..].trim_start();
    if t.is_empty() {
        return None;
    }
    if t.starts_with(['\'', '"']) {
        let q = t.as_bytes()[0];
        t = &t[1..];
        let end = t.as_bytes().iter().position(|&b| b == q || b == b')')?;
        let url = t[..end].trim().to_string();
        Some((&t[end..], url))
    } else {
        let end = t.find(')')?;
        let url = t[..end].trim().trim_matches(['\'', '"']).to_string();
        Some((&t[end..], url))
    }
}

fn collect_urls_from(text: &str, out: &mut Vec<String>) {
    let mut rest = text;
    while let Some((_, url)) = take_css_url(rest) {
        if let Some(i) = rest.find("url(") {
            rest = &rest[i + 4..];
        } else {
            break;
        }
        if is_font_url(&url) {
            out.push(abs_url(&url));
        }
    }
    let mut search = text;
    while let Some(i) = search.find(".woff") {
        let prefix = &search[..i];
        let start = prefix.rfind("https://").or_else(|| prefix.rfind("http://")).or_else(|| prefix.rfind("//")).unwrap_or(prefix.len());
        let tail = &search[start..];
        let n = tail.find(|c: char| c.is_whitespace() || matches!(c, ')' | '\'' | '"' | '<' | '>' | '\\' | ',')).unwrap_or(tail.len());
        let url = &tail[..n];
        if is_font_url(url) {
            out.push(abs_url(url));
        }
        search = &search[i + 5..];
    }
}

fn extract_font_urls(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("@font-face") {
        rest = &rest[i + 10..];
        let Some(brace) = rest.find('{') else { break };
        rest = &rest[brace + 1..];
        let Some(end) = rest.find('}') else { break };
        collect_urls_from(&rest[..end], &mut out);
        rest = &rest[end + 1..];
    }
    collect_urls_from(text, &mut out);
    let mut seen = HashSet::new();
    out.retain(|u| {
        if is_blocked_host(u) || seen.contains(u) {
            return false;
        }
        seen.insert(u.clone())
    });
    out.sort_by_key(|a| std::cmp::Reverse(font_url_rank(a)));
    out
}

fn extract_prefixed_ids(html: &str, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find(prefix) {
        rest = &rest[i + prefix.len()..];
        let n = rest.bytes().take_while(u8::is_ascii_digit).count();
        if n >= 10 {
            out.push(rest[..n].to_string());
        }
        rest = &rest[n..];
    }
    let mut seen = HashSet::new();
    out.retain(|id| seen.insert(id.clone()));
    out
}

fn collect_item_ids(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, val) in m {
                if k.eq_ignore_ascii_case("itemid") || k.eq_ignore_ascii_case("item_id") {
                    if let Some(s) = val.as_str() {
                        if s.len() >= 10 && s.bytes().all(|b| b.is_ascii_digit()) {
                            out.push(s.to_string());
                        }
                    } else if let Some(n) = val.as_u64() {
                        let s = n.to_string();
                        if s.len() >= 10 {
                            out.push(s);
                        }
                    }
                }
                collect_item_ids(val, out);
            }
        }
        serde_json::Value::Array(a) => {
            for x in a {
                collect_item_ids(x, out);
            }
        }
        _ => {}
    }
}

fn enqueue_unique(queue: &mut VecDeque<String>, seen: &HashSet<String>, url: String) {
    if !seen.contains(&url) && !queue.contains(&url) {
        queue.push_back(url);
    }
}

pub(super) async fn discover_font_url(client: &reqwest::Client) -> Result<String> {
    let mut queue = VecDeque::new();
    for u in SEED_PAGES {
        queue.push_back((*u).to_string());
    }
    let mut seen = HashSet::new();
    let mut last_err = String::from("未在公开章节页找到官方 WOFF2");
    while let Some(url) = queue.pop_front() {
        if seen.len() >= MAX_DISCOVER {
            break;
        }
        if !seen.insert(url.clone()) {
            continue;
        }
        match fetch_text(client, &url).await {
            Ok(body) => {
                if let Some(font) = extract_font_urls(&body).into_iter().find(|u| u.contains(".woff2") && !is_blocked_host(u)) {
                    return Ok(font);
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                    let mut ids = Vec::new();
                    collect_item_ids(&json, &mut ids);
                    for id in ids.into_iter().take(4) {
                        enqueue_unique(&mut queue, &seen, format!("https://fanqienovel.com/reader/{id}"));
                        enqueue_unique(&mut queue, &seen, format!("https://fanqienovel.com/api/reader/full?itemId={id}"));
                    }
                    let mut fonts = Vec::new();
                    walk_json_fonts(&json, &mut fonts);
                    if let Some(font) = fonts.into_iter().find(|u| u.contains(".woff2") && !is_blocked_host(u)) {
                        return Ok(font);
                    }
                }
                for id in extract_prefixed_ids(&body, "/reader/").into_iter().take(4) {
                    enqueue_unique(&mut queue, &seen, format!("https://fanqienovel.com/reader/{id}"));
                }
                for id in extract_prefixed_ids(&body, "/page/").into_iter().take(3) {
                    enqueue_unique(&mut queue, &seen, format!("https://fanqienovel.com/page/{id}"));
                    enqueue_unique(&mut queue, &seen, format!("https://fanqienovel.com/api/reader/directory/detail?bookId={id}"));
                }
            }
            Err(e) => last_err = format!("{url}: {e}"),
        }
    }
    bail!("{last_err}")
}

fn walk_json_fonts(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => {
            if s.contains("@font-face") || s.contains(".woff") || s.contains("awesome-font") {
                collect_urls_from(s, out);
                if is_font_url(s) {
                    out.push(abs_url(s));
                }
            }
        }
        serde_json::Value::Array(a) => {
            for x in a {
                walk_json_fonts(x, out);
            }
        }
        serde_json::Value::Object(m) => {
            for x in m.values() {
                walk_json_fonts(x, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_minified_font_face() {
        let html = r#"@font-face{font-family:X;src:url(https://lf6-awef.bytetos.com/obj/awesome-font/c/dc027189e0ba4cd.woff2)format("woff2")}"#;
        let urls = extract_font_urls(html);
        assert!(urls.iter().any(|u| u.ends_with(".woff2") && u.contains("awesome-font")));
    }

    #[test]
    fn rejects_github_font_urls() {
        let html = r#"@font-face{src:url(https://raw.githubusercontent.com/zwb8926/fanqie-novel-vscode/main/x.woff2)}"#;
        assert!(extract_font_urls(html).is_empty());
    }
}
