use crate::reflow::{Para, ParaKind, Reflow, detect, reflow};

/// 折行参数。任一值变化都要重新折行，所以一起作缓存键。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WrapOpts {
    pub line_gap: u8,
    pub reflow: Reflow,
    /// 段首缩进两个全角空格（只对 `<p>` 起头的正文段，标题、诗行和 `<br>` 续行不缩进）。
    pub para_indent: bool,
    /// 段与段之间空一行（`<br>` 续行前不空）。
    pub para_blank: bool,
    /// 伪装态给正文加的戏，原生阅读页用 None。
    pub deco: Deco,
}

/// 正文行之间插什么、每行按什么身份画。内容层的伪装靠它：正文本身要像被模仿工具的输出，不只是外面套个框。
/// 每种 agent 皮肤的一轮对话由不同的行组成（见 turn_block），行文本由皮肤在绘制时生成，这里只定身份和顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Deco {
    #[default]
    None,
    Claude,
    Codex,
    Grok,
    /// 手册页：正文之后空一行再加一行页脚，页脚是可滚动内容的一部分，滚到底才看得见。
    Man,
}

/// 每行的身份。带 u16 的是插入的假对话行，值是轮次序号（章首为 0），皮肤按它派生文案；这些行不计入正文位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Text,
    Blank,
    /// 章节标题行，agent 皮肤画成 markdown 标题。
    Heading,
    /// 回复的第一行。
    ReplyStart,
    /// 回复的第一行同时是章节标题。
    ReplyHeading,
    /// 用户输入行。
    Turn(u16),
    /// 用户输入带上下的空带行（Grok 的 vpad）。
    Band(u16),
    /// 工具调用头行。
    ToolCall(u16),
    /// 工具结果 / 子行。
    ToolResult(u16),
    /// 折叠的思考行或推理摘要行。
    Thinking(u16),
    /// 一轮结束的统计行。
    TurnEnd(u16),
    /// 手册页页脚。
    Footer,
}

impl LineKind {
    /// 是否是正文（计入正文位置）。
    pub fn is_content(self) -> bool {
        matches!(self, LineKind::Text | LineKind::Heading | LineKind::ReplyStart | LineKind::ReplyHeading)
    }
}

/// 每隔多少个正文段插一次用户输入。
const CHAT_TURN_EVERY: usize = 9;

/// 一轮对话在回复正文之前插的行。turn 0 是章首，前面没有上一轮，也不需要开头的空行；后续轮次紧跟在段间空行之后。
/// 顺序按各工具的真实会话：Claude 是 上一轮耗时 / 用户输入 / 思考 / 工具调用 + 结果；Codex 是 用户输入 / 工具调用 + 子行 / 推理摘要；
/// Grok 是 上一轮耗时 / 带空带的用户输入 / 工具行 / 思考行。思考行隔轮出现，避免每轮一模一样。
fn turn_block(deco: Deco, turn: u16) -> Vec<LineKind> {
    use LineKind::*;
    let think = turn % 2 == 1;
    let mut v = Vec::new();
    match deco {
        Deco::None | Deco::Man => {}
        Deco::Claude => {
            if turn > 0 {
                v.extend([TurnEnd(turn - 1), Blank]);
            }
            v.extend([Turn(turn), Blank]);
            if think {
                v.extend([Thinking(turn), Blank]);
            }
            v.extend([ToolCall(turn), ToolResult(turn), Blank]);
        }
        Deco::Codex => {
            v.extend([Turn(turn), Blank, ToolCall(turn), ToolResult(turn), Blank]);
            if think {
                v.extend([Thinking(turn), Blank]);
            }
        }
        Deco::Grok => {
            if turn > 0 {
                v.extend([TurnEnd(turn - 1), Blank]);
            }
            v.extend([Band(turn), Turn(turn), Band(turn), Blank, ToolCall(turn), Blank]);
            if think {
                v.extend([Thinking(turn), Blank]);
            }
        }
    }
    v
}

const INDENT: &str = "\u{3000}\u{3000}";
/// 缩进占 4 列，宽度不足时省掉，避免吃掉最小折行宽度。
const INDENT_MIN_WIDTH: usize = 12;

