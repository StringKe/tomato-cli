//! 正文段落整理。`Reflow` 决定哪些源行拼成同一段；段首缩进和段间空行是渲染层的两个独立开关（`reader::WrapOpts`），与这里无关。
//! 输入是 `html::html_to_text` 的输出：`<p>` 边界是空行，`<br>` 是单个换行，连续空行至多 1 个。

use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthStr;

const TERM: &[char] = &['。', '！', '？', '；', '…', '—', '～', '.', '!', '?', ';', '~'];
const CONT: &[char] = &['，', '、', '：', ',', ':'];
/// 显示宽度超过它的源行本身就是完整段落，不再往里并短行。
const FRAG_MAX_CELLS: usize = 60;
/// 合并段累积到这个宽度、且上一句已收尾时另起一段。
const PARA_TARGET_CELLS: usize = 120;
/// 累积不足它时，以引号开头的行也并进来（「“嗯。”他转身走了。」），够了才让对话另起一段。
const PARA_MIN_CELLS: usize = 40;
const CLOSE: &[char] = &['”', '’', '」', '』', '）', '】', '》', '〉', '〕', '〗', '〙', '〛', '"', '\'', ')', ']', '}'];
const OPEN: &[char] = &['“', '‘', '「', '『', '（', '【', '《', '〈', '〔', '"'];
const HEADING_KEYS: &[char] = &['章', '节', '回', '卷', '部', '集', '篇', '话', '幕'];
const HEADING_NUM: &[char] = &['〇', '零', '一', '二', '三', '四', '五', '六', '七', '八', '九', '十', '百', '千', '两', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];
const HEADING_WORDS: &[&str] = &["序章", "序幕", "楔子", "引子", "尾声", "终章", "番外", "后记", "前言", "作者的话"];
/// 标题关键字之后允许出现的分隔符；其余字符说明这是正文里的「第三章的内容」一类短语。
const HEADING_SEP: &[char] = &[' ', '\u{3000}', '：', ':', '·', '-', '—', '、'];
const HEADING_MAX_CELLS: usize = 40;
const VERSE_MIN_LINES: usize = 4;
const VERSE_MAX_CELLS: usize = 20;
const AUTO_MIN_LINES: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reflow {
    #[default]
    Auto,
    Raw,
    Merge,
}

impl Reflow {
    pub const ALL: [Reflow; 3] = [Reflow::Auto, Reflow::Raw, Reflow::Merge];

    pub fn name(self) -> &'static str {
        match self {
            Reflow::Auto => "自动",
            Reflow::Raw => "原样",
            Reflow::Merge => "合并",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// Auto 按判定落到 Raw 或 Merge，其余原样返回。
    pub fn resolve_with(self, verdict: Verdict) -> Reflow {
        match self {
            Reflow::Auto if verdict.fragmented() => Reflow::Merge,
            Reflow::Auto => Reflow::Raw,
            other => other,
        }
    }

    pub fn resolve(self, text: &str) -> Reflow {
        self.resolve_with(detect(text))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParaKind {
    Heading,
    Body,
    Verse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Para {
    pub kind: ParaKind,
    pub text: String,
    /// 该段在源文里前面有空行（`<p>` 边界）或是第一段。false 表示来自 `<br>` 单换行，渲染时不插段间空行、不缩进。
    pub block_start: bool,
}

/// 一章的统计特征。lines 不含标题行、诗行和冒号结尾行；nonterm 是其中句子没有收尾（逗号结尾或根本没有标点）的行数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Verdict {
    pub lines: usize,
    pub nonterm: usize,
}

impl Verdict {
    /// 阈值 25%：正常网文的段落几乎都以句号问号收尾，碎行章节 30% 到 70% 的行停在半句；短章和占位文本落在 lines < 8。
    pub fn fragmented(self) -> bool {
        self.lines >= AUTO_MIN_LINES && self.nonterm.saturating_mul(4) >= self.lines
    }

    pub fn nonterm_percent(self) -> u8 {
        if self.lines == 0 {
            return 0;
        }
        (self.nonterm * 100 / self.lines).min(100) as u8
    }
}

/// 一个非空源行。空行本身不出现，只用来决定 block_start（前面是 `<p>` 边界还是 `<br>`）。
struct Line<'a> {
    text: &'a str,
    block_start: bool,
}

fn split_lines(text: &str) -> Vec<Line<'_>> {
    let raw: Vec<&str> = text.split('\n').map(str::trim).collect();
    let mut out = Vec::with_capacity(raw.len());
    for (i, line) in raw.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        out.push(Line { text: line, block_start: i == 0 || raw[i - 1].is_empty() });
    }
    out
}

