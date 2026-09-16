//! 番茄网页把数据以 `window.__INITIAL_STATE__={...}` 内联在 HTML 里。
//! 网页版 `/api/*` 大多要求浏览器 SDK 生成的 a_bogus 签名，原生客户端拿到的是空响应；书籍页和阅读页的 SSR 数据不需要签名。

use anyhow::{Result, anyhow};
use serde_json::Value;

const MARKER: &str = "window.__INITIAL_STATE__=";

/// 从页面 HTML 里取出 `__INITIAL_STATE__` 对象。JSON 之后可能紧跟 `;</script>`，所以只反序列化第一个值。
pub fn initial_state(html: &str) -> Result<Value> {
    let start = html.find(MARKER).ok_or_else(|| anyhow!("页面没有 __INITIAL_STATE__"))?;
    let rest = &html[start + MARKER.len()..];
    let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
    match stream.next() {
        Some(Ok(v)) => Ok(v),
        Some(Err(e)) => Err(anyhow!("解析页面数据：{e}")),
        None => Err(anyhow!("页面数据为空")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_first_json_value_and_ignores_trailer() {
        let html = r#"<script>window.__INITIAL_STATE__={"page":{"bookName":"书","chapterTotal":2},"x":[1,2]};</script><script>other()</script>"#;
        let v = initial_state(html).unwrap();
        assert_eq!(v["page"]["bookName"], "书");
        assert_eq!(v["x"][1], 2);
    }

    #[test]
    fn missing_marker_is_error() {
        assert!(initial_state("<html></html>").is_err());
    }
}