/// 按终端宽度预折行。宽度和参数不变时复用缓存，滚动只改行偏移。
#[derive(Debug, Default)]
pub struct WrapCache {
    width: u16,
    opts: WrapOpts,
    /// ensure 之后才有值；设置行用它显示 Auto 在本章落到了哪种模式。
    resolved: Option<Reflow>,
    source: String,
    lines: Vec<String>,
    /// 与 lines 平行：该行开头之前的正文非空白字符数。折行、整理、缩进、段距、插入的假对话行只增删空白或非正文内容，所以同一个值在任何参数下都指向同一处正文，进度按它保存就不随行号漂移。
    anchors: Vec<usize>,
    /// 与 lines 平行：每行的身份，伪装态按它画 glyph 和提示符。
    kinds: Vec<LineKind>,
}

impl WrapCache {
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.source = text.into();
        self.width = 0;
        self.resolved = None;
        self.lines.clear();
        self.anchors.clear();
        self.kinds.clear();
    }

    pub fn kinds(&self) -> &[LineKind] {
        &self.kinds
    }

    /// 返回是否真的重新折行了，调用方据此决定要不要按正文位置重算行偏移。
    pub fn ensure(&mut self, width: u16, opts: WrapOpts) -> bool {
        if width == 0 {
            return false;
        }
        if self.width == width && self.opts == opts && !self.lines.is_empty() {
            return false;
        }
        self.width = width;
        self.opts = opts;
        let resolved = opts.reflow.resolve_with(detect(&self.source));
        self.resolved = Some(resolved);
        let wrapped = wrap_lines(&self.source, width as usize, WrapOpts { reflow: resolved, ..opts });
        self.lines = wrapped.lines;
        self.anchors = wrapped.anchors;
        self.kinds = wrapped.kinds;
        true
    }

    pub fn resolved(&self) -> Option<Reflow> {
        self.resolved
    }

    /// 第 line 行开头对应的正文位置；还没折行时为 None。越界按最后一行算，End 键把偏移设成 usize::MAX 也能用。
    pub fn anchor_of(&self, line: usize) -> Option<usize> {
        let last = self.anchors.len().checked_sub(1)?;
        Some(self.anchors[line.min(last)])
    }

    /// 正文位置落在哪一行：最后一个 anchor <= 目标的行。空行的 anchor 与它后面那行相同，所以会落到有内容的行上。
    pub fn line_of(&self, anchor: usize) -> usize {
        self.anchors.partition_point(|a| *a <= anchor).saturating_sub(1)
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }
}

/// 段前插几个空行：`<br>` 续段不插，其余看开关。
fn blank_before(next: &Para, para_blank: bool) -> u8 {
    u8::from(next.block_start && para_blank)
}

#[cfg(test)]
pub fn wrap_text(text: &str, width: usize, opts: WrapOpts) -> Vec<String> {
    wrap_lines(text, width, opts).lines
}

fn nonblank_chars(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count()
}

#[derive(Default)]
struct Wrapped {
    lines: Vec<String>,
    anchors: Vec<usize>,
    kinds: Vec<LineKind>,
    /// 到目前为止已经排出的正文非空白字符数，也就是下一行的 anchor。
    seen: usize,
}

impl Wrapped {
    fn push(&mut self, line: String, kind: LineKind) {
        self.anchors.push(self.seen);
        if kind.is_content() {
            self.seen += nonblank_chars(&line);
        }
        self.lines.push(line);
        self.kinds.push(kind);
    }

    fn blank(&mut self) {
        self.push(String::new(), LineKind::Blank);
    }
}

/// 折行并给每行记下正文位置（见 WrapCache::anchors）和身份。
fn wrap_lines(text: &str, width: usize, opts: WrapOpts) -> Wrapped {
    let mut out = Wrapped::default();
    if width == 0 {
        return out;
    }
    let width = width.max(4);
    let paras = reflow(text, opts.reflow);
    let indent_ok = width >= INDENT_MIN_WIDTH;
    let chat = !matches!(opts.deco, Deco::None | Deco::Man);
    let mut reply_pending = false;
    let mut body_paras = 0usize;
    let mut turn: u16 = 0;
    for (i, para) in paras.iter().enumerate() {
        if i > 0 {
            for _ in 0..blank_before(para, opts.para_blank) {
                out.blank();
            }
        }
        if chat {
            // 章首一轮，之后每 CHAT_TURN_EVERY 个正文段再来一轮。
            if i == 0 || (para.kind == ParaKind::Body && body_paras > 0 && body_paras.is_multiple_of(CHAT_TURN_EVERY)) {
                for kind in turn_block(opts.deco, turn) {
                    out.push(String::new(), kind);
                }
                turn = turn.saturating_add(1);
                reply_pending = true;
            }
            if para.kind == ParaKind::Body {
                body_paras += 1;
            }
        }
        let indent = if indent_ok && opts.para_indent && para.block_start && para.kind == ParaKind::Body { INDENT } else { "" };
        let wrap_opts = textwrap::Options::new(width).break_words(true).initial_indent(indent);
        let heading = chat && para.kind == ParaKind::Heading;
        for line in textwrap::wrap(&para.text, wrap_opts) {
            let kind = match (reply_pending, heading) {
                (true, true) => LineKind::ReplyHeading,
                (true, false) => LineKind::ReplyStart,
                (false, true) => LineKind::Heading,
                (false, false) => LineKind::Text,
            };
            reply_pending = false;
            out.push(line.into_owned(), kind);
            for _ in 0..opts.line_gap {
                out.blank();
            }
        }
    }
    if out.lines.is_empty() {
        out.blank();
    }
    if opts.deco == Deco::Man {
        out.blank();
        out.push(String::new(), LineKind::Footer);
    }
    out
}