pub fn is_heading(line: &str) -> bool {
    if line.width() > HEADING_MAX_CELLS {
        return false;
    }
    if let Some(rest) = line.strip_prefix('第') {
        let n = rest.chars().take_while(|c| HEADING_NUM.contains(c)).count();
        if (1..=8).contains(&n) {
            let mut tail = rest.chars().skip(n);
            if tail.next().is_some_and(|c| HEADING_KEYS.contains(&c)) && tail.next().is_none_or(|c| HEADING_SEP.contains(&c)) {
                return true;
            }
        }
    }
    HEADING_WORDS.iter().any(|w| line.strip_prefix(w).is_some_and(|rest| rest.chars().next().is_none_or(|c| HEADING_SEP.contains(&c))))
}

/// 行尾（忽略收尾引号括号）的最后一个字符。
fn last_char(line: &str) -> Option<char> {
    line.trim_end_matches(CLOSE).chars().last()
}

/// 句子已收尾：句号问号等，或者引语收口。
fn terminated(line: &str) -> bool {
    last_char(line).is_some_and(|c| TERM.contains(&c))
}

/// 停在半句上（逗号顿号冒号结尾），下一行必须接着念。
fn dangling(line: &str) -> bool {
    last_char(line).is_some_and(|c| CONT.contains(&c))
}

/// 没有任何文字的行（`***`、`——` 之类的分隔符），自己占一段，两边都不并。
fn is_divider(line: &str) -> bool {
    !line.chars().any(char::is_alphanumeric)
}

/// 两个源行拼成一段。`<p>` 边界上两个都没标点的中文句子之间补一个逗号，否则「他推开门屋里没有灯」会粘成一句；
/// 英文两侧补空格；`<br>` 硬折行直接相连。
fn join(a: &str, b: &str, block_boundary: bool) -> String {
    let tail = a.chars().last();
    let head = b.chars().next();
    let both_words = tail.is_some_and(char::is_alphanumeric) && head.is_some_and(char::is_alphanumeric);
    if !both_words {
        return format!("{a}{b}");
    }
    let ascii = tail.is_some_and(|c| c.is_ascii()) || head.is_some_and(|c| c.is_ascii());
    if ascii {
        format!("{a} {b}")
    } else if block_boundary {
        format!("{a}，{b}")
    } else {
        format!("{a}{b}")
    }
}

/// 累积段 acc 是否在 next 之前另起一段。停在半句上的段无论如何都要接着并；acc_real 表示 acc 起自一个本来就够长的源段，句子收尾后就不再往里并短行。
fn breaks_before(acc: &str, acc_real: bool, next: &str) -> bool {
    if dangling(acc) {
        return false;
    }
    let acc_w = acc.width();
    is_divider(acc)
        || is_divider(next)
        || next.width() > FRAG_MAX_CELLS
        || (acc_real && terminated(acc))
        || (acc_w >= PARA_TARGET_CELLS && terminated(acc))
        || (next.starts_with(OPEN) && acc_w >= PARA_MIN_CELLS)
}

/// 连续 ≥ 4 个等宽短 Body 段视为诗词，整组标记为 Verse。源空行不打断，标题打断。
fn mark_verse(paras: &mut [Para]) {
    let eligible = |p: &Para| p.kind == ParaKind::Body && p.text.width() <= VERSE_MAX_CELLS && !p.text.starts_with(OPEN);
    let mut i = 0;
    while i < paras.len() {
        if !eligible(&paras[i]) {
            i += 1;
            continue;
        }
        let w = paras[i].text.width();
        let mut j = i + 1;
        while j < paras.len() && eligible(&paras[j]) && paras[j].text.width() == w {
            j += 1;
        }
        if j - i >= VERSE_MIN_LINES {
            for p in &mut paras[i..j] {
                p.kind = ParaKind::Verse;
            }
        }
        i = j;
    }
}

