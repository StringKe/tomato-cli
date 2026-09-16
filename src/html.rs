use crate::font::decode_pua;

/// 章节 HTML 转纯文本，再做 PUA 字体还原。
pub fn html_to_text(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    let plain = html2text::from_read(content.as_bytes(), usize::MAX).unwrap_or_else(|_| fallback_strip(content));
    decode_pua(&collapse_blank_lines(&plain))
}

fn fallback_strip(content: &str) -> String {
    html2text::from_read(content.replace('<', " <").as_bytes(), usize::MAX).unwrap_or_else(|_| content.to_string())
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::new();
    let mut blank = 0u8;
    for line in s.lines() {
        let t = line.trim();
        if t.is_empty() {
            blank = blank.saturating_add(1);
            if blank <= 1 && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        blank = 0;
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_paragraphs() {
        let text = html_to_text("<article><p>第一段</p><p>第二段</p></article>");
        assert!(text.contains("第一段"));
        assert!(text.contains("第二段"));
    }

    #[test]
    fn br_is_single_newline() {
        assert_eq!(html_to_text("<p>a<br>b</p><p>c</p>"), "a\nb\n\nc");
    }
}