pub fn visible_window(lines: &[String], offset: usize, height: usize) -> (usize, &[String]) {
    if lines.is_empty() || height == 0 {
        return (0, &[]);
    }
    let max_off = lines.len().saturating_sub(1);
    let start = offset.min(max_off);
    let end = (start + height).min(lines.len());
    (start, &lines[start..end])
}

pub fn clamp_offset(offset: usize, line_count: usize, view_height: usize) -> usize {
    if line_count <= view_height {
        return 0;
    }
    offset.min(line_count - view_height)
}

pub fn demo_book() -> (crate::model::Book, Vec<crate::model::Chapter>, Vec<crate::model::ChapterBody>) {
    let book = crate::model::Book {
        book_id: "demo".into(),
        title: "演示小说".into(),
        author: "tomato-cli".into(),
        abstract_text: "本地三章演示，用来走通目录、跳转、滚动和主题。".into(),
        category: "演示".into(),
        chapter_count: 3,
        last_chapter_title: "第三章 设置与主题".into(),
        creation_status: Some(0),
        ..crate::model::Book::default()
    };
    let chapters = vec![
        crate::model::Chapter { item_id: "demo-1".into(), title: "第一章 滚动".into(), need_pay: false, published: 0 },
        crate::model::Chapter { item_id: "demo-2".into(), title: "第二章 目录跳转".into(), need_pay: false, published: 0 },
        crate::model::Chapter { item_id: "demo-3".into(), title: "第三章 设置与主题".into(), need_pay: false, published: 0 },
    ];
    let bodies = vec![demo_body("demo-1", "第一章 滚动", 1, "", "demo-2"), demo_body("demo-2", "第二章 目录跳转", 2, "demo-1", "demo-3"), demo_body("demo-3", "第三章 设置与主题", 3, "demo-2", "")];
    (book, chapters, bodies)
}