/// 每个非空行一个段并标出标题和诗行。Raw 直接用它，Merge 以它为起点拼接，detect 用它决定哪些行不计入统计。
fn classify(lines: &[Line]) -> Vec<Para> {
    let mut paras: Vec<Para> = lines.iter().map(|l| Para { kind: if is_heading(l.text) { ParaKind::Heading } else { ParaKind::Body }, text: l.text.to_string(), block_start: l.block_start }).collect();
    mark_verse(&mut paras);
    paras
}

pub fn detect(text: &str) -> Verdict {
    let lines = split_lines(text);
    let paras = classify(&lines);
    let mut verdict = Verdict::default();
    for (line, para) in lines.iter().zip(&paras) {
        if para.kind != ParaKind::Body || is_divider(line.text) {
            continue;
        }
        // 「xx道：」接引语在正常文本里很常见，计入会抬高占比。
        if line.text.trim_end_matches(CLOSE).ends_with(['：', ':']) {
            continue;
        }
        verdict.lines += 1;
        if !terminated(line.text) {
            verdict.nonterm += 1;
        }
    }
    verdict
}

/// Auto 内部先 resolve；返回的段序列供 reader::wrap_text 折行。
/// Merge 把连续的短行并成正常长度的段落：停在半句上的一定接着并，句子收尾且累积够长、遇到分隔符或本来就够长的段落、
/// 累积够长后遇到引语行，才另起一段。标题和诗行永远自己一段。
pub fn reflow(text: &str, mode: Reflow) -> Vec<Para> {
    let mode = if mode == Reflow::Auto { mode.resolve(text) } else { mode };
    let lines = split_lines(text);
    let paras = classify(&lines);
    if mode != Reflow::Merge {
        return paras;
    }
    let mut out = Vec::with_capacity(paras.len());
    // 累积段和它是否起自一个本来就够长的源段。
    let mut cur: Option<(Para, bool)> = None;
    for (line, para) in lines.iter().zip(paras) {
        if para.kind != ParaKind::Body {
            out.extend(cur.take().map(|(p, _)| p));
            out.push(para);
            continue;
        }
        match cur.as_mut() {
            Some((acc, real)) if !breaks_before(&acc.text, *real, line.text) => acc.text = join(&acc.text, line.text, line.block_start),
            _ => {
                out.extend(cur.take().map(|(p, _)| p));
                let real = line.text.width() > FRAG_MAX_CELLS;
                cur = Some((para, real));
            }
        }
    }
    out.extend(cur.map(|(p, _)| p));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(paras: &[Para]) -> Vec<&str> {
        paras.iter().map(|p| p.text.as_str()).collect()
    }

    fn kinds(paras: &[Para]) -> Vec<ParaKind> {
        paras.iter().map(|p| p.kind).collect()
    }

    const FRAGMENTED: &str = "夜色渐深，\n\n林远推开窗，\n\n风灌了进来。\n\n“你还没睡？”\n\n苏晚站在门口，\n\n手里端着一杯茶。\n\n“睡不着。”\n\n他推开门，\n\n屋里没有灯。\n\n窗外的雨还没停，屋檐下的水一滴一滴落在石阶上，";

    const NORMAL: &str = "第一句话。\n\n第二句话说完了。\n\n第三句话很长很长很长。\n\n第四句。\n\n第五句话说完了吗。\n\n第六句话又说了一遍。\n\n第七句。\n\n第八句话说完了。\n\n第九句话很长很长。\n\n第十句话说完了吗。";

    #[test]
    fn t01_short_sentences_merge_only_in_merge_mode() {
        let paras = reflow("他推开门。\n\n屋里没人。", Reflow::Merge);
        assert_eq!(texts(&paras), ["他推开门。屋里没人。"]);
        assert!(paras[0].block_start);
        let paras = reflow("他推开门。\n\n屋里没人。", Reflow::Raw);
        assert_eq!(texts(&paras), ["他推开门。", "屋里没人。"]);
    }

    #[test]
    fn t02_comma_fragments_across_p_merge() {
        let paras = reflow("夜色渐深，\n\n林远推开窗，\n\n风灌了进来。", Reflow::Merge);
        assert_eq!(texts(&paras), ["夜色渐深，林远推开窗，风灌了进来。"]);
    }

    #[test]
    fn t03_unpunctuated_p_lines_join_with_comma_and_divider_stays() {
        let paras = reflow("他推开门\n\n屋里没有灯\n\n***\n\n她来了。", Reflow::Merge);
        assert_eq!(texts(&paras), ["他推开门，屋里没有灯", "***", "她来了。"]);
    }

    #[test]
    fn t04_br_hard_wrap_joins_without_comma() {
        let paras = reflow("风从窗缝里钻进来，吹得帘子\n一抖一抖的。\n他往前走了两步。", Reflow::Merge);
        assert_eq!(texts(&paras), ["风从窗缝里钻进来，吹得帘子一抖一抖的。他往前走了两步。"]);
        assert!(paras[0].block_start);
    }

    #[test]
    fn t05_short_dialogue_lines_fold_into_one_paragraph() {
        let paras = reflow("他看着她，\n\n“你还好吗？”\n\n“还好。”\n\n她说，\n\n手却在抖。", Reflow::Merge);
        assert_eq!(texts(&paras), ["他看着她，“你还好吗？”“还好。”她说，手却在抖。"]);
    }

    #[test]
    fn t06_comma_inside_quote_continues() {
        let paras = reflow("“好，”\n\n他说，\n\n“走吧。”", Reflow::Merge);
        assert_eq!(texts(&paras), ["“好，”他说，“走吧。”"]);
    }

    #[test]
    fn t07_colon_leads_into_dialogue() {
        let paras = reflow("他开口道：\n\n“走吧。”", Reflow::Merge);
        assert_eq!(texts(&paras), ["他开口道：“走吧。”"]);
    }

    #[test]
    fn t08_short_terminated_lines_still_merge() {
        let paras = reflow("他愣住了……\n半晌没说话。\nthis...\nno!", Reflow::Merge);
        assert_eq!(texts(&paras), ["他愣住了……半晌没说话。this...no!"]);
    }

    #[test]
    fn t09_ascii_join_adds_space() {
        let paras = reflow("the quick brown\nfox jumps.", Reflow::Merge);
        assert_eq!(texts(&paras), ["the quick brown fox jumps."]);
    }

    #[test]
    fn t10_heading_blocks_merge_on_both_sides() {
        let paras = reflow("第三章 夜谈\n夜色渐深，\n风起了。\n他说，\n第四章 归来", Reflow::Merge);
        assert_eq!(texts(&paras), ["第三章 夜谈", "夜色渐深，风起了。他说，", "第四章 归来"]);
        assert_eq!(kinds(&paras), [ParaKind::Heading, ParaKind::Body, ParaKind::Heading]);
    }

    #[test]
    fn t11_heading_suffix_check() {
        assert!(!is_heading("第三章的内容他早已看过"));
        assert!(is_heading("第三章"));
        assert!(is_heading("第十二章 夜谈"));
        assert!(is_heading("第3章：归来"));
        assert!(is_heading("楔子"));
        assert!(is_heading("番外 番外一"));
        assert!(!is_heading("番外篇的故事"));
    }

    #[test]
    fn t12_verse_protected_in_both_modes() {
        let text = "床前明月光，\n\n疑是地上霜。\n\n举头望明月，\n\n低头思故乡。";
        let expected = ["床前明月光，", "疑是地上霜。", "举头望明月，", "低头思故乡。"];
        for mode in [Reflow::Merge, Reflow::Raw] {
            let paras = reflow(text, mode);
            assert_eq!(texts(&paras), expected);
            assert!(paras.iter().all(|p| p.kind == ParaKind::Verse));
        }
    }

    #[test]
    fn t13_raw_never_joins() {
        let paras = reflow("夜色渐深，\n\n林远推开窗，\n\n风灌了进来。", Reflow::Raw);
        assert_eq!(paras.len(), 3);
    }

    #[test]
    fn t14_empty_input() {
        assert!(reflow("", Reflow::Merge).is_empty());
        assert!(reflow("\n\n", Reflow::Merge).is_empty());
    }

    #[test]
    fn t15_detect_normal_text() {
        let verdict = detect(NORMAL);
        assert_eq!(verdict.lines, 10);
        assert!(!verdict.fragmented());
    }

    #[test]
    fn t16_detect_fragmented_text() {
        let verdict = detect(FRAGMENTED);
        assert_eq!((verdict.lines, verdict.nonterm), (10, 5));
        assert!(verdict.fragmented());
        assert_eq!(verdict.nonterm_percent(), 50);
    }

    #[test]
    fn t17_detect_counts_unpunctuated_lines_as_nonterm() {
        let text = "他推开门\n\n屋里没有灯\n\n她站在那里没动\n\n风\n\n窗外一片漆黑\n\n没有人说话\n\n他往前走了两步\n\n灯亮了\n\n她转过身来看着他\n\n什么都没说";
        let verdict = detect(text);
        assert_eq!((verdict.lines, verdict.nonterm), (10, 10));
        assert!(verdict.fragmented());
    }

    #[test]
    fn t18_detect_short_chapter_not_fragmented() {
        let verdict = detect("夜色渐深，\n\n林远推开窗，\n\n风灌了进来，\n\n她站在门口，\n\n手里端着一杯茶，");
        assert_eq!(verdict.nonterm, 5);
        assert!(!verdict.fragmented());
    }

    #[test]
    fn t19_detect_skips_colon_lines() {
        let text = "第一句话。\n\n第二句话说完了。\n\n第三句话很长很长很长。\n\n第四句。\n\n第五句话说完了吗。\n\n第六句话又说了一遍。\n\n第七句。\n\n第八句话说完了。\n\n他开口道：\n\n她冷冷地说：";
        assert_eq!(detect(text), Verdict { lines: 8, nonterm: 0 });
    }

    #[test]
    fn t20_auto_resolves_by_verdict() {
        assert_eq!(Reflow::Auto.resolve(FRAGMENTED), Reflow::Merge);
        assert_eq!(Reflow::Auto.resolve(NORMAL), Reflow::Raw);
        assert_eq!(Reflow::Raw.resolve(FRAGMENTED), Reflow::Raw);
        assert_eq!(Reflow::Merge.resolve(NORMAL), Reflow::Merge);
    }

    #[test]
    fn t21_two_char_lines_merge() {
        let paras = reflow("他抬头。\n\n嗯。\n\n她说。\n\n走吧。", Reflow::Merge);
        assert_eq!(texts(&paras), ["他抬头。嗯。她说。走吧。"]);
    }

    #[test]
    fn t22_long_paragraph_stays_alone_unless_previous_line_dangles() {
        let long = "夜色像一块湿透的毯子压在城墙上，守夜的兵丁裹紧了袍子，把火把往怀里拢了拢，远处的灯火比往常少了一半，没有人知道为什么。";
        assert!(long.width() > FRAG_MAX_CELLS);
        let paras = reflow(&format!("{long}\n\n他说。\n\n走吧。"), Reflow::Merge);
        assert_eq!(texts(&paras), [long, "他说。走吧。"]);
        let paras = reflow(&format!("他看着她，\n\n{long}\n\n她没说话。"), Reflow::Merge);
        assert_eq!(texts(&paras), [format!("他看着她，{long}"), "她没说话。".to_string()]);
    }

    #[test]
    fn t23_dialogue_starts_new_paragraph_once_enough_text_accumulated() {
        let paras = reflow("夜色渐深，林远推开窗，风灌了进来，屋里的灯晃了晃。\n\n“你还没睡？”\n\n“睡不着。”", Reflow::Merge);
        assert_eq!(texts(&paras), ["夜色渐深，林远推开窗，风灌了进来，屋里的灯晃了晃。", "“你还没睡？”“睡不着。”"]);
    }

    #[test]
    fn t24_target_length_breaks_only_at_sentence_end() {
        let sentence = "这是一句二十个字左右的普通句子写在这里。";
        assert_eq!(sentence.width(), 40);
        let text = [sentence; 5].join("\n\n");
        let paras = reflow(&text, Reflow::Merge);
        assert_eq!(texts(&paras), [sentence.repeat(3), sentence.repeat(2)]);
        let dangling = "这是一句二十个字左右停在半句上的句子写在这，";
        let text = format!("{sentence}\n\n{sentence}\n\n{dangling}\n\n{sentence}");
        let paras = reflow(&text, Reflow::Merge);
        assert_eq!(paras.len(), 1);
    }
}