fn demo_body(id: &str, title: &str, seed: u32, pre: &str, next: &str) -> crate::model::ChapterBody {
    let mut content = format!("{title}\n\n这是内置演示正文。阅读器按显示宽度预折行，滚动只移动行偏移。按 t 打开目录，g 跳章，s 打开设置。\n\n");
    for i in 1..=40 {
        content.push_str(&format!("第{seed}章第{i}段。中文按单元格宽度折行。j/k 逐行，空格翻页，h/l 切章。主题和边距在设置里改，会立刻作用到这一屏。\n\n"));
    }
    crate::model::ChapterBody {
        item_id: id.into(),
        book_id: "demo".into(),
        book_name: "演示小说".into(),
        title: title.into(),
        content,
        pre_item_id: pre.into(),
        next_item_id: next.into(),
        need_pay: false,
        locked: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(reflow: Reflow, para_indent: bool, para_blank: bool) -> WrapOpts {
        WrapOpts { line_gap: 0, reflow, para_indent, para_blank, deco: Deco::None }
    }

    #[test]
    fn chat_deco_inserts_turns_and_keeps_anchors_aligned() {
        // 各段宽度不同，避免被当成等宽诗行。
        let text = (1..=12).map(|i| format!("第{i}段{}。", "字".repeat(i))).collect::<Vec<_>>().join("\n\n");
        let plain = wrap_lines(&text, 80, WrapOpts { para_blank: true, ..WrapOpts::default() });
        assert!(plain.kinds.iter().all(|k| matches!(k, LineKind::Text | LineKind::Blank)));
        let idx10 = plain.lines.iter().position(|l| l.starts_with("第10段")).unwrap();
        for deco in [Deco::Claude, Deco::Codex, Deco::Grok] {
            let w = wrap_lines(&text, 80, WrapOpts { para_blank: true, deco, ..WrapOpts::default() });
            assert_eq!(w.lines.len(), w.anchors.len());
            assert_eq!(w.lines.len(), w.kinds.len());
            // 章首先是第 0 轮的戏，正文第一行是回复起点。
            let block0 = turn_block(deco, 0);
            assert!(block0.contains(&LineKind::Turn(0)), "{deco:?}");
            assert_eq!(&w.kinds[..block0.len()], &block0[..], "{deco:?}");
            assert_eq!(w.kinds[block0.len()], LineKind::ReplyStart, "{deco:?}");
            assert!(w.lines[block0.len()].starts_with("第1段"));
            // 第 10 段前是第 1 轮，插入行文本为空、不计入正文位置。
            let turns: Vec<usize> = w.kinds.iter().enumerate().filter(|(_, k)| matches!(k, LineKind::Turn(_))).map(|(i, _)| i).collect();
            assert_eq!(turns.len(), 2, "{deco:?}");
            assert_eq!(w.kinds[turns[1]], LineKind::Turn(1));
            let reply = w.kinds.iter().enumerate().skip(turns[1]).find(|(_, k)| **k == LineKind::ReplyStart).map(|(i, _)| i).unwrap();
            assert!(w.lines[reply].starts_with("第10段"), "{deco:?}");
            assert_eq!(plain.anchors[idx10], w.anchors[reply], "{deco:?}");
            assert!(w.kinds.iter().zip(&w.lines).all(|(k, l)| k.is_content() || l.is_empty()), "{deco:?}");
        }
    }

    #[test]
    fn man_deco_appends_footer_after_blank() {
        let w = wrap_lines("甲。\n\n乙。", 20, WrapOpts { para_blank: true, deco: Deco::Man, ..WrapOpts::default() });
        assert_eq!(w.lines, ["甲。", "", "乙。", "", ""]);
        assert_eq!(w.kinds[3], LineKind::Blank);
        assert_eq!(w.kinds[4], LineKind::Footer);
        assert_eq!(w.anchors[4], 4);
    }

    #[test]
    fn chat_deco_marks_heading_as_reply_heading() {
        let w = wrap_lines("第一章 开始\n\n甲乙丙。", 40, WrapOpts { para_blank: true, deco: Deco::Claude, ..WrapOpts::default() });
        let head = w.kinds.iter().position(|k| *k == LineKind::ReplyHeading).unwrap();
        assert_eq!(w.lines[head], "第一章 开始");
        assert_eq!(w.anchors[head], 0);
        assert_eq!(w.kinds[head + 1], LineKind::Blank);
        assert_eq!(w.kinds[head + 2], LineKind::Text);
        assert_eq!(w.anchors[head + 2], 5);
    }

    #[test]
    fn wraps_cjk_to_width() {
        let lines = wrap_text("一二三四五六七八", 8, WrapOpts::default());
        assert_eq!(lines, vec!["一二三四".to_string(), "五六七八".to_string()]);
    }

    #[test]
    fn cache_skips_rebuild_on_same_width() {
        let mut cache = WrapCache::default();
        cache.set_text("一二三四");
        cache.ensure(4, WrapOpts::default());
        let ptr = cache.lines().as_ptr();
        cache.ensure(4, WrapOpts::default());
        assert_eq!(cache.lines().as_ptr(), ptr);
        cache.ensure(8, WrapOpts::default());
        assert_ne!(cache.lines().as_ptr(), ptr);
    }

    #[test]
    fn raw_blank_matches_legacy_output() {
        let lines = wrap_text("甲。\n\n乙。\n丙。", 20, WrapOpts { line_gap: 1, reflow: Reflow::Raw, para_indent: false, para_blank: true, deco: Deco::None });
        assert_eq!(lines, ["甲。", "", "", "乙。", "", "丙。", ""]);
    }

    #[test]
    fn indent_survives_wrap() {
        let lines = wrap_text("一二三四五六七八", 12, opts(Reflow::Raw, true, false));
        assert_eq!(lines, ["\u{3000}\u{3000}一二三四", "五六七八"]);
    }

    #[test]
    fn heading_not_indented_and_no_blank_when_para_blank_off() {
        let lines = wrap_text("第一章 开始\n\n正文。\n\n第二段。", 20, opts(Reflow::Raw, true, false));
        assert_eq!(lines, ["第一章 开始", "\u{3000}\u{3000}正文。", "\u{3000}\u{3000}第二段。"]);
    }

    #[test]
    fn br_line_neither_indented_nor_spaced() {
        let lines = wrap_text("甲。\n乙。\n\n丙。", 20, opts(Reflow::Raw, true, true));
        assert_eq!(lines, ["\u{3000}\u{3000}甲。", "乙。", "", "\u{3000}\u{3000}丙。"]);
    }

    #[test]
    fn indent_and_blank_are_independent() {
        assert_eq!(wrap_text("甲。\n\n乙。", 20, opts(Reflow::Raw, false, false)), ["甲。", "乙。"]);
        assert_eq!(wrap_text("甲。\n\n乙。", 20, opts(Reflow::Raw, false, true)), ["甲。", "", "乙。"]);
        assert_eq!(wrap_text("甲。\n\n乙。", 20, opts(Reflow::Raw, true, true)), ["\u{3000}\u{3000}甲。", "", "\u{3000}\u{3000}乙。"]);
    }

    #[test]
    fn narrow_width_skips_indent() {
        for width in [8, 11] {
            let lines = wrap_text("一二三四", width, opts(Reflow::Raw, true, false));
            assert_eq!(lines, ["一二三四"], "width {width}");
        }
    }

    #[test]
    fn empty_text_yields_one_blank_line() {
        assert_eq!(wrap_text("", 20, WrapOpts::default()), [""]);
    }

    #[test]
    fn merge_joins_unpunctuated_lines_inside_block() {
        let lines = wrap_text("风从窗缝里钻进来\n吹得帘子\n一抖一抖的。", 40, opts(Reflow::Merge, false, true));
        assert_eq!(lines, ["风从窗缝里钻进来吹得帘子一抖一抖的。"]);
    }

    #[test]
    fn anchors_point_to_same_text_after_rewrap() {
        let mut cache = WrapCache::default();
        cache.set_text("第一章 开始\n\n甲乙丙丁戊己庚辛壬癸。\n\n子丑寅卯辰巳午未申酉戌亥。");
        assert_eq!(cache.anchor_of(0), None);
        assert!(cache.ensure(10, opts(Reflow::Raw, false, true)));
        assert!(!cache.ensure(10, opts(Reflow::Raw, false, true)));
        let line = cache.lines().iter().position(|l| l.starts_with('子')).unwrap();
        let anchor = cache.anchor_of(line).unwrap();
        assert_eq!(anchor, 5 + 11);
        // 第二段折成多行，取它的第二行：anchor 等于标题字数加第一行字数。
        let first = cache.lines().iter().position(|l| l.starts_with('甲')).unwrap();
        let mid_anchor = cache.anchor_of(first + 1).unwrap();
        assert_eq!(mid_anchor, 5 + cache.lines()[first].chars().count());
        assert!(mid_anchor > 5 && mid_anchor < anchor);
        assert!(cache.ensure(40, opts(Reflow::Raw, true, false)));
        assert!(cache.lines()[cache.line_of(anchor)].trim_start().starts_with('子'));
        assert!(cache.lines()[cache.line_of(mid_anchor)].trim_start().starts_with('甲'));
        assert_eq!(cache.line_of(0), 0);
        assert_eq!(cache.line_of(usize::MAX), cache.len() - 1);
        assert_eq!(cache.anchor_of(usize::MAX), cache.anchor_of(cache.len() - 1));
    }

    #[test]
    fn cache_key_includes_opts_and_tracks_resolved() {
        let mut cache = WrapCache::default();
        cache.set_text("甲。\n\n乙。");
        assert_eq!(cache.resolved(), None);
        cache.ensure(20, opts(Reflow::Auto, false, true));
        let ptr = cache.lines().as_ptr();
        assert_eq!(cache.resolved(), Some(Reflow::Raw));
        cache.ensure(20, opts(Reflow::Auto, true, false));
        assert_ne!(cache.lines().as_ptr(), ptr);
        cache.set_text("丙。");
        assert_eq!(cache.resolved(), None);
        cache.ensure(20, opts(Reflow::Auto, true, false));
        assert!(cache.resolved().is_some());
    }
}
