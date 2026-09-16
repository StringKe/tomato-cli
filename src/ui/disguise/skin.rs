//! 皮肤常量与文案生成。字符串、字形、颜色、行数全部来自对本机 Claude Code 2.1.273 / Codex 0.154.0 / Grok Build 1.0.30 二进制与源码、
//! macOS git 2.55 + less 668 + mandoc 的取证（2026-09），被模仿的 CLI 改版时只改这里。绘制逻辑在 mod.rs。

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::model::LayoutId;
use crate::reader::Deco;
use crate::theme::terminal_is_dark;

// ---------- Claude Code ----------

pub(super) const CLAUDE_TITLE: &str = "✳ Claude Code";
/// 回复首行和工具调用行的点，macOS 上是 ⏺。
pub(super) const CLAUDE_GLYPH: &str = "⏺";
pub(super) const CLAUDE_PROMPT: &str = "❯";
/// 工具结果行前缀：2 空格 + ⎿ + 空格 + NBSP，正文从第 5 列起。
pub(super) const CLAUDE_BRANCH: &str = "⎿";
/// 工作行往返播放的帧。
pub(super) const CLAUDE_SPINNER: [&str; 6] = ["·", "✢", "✳", "✶", "✻", "✽"];
/// 回合结束行和思考行的字形。
pub(super) const CLAUDE_STAR: &str = "✻";
pub(super) const CLAUDE_THINKING: &str = "Thinking…";
pub(super) const CLAUDE_INTERRUPT: &str = "esc to interrupt";
pub(super) const CLAUDE_EXPAND: &str = "(ctrl+o to expand)";
/// 状态行左侧的权限模式徽标和它后面的切换提示。
pub(super) const CLAUDE_MODE: &str = "⏵⏵ accept edits on";
pub(super) const CLAUDE_MODE_HINT: &str = "(shift+tab to cycle)";
/// 工作行动词，取自真实列表的一部分。
pub(super) const CLAUDE_VERBS: [&str; 24] = ["Brewing", "Cogitating", "Contemplating", "Crafting", "Deliberating", "Elucidating", "Envisioning", "Forging", "Generating", "Hatching", "Ideating", "Imagining", "Incubating", "Inferring", "Mulling", "Musing", "Pondering", "Perusing", "Ruminating", "Simmering", "Synthesizing", "Thinking", "Unravelling", "Working"];
/// 回合结束行的过去式动词。
pub(super) const CLAUDE_PAST: [&str; 8] = ["Baked", "Brewed", "Churned", "Cogitated", "Cooked", "Crunched", "Sautéed", "Worked"];
/// 输入框占位：`Try "<例子>"`。
pub(super) const CLAUDE_TRY: &str = "Try";

pub(super) const CLAUDE_ORANGE: Color = Color::Rgb(215, 119, 87);

fn dark() -> bool {
    terminal_is_dark().unwrap_or(true)
}

fn pick(dark_c: Color, light_c: Color) -> Color {
    if dark() { dark_c } else { light_c }
}

pub(super) fn claude_success() -> Color {
    pick(Color::Rgb(78, 186, 101), Color::Rgb(44, 122, 57))
}

pub(super) fn claude_auto_accept() -> Color {
    pick(Color::Rgb(175, 135, 255), Color::Rgb(135, 0, 255))
}

pub(super) fn claude_prompt_border() -> Color {
    pick(Color::Rgb(136, 136, 136), Color::Rgb(153, 153, 153))
}

pub(super) fn claude_subtle() -> Color {
    pick(Color::Rgb(80, 80, 80), Color::Rgb(175, 175, 175))
}

/// 用户消息整行的底色；终端深浅未知时不铺。
pub(super) fn claude_user_bg() -> Option<Color> {
    terminal_is_dark().map(|d| if d { Color::Rgb(55, 55, 55) } else { Color::Rgb(240, 240, 240) })
}

// ---------- Codex ----------

/// Codex 空闲时的终端标题只剩项目名，与假目录 `~/docs` 对应。
pub(super) const CODEX_TITLE: &str = "docs";
pub(super) const CODEX_BULLET: &str = "•";
pub(super) const CODEX_PROMPT: &str = "›";
pub(super) const CODEX_BRANCH: &str = "└";
pub(super) const CODEX_PLACEHOLDER: &str = "Ask Codex to do anything";
pub(super) const CODEX_WORKING: &str = "Working";
pub(super) const CODEX_INTERRUPT: &str = "esc to interrupt";
pub(super) const CODEX_BANNER_MARK: &str = ">_";
pub(super) const CODEX_BANNER_NAME: &str = "OpenAI Codex";
pub(super) const CODEX_VERSION: &str = "(v0.154.0)";
pub(super) const CODEX_MODEL: &str = "gpt-6-astra medium";
pub(super) const CODEX_MODEL_HINT: &str = "/model";
pub(super) const CODEX_MODEL_HINT_TAIL: &str = " to change";
pub(super) const CODEX_DIRECTORY: &str = "~/docs";
/// banner 框内宽上限。
pub(super) const CODEX_BANNER_INNER: u16 = 56;
/// banner 框下方的提示行（非首次会话），缩进 2 列。
pub(super) const CODEX_TIP_LABEL: &str = "Tip:";
pub(super) const CODEX_TIP_HEAD: &str = " Try the ";
pub(super) const CODEX_TIP_BOLD: &str = "Desktop app";
pub(super) const CODEX_TIP_TAIL: &str = ". Run 'codex app' or visit https://chatgpt.com/codex?app-landing-page=true";
pub(super) const CODEX_EXPLORED: &str = "Explored";
pub(super) const CODEX_RAN: &str = "Ran";
pub(super) const CODEX_READ: &str = "Read";
pub(super) const CODEX_SEARCH: &str = "Search";
pub(super) const CODEX_LIST: &str = "List";
pub(super) const CODEX_IN: &str = " in ";
/// 推理摘要行（dim italic），隔轮出现。
pub(super) const CODEX_REASONING: [&str; 3] = ["Reviewing the chapter", "Summarizing the next section", "Checking story continuity"];

// ---------- Grok Build ----------

pub(super) const GROK_TITLE: &str = "grok";
pub(super) const GROK_BULLET: &str = "◆";
pub(super) const GROK_PROMPT: &str = "❯";
pub(super) const GROK_PLACEHOLDER: &str = "Build anything";
pub(super) const GROK_SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
pub(super) const GROK_RESPONDING: &str = "Responding…";
pub(super) const GROK_MODEL: &str = "Grok 4.6 (xhigh)";
pub(super) const GROK_MODE: &str = "always-approve";
pub(super) const GROK_DOT: &str = " · ";
pub(super) const GROK_BAR: [(&str, &str); 2] = [("Shift+Tab", "mode"), ("Ctrl+x", "shortcuts")];
pub(super) const GROK_BAR_SEP: &str = "  │  ";
pub(super) const GROK_CWD: &str = "~/docs";
pub(super) const GROK_CONTEXT: &str = "500K";
pub(super) const GROK_DASHBOARD: &str = "[Dashboard]";
pub(super) const GROK_STOP: &str = "[stop]";
pub(super) const GROK_TOKENS_GLYPH: &str = "⇣";
pub(super) const GROK_THOUGHT: &str = "Thought";
pub(super) const GROK_WORKED: &str = "Worked for";
pub(super) const GROK_READ: &str = "Read";
pub(super) const GROK_RUN: &str = "Run";
pub(super) const GROK_SEARCH: &str = "Search";
/// 输入框左缩进和右留白。
pub(super) const GROK_BOX_PAD: u16 = 2;

pub(super) fn grok_gray() -> Color {
    pick(Color::Rgb(108, 108, 108), Color::Rgb(118, 118, 118))
}

pub(super) fn grok_gray_bright() -> Color {
    pick(Color::Rgb(120, 120, 120), Color::Rgb(98, 98, 98))
}

pub(super) fn grok_gray_dim() -> Color {
    pick(Color::Rgb(88, 88, 88), Color::Rgb(165, 165, 165))
}

pub(super) fn grok_path() -> Color {
    pick(Color::Rgb(255, 158, 100), Color::Rgb(195, 105, 30))
}

pub(super) fn grok_command() -> Color {
    pick(Color::Rgb(224, 175, 104), Color::Rgb(162, 118, 18))
}

pub(super) fn grok_magenta() -> Color {
    pick(Color::Rgb(187, 154, 247), Color::Rgb(125, 75, 198))
}

pub(super) fn grok_teal() -> Color {
    pick(Color::Rgb(26, 188, 156), Color::Rgb(10, 142, 112))
}

pub(super) fn grok_border() -> Color {
    pick(Color::Rgb(80, 80, 88), Color::Rgb(165, 165, 175))
}

pub(super) fn grok_text() -> Color {
    pick(Color::Rgb(225, 225, 225), Color::Rgb(38, 38, 38))
}

pub(super) fn grok_text_secondary() -> Color {
    pick(Color::Rgb(200, 200, 200), Color::Rgb(68, 68, 68))
}

/// 用户输入带的底色；终端深浅未知时退化为粗体（对应 grok 的 terminal 主题）。
pub(super) fn grok_band() -> Option<Color> {
    terminal_is_dark().map(|d| if d { Color::Rgb(36, 36, 36) } else { Color::Rgb(222, 222, 222) })
}

// ---------- git log / man / less ----------

pub(super) const GITLOG_TITLE: &str = "git log";
pub(super) const GITLOG_HEAD_COLOR: Color = Color::Yellow;
pub(super) const GITLOG_AUTHOR: &str = "Author: dev <dev@localhost>";
pub(super) const GITLOG_DATE_LABEL: &str = "Date:";
/// git log 里 Author / Date 的值都从第 8 列起。
pub(super) const GITLOG_VALUE_COL: u16 = 8;
/// commit 说明行缩进 4 列。
pub(super) const GITLOG_SUBJECT_COL: u16 = 4;
/// `git log -p` 的正文：新增行整行绿色，行首一列 `+`。
pub(super) const DIFF_ADD_COLOR: Color = Color::Green;
pub(super) const DIFF_HUNK_COLOR: Color = Color::Cyan;
pub(super) const DIFF_PLUS: &str = "+";

pub(super) const MAN_TITLE: &str = "man";
pub(super) const MAN_NAME: &str = "NAME";
pub(super) const MAN_SECTION: &str = "DESCRIPTION";
pub(super) const MAN_CENTER: &str = "Miscellaneous Information Manual";
/// NAME 节里名字和一句话之间的 en dash（mdoc 页的写法）。
pub(super) const MAN_DASH: &str = "–";
pub(super) const MAN_OS: &str = "macOS 27.0";
/// mandoc 的正文缩进。
pub(super) const MAN_INDENT: u16 = 5;

/// less 的命令提示符和到底标记。
pub(super) const PAGER_PROMPT: &str = ":";
pub(super) const PAGER_SEARCH: &str = "/";
pub(super) const PAGER_END: &str = "(END)";

pub(super) const NATIVE_TITLE: &str = "tomato";

// ---------- 皮肤表 ----------

pub(super) struct Skin {
    pub id: LayoutId,
    /// 正文左缩进列数：Claude / Codex 2（glyph 栏），Grok 5（外边距 2 + accent 1 + 内边距 2），GitLog 1（放 `+`），Man 5。
    pub indent: u16,
    /// 正文右侧留白：Codex 2、Grok 5（内边距 2 + 外边距 2 + 滚动条 1），其余 1 到 2。
    pub right_pad: u16,
    /// 顶栏行数：Codex 9（banner 框 6 行、空行、Tip、空行），GitLog 12（commit 头 6 行加 diff 头 6 行），Man 6（标题 / 空 / NAME / 名字行 / 空 / DESCRIPTION），Grok 2（空行加 header）。
    pub top_rows: u16,
    /// 顶栏是否固定不随内容滚动（Grok 的 header 行）。
    pub top_fixed: bool,
    /// 固定底栏行数：Claude 4（横线 / 提示符 / 横线 / 状态），Codex 4（上边距 / 提示符 / 下边距 / 状态），Grok 6（框 3 / 空 / 快捷键条 / 空），pager 1。
    pub bottom_rows: u16,
    /// 高度不足时底栏退化后的行数。
    pub compact_bottom: u16,
    /// busy 时底栏上方再加的行数（工作行，Codex / Grok 后面还跟一个空行）。
    pub work_rows: u16,
    /// 进入伪装态时写入的终端标题。
    pub title: &'static str,
}

pub(super) fn skin(id: LayoutId) -> Skin {
    match id {
        LayoutId::Native => Skin { id, indent: 0, right_pad: 0, top_rows: 0, top_fixed: false, bottom_rows: 0, compact_bottom: 0, work_rows: 0, title: NATIVE_TITLE },
        LayoutId::Claude => Skin { id, indent: 2, right_pad: 1, top_rows: 0, top_fixed: false, bottom_rows: 4, compact_bottom: 1, work_rows: 1, title: CLAUDE_TITLE },
        LayoutId::Codex => Skin { id, indent: 2, right_pad: 2, top_rows: 9, top_fixed: false, bottom_rows: 4, compact_bottom: 2, work_rows: 2, title: CODEX_TITLE },
        LayoutId::Grok => Skin { id, indent: 5, right_pad: 5, top_rows: 2, top_fixed: true, bottom_rows: 6, compact_bottom: 3, work_rows: 2, title: GROK_TITLE },
        LayoutId::GitLog => Skin { id, indent: 1, right_pad: 1, top_rows: 12, top_fixed: false, bottom_rows: 1, compact_bottom: 1, work_rows: 0, title: GITLOG_TITLE },
        LayoutId::Man => Skin { id, indent: MAN_INDENT, right_pad: 2, top_rows: 6, top_fixed: false, bottom_rows: 1, compact_bottom: 1, work_rows: 0, title: MAN_TITLE },
    }
}

/// 每种皮肤要给正文加的戏：agent 皮肤是各自的对话脚本，pager 皮肤不加。
pub(super) fn deco_of(id: LayoutId) -> Deco {
    match id {
        LayoutId::Claude => Deco::Claude,
        LayoutId::Codex => Deco::Codex,
        LayoutId::Grok => Deco::Grok,
        LayoutId::Man => Deco::Man,
        LayoutId::Native | LayoutId::GitLog => Deco::None,
    }
}

/// Claude 状态行的上下文余量：快满时红色提示，其余时候只报「距离压缩还剩」。
pub(super) fn claude_context(percent: usize) -> Line<'static> {
    let left = 100usize.saturating_sub(percent);
    if left <= 5 {
        Line::from(Span::styled(format!("Context low ({left}% remaining) · Run /compact to compact & continue"), Style::new().fg(claude_error())))
    } else {
        Line::from(Span::styled(format!("{left}% until auto-compact"), dim()))
    }
}

pub(super) fn claude_error() -> Color {
    pick(Color::Rgb(255, 107, 128), Color::Rgb(171, 43, 63))
}

pub(super) fn cover_title(id: LayoutId) -> &'static str {
    skin(id).title
}

/// 给 app 层用的终端标题。Man 皮肤在阅读页带上假的手册名，其余皮肤标题固定。
pub(crate) fn cover_window_title(id: LayoutId, index: Option<usize>) -> String {
    match (id, index) {
        (LayoutId::Man, Some(i)) => format!("man {}", fake_man_name(i)),
        _ => cover_title(id).to_string(),
    }
}

/// 伪装态下切换整理模式的反馈，用被模仿工具的口吻写，不出现中文。
pub(crate) fn reflow_note(mode: crate::reflow::Reflow, fragmented: bool) -> String {
    use crate::reflow::Reflow;
    match mode {
        Reflow::Auto if fragmented => "reflow: auto (merged)".into(),
        Reflow::Auto => "reflow: auto (raw)".into(),
        Reflow::Raw => "reflow: raw".into(),
        Reflow::Merge => "reflow: merge".into(),
    }
}

// ---------- 假数据 ----------

fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// 同一章同一轮次稳定的伪随机数。
pub(super) fn seed(item_id: &str, turn: u16) -> u64 {
    fnv1a(fnv1a(FNV_BASIS, item_id.as_bytes()), &turn.to_le_bytes())
}

/// 40 位小写 hex，看起来像 SHA-1：三轮 FNV-1a 64 链式取值拼成 48 位再截断。
pub(super) fn fake_hash(item_id: &str) -> String {
    let h1 = fnv1a(FNV_BASIS, item_id.as_bytes());
    let h2 = fnv1a(h1, item_id.as_bytes());
    let h3 = fnv1a(h2, item_id.as_bytes());
    let mut s = format!("{h1:016x}{h2:016x}{h3:016x}");
    s.truncate(40);
    s
}

pub(super) fn fake_path(index: usize) -> String {
    format!("docs/ch-{:04}.md", index + 1)
}

pub(super) fn fake_man_name(index: usize) -> String {
    format!("ch-{:04}(7)", index + 1)
}

pub(super) fn gitlog_subject(index: usize) -> String {
    format!("docs: sync ch-{:04}", index + 1)
}

/// `git log -p` 里 commit 头之后的 diff 头六行：新文件整篇是新增行。只有一行时 hunk 头不带 `,1`。
pub(super) fn diff_header(index: usize, item_id: &str, lines: usize) -> [String; 6] {
    let path = fake_path(index);
    let blob = &fake_hash(item_id)[..7];
    let hunk = if lines == 1 { "@@ -0,0 +1 @@".to_string() } else { format!("@@ -0,0 +1,{lines} @@") };
    [format!("diff --git a/{path} b/{path}"), "new file mode 100644".into(), format!("index 0000000..{blob}"), "--- /dev/null".into(), format!("+++ b/{path}"), hunk]
}

/// commit 行的装饰：括号和箭头黄色，HEAD 粗体青，分支名粗体绿。
pub(super) fn gitlog_decoration() -> Vec<Span<'static>> {
    let yellow = Style::new().fg(GITLOG_HEAD_COLOR);
    vec![Span::styled("(", yellow), Span::styled("HEAD", Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)), Span::styled(" -> ", yellow), Span::styled("main", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)), Span::styled(")", yellow)]
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_LONG: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// 章节对应的假日期：由当天往前推 hash % 365 天，时分秒取 hash 的不同字节，同一章一天内稳定。
fn fake_civil(item_id: &str, now_ms: u64) -> (i64, u32, u32, u64, u64, u64, usize) {
    let hash = fnv1a(FNV_BASIS, item_id.as_bytes());
    let today = (now_ms / 86_400_000) as i64;
    let day = today - (hash % 365) as i64;
    let hour = (hash >> 8) % 24;
    let minute = (hash >> 16) % 60;
    let second = (hash >> 24) % 60;
    let (year, month, d) = civil_from_days(day);
    // 1970-01-01 是周四，Sun 起算偏移 4。
    let weekday = (day + 4).rem_euclid(7) as usize;
    (year, month, d, hour, minute, second, weekday)
}

/// git log 风格日期，时区写死 +0800。日不补位，与 git 默认格式一致（"Sep 7"）。
pub(super) fn fake_date(item_id: &str, now_ms: u64) -> String {
    let (year, month, d, hour, minute, second, weekday) = fake_civil(item_id, now_ms);
    format!("{} {} {d} {hour:02}:{minute:02}:{second:02} {year} +0800", WEEKDAYS[weekday], MONTHS[(month - 1) as usize])
}

/// man 页脚的日期，如 `September 7, 2026`。
pub(super) fn man_date(item_id: &str, now_ms: u64) -> String {
    let (year, month, d, ..) = fake_civil(item_id, now_ms);
    format!("{} {d}, {year}", MONTHS_LONG[(month - 1) as usize])
}

/// Grok 用户行右侧的时间戳（`8:19 AM`），每轮往后推几分钟。
pub(super) fn fake_clock(item_id: &str, now_ms: u64, turn: u16) -> String {
    let (.., hour, minute, _, _) = fake_civil(item_id, now_ms);
    let total = (hour * 60 + minute + u64::from(turn) * 3) % 1440;
    let (h, m) = (total / 60, total % 60);
    let (h12, ampm) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    format!("{h12}:{m:02} {ampm}")
}

/// Howard Hinnant 的 days_from_civil 逆运算：epoch 起天数 -> (年, 月, 日)。
pub(super) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// 一轮假对话的耗时，12 到 230 秒。
pub(super) fn turn_seconds(item_id: &str, turn: u16) -> u64 {
    12 + seed(item_id, turn) % 219
}

/// Claude Code 的时长：`36s`、`2m 1s`、`3m`。
pub(super) fn claude_duration(secs: u64) -> String {
    match (secs / 60, secs % 60) {
        (0, s) => format!("{s}s"),
        (m, 0) => format!("{m}m"),
        (m, s) => format!("{m}m {s}s"),
    }
}

/// Codex 的时长：`0s`、`59s`、`1m 00s`。
pub(super) fn codex_duration(secs: u64) -> String {
    match (secs / 3600, (secs / 60) % 60, secs % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m {s:02}s"),
        (h, m, s) => format!("{h}h {m:02}m {s:02}s"),
    }
}

/// Grok 的时长：十秒内一位小数，一分钟内整秒，之后 `1m20s`。
pub(super) fn grok_duration(tenths: u64) -> String {
    let secs = tenths / 10;
    if secs < 10 {
        format!("{}.{}s", secs, tenths % 10)
    } else if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs / 60) % 60)
    }
}

/// Claude Code 的紧凑 token 数：`892`、`3.2k`、`27.0k`（千位以上一位小数）。
pub(super) fn claude_tokens(n: usize) -> String {
    if n < 1000 { n.to_string() } else { format!("{:.1}k", n as f64 / 1000.0) }
}

/// Grok 工作行的 token 数：三位有效数字，`912`、`1.23k`、`12.3k`、`123k`。
pub(super) fn grok_tokens(n: usize) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 10_000 {
        format!("{:.2}k", n as f64 / 1000.0)
    } else if n < 100_000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        format!("{}k", n / 1000)
    }
}

/// Grok header 行的 token 用量：`1.5K`、`12K`。
pub(super) fn grok_header_tokens(n: usize) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 10_000 {
        format!("{:.1}K", n as f64 / 1000.0)
    } else {
        format!("{}K", n / 1000)
    }
}

/// 正文字数换算成像样的 token 数：中文大约一字一个多 token。
pub(super) fn tokens_for(chars: usize) -> usize {
    chars + chars / 3 + 210
}

// ---------- 对话文案 ----------

/// 用户输入：章首是让它看文件，之后每轮都是「继续」。中文小说配中文提示词，比英文更像真实会话。
pub(super) fn turn_text(index: usize, turn: u16) -> String {
    if turn == 0 { format!("看看 {}", fake_path(index)) } else { "继续".into() }
}

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// 每轮工具调用的花样：章首固定读文件，之后按章和轮次轮换。
fn tool_variant(index: usize, turn: u16) -> usize {
    if turn == 0 { 0 } else { (index + turn as usize) % 4 }
}

/// 工具调用头行（不含 glyph 栏）。
pub(super) fn tool_call(deco: Deco, index: usize, turn: u16, lines: usize) -> Line<'static> {
    let path = fake_path(index);
    let variant = tool_variant(index, turn);
    match deco {
        Deco::None | Deco::Man => Line::default(),
        Deco::Claude => match variant {
            1 => Line::from(vec![Span::styled("Bash", bold()), Span::raw(format!("(wc -l {path})"))]),
            2 => Line::from(vec![Span::styled("Search", bold()), Span::raw(format!("(pattern: \"ch-{:04}\", glob: \"docs/**\")", index + 1))]),
            _ => Line::from(vec![Span::styled("Read", bold()), Span::raw(format!("({path})"))]),
        },
        Deco::Codex => match variant {
            1 => Line::from(vec![Span::styled(CODEX_RAN, bold()), Span::raw(format!(" wc -l {path}"))]),
            _ => Line::from(Span::styled(CODEX_EXPLORED, bold())),
        },
        Deco::Grok => match variant {
            1 => Line::from(vec![Span::styled(GROK_RUN, bold()), Span::raw(" "), Span::styled(format!("wc -l {path}"), Style::new().fg(grok_command()))]),
            2 => Line::from(vec![Span::styled(GROK_SEARCH, bold()), Span::raw(" "), Span::styled(format!("ch-{:04}", index + 1), Style::new().fg(grok_command())), Span::styled(" (1 file)", dim())]),
            _ => Line::from(vec![Span::styled(GROK_READ, bold()), Span::raw(" "), Span::styled(path, Style::new().fg(grok_path())), Span::styled(format!(" (1-{lines})"), dim())]),
        },
    }
}

/// 工具结果 / 子行（不含前缀栏）。Grok 的结果折叠在头行里，没有这一行。
pub(super) fn tool_result(deco: Deco, index: usize, turn: u16, lines: usize) -> Line<'static> {
    let path = fake_path(index);
    let variant = tool_variant(index, turn);
    match deco {
        Deco::None | Deco::Man | Deco::Grok => Line::default(),
        Deco::Claude => match variant {
            1 => Line::from(format!("{lines} {path}")),
            2 => Line::from(vec![Span::raw("Found "), Span::styled("1 ", bold()), Span::raw("file "), Span::styled(CLAUDE_EXPAND, dim())]),
            3 => Line::from(vec![Span::raw("Read "), Span::styled(lines.to_string(), bold()), Span::raw(if lines == 1 { " line " } else { " lines " }), Span::styled(CLAUDE_EXPAND, dim())]),
            _ => Line::from(vec![Span::raw("Read "), Span::styled(lines.to_string(), bold()), Span::raw(if lines == 1 { " line" } else { " lines" })]),
        },
        Deco::Codex => {
            let verb = |s: &'static str| Span::styled(s, Style::new().fg(Color::Cyan));
            match variant {
                1 => Line::from(Span::styled(format!("{lines} {path}"), dim())),
                2 => Line::from(vec![verb(CODEX_SEARCH), Span::raw(format!(" ch-{:04}", index + 1)), Span::styled(CODEX_IN, dim()), Span::raw("docs")]),
                3 => Line::from(vec![verb(CODEX_LIST), Span::raw(" docs")]),
                _ => Line::from(vec![verb(CODEX_READ), Span::raw(format!(" {path}"))]),
            }
        }
    }
}

/// 折叠的思考行 / 推理摘要行（不含 glyph 栏）。
pub(super) fn thinking_line(deco: Deco, item_id: &str, turn: u16) -> Line<'static> {
    match deco {
        Deco::None | Deco::Man => Line::default(),
        Deco::Claude => Line::from(Span::styled(CLAUDE_THINKING, dim().add_modifier(Modifier::ITALIC))),
        Deco::Codex => Line::from(Span::styled(CODEX_REASONING[(seed(item_id, turn) % 3) as usize], dim().add_modifier(Modifier::ITALIC))),
        Deco::Grok => {
            let tenths = 8 + seed(item_id, turn) % 300;
            let muted = Style::new().fg(grok_gray_bright());
            Line::from(vec![Span::styled(GROK_THOUGHT, muted.add_modifier(Modifier::BOLD)), Span::styled(format!(" for {}", grok_duration(tenths)), muted)])
        }
    }
}

/// 一轮结束的统计行（不含 glyph 栏）。Codex 没有。
pub(super) fn turn_end_line(deco: Deco, item_id: &str, turn: u16) -> Line<'static> {
    let secs = turn_seconds(item_id, turn);
    match deco {
        Deco::None | Deco::Man | Deco::Codex => Line::default(),
        Deco::Claude => Line::from(Span::styled(format!("{} for {}", CLAUDE_PAST[(seed(item_id, turn) % 8) as usize], claude_duration(secs)), dim())),
        Deco::Grok => Line::from(Span::styled(format!("{GROK_WORKED} {}", grok_duration(secs * 10 + seed(item_id, turn) % 10)), Style::new().fg(grok_gray()))),
    }
}

/// 工作行的 spinner 帧。Claude 往返播放，Grok 循环。
pub(super) fn spinner_frame(deco: Deco, now_ms: u64) -> &'static str {
    match deco {
        Deco::Claude => {
            let n = CLAUDE_SPINNER.len();
            let i = (now_ms / 120) as usize % (n * 2);
            CLAUDE_SPINNER[if i < n { i } else { n * 2 - 1 - i }]
        }
        Deco::Grok => GROK_SPINNER[(now_ms / 130) as usize % GROK_SPINNER.len()],
        Deco::None | Deco::Man | Deco::Codex => CODEX_BULLET,
    }
}

/// 工作行正文（不含 spinner 栏）。
pub(super) fn work_line(deco: Deco, item_id: &str, elapsed_s: u64, tokens: usize) -> Line<'static> {
    match deco {
        Deco::None | Deco::Man => Line::default(),
        Deco::Claude => {
            let verb = CLAUDE_VERBS[(seed(item_id, 0) % CLAUDE_VERBS.len() as u64) as usize];
            Line::from(vec![Span::raw(format!("{verb}… ")), Span::styled(format!("({CLAUDE_INTERRUPT} · {} · ↓ {} tokens)", claude_duration(elapsed_s), claude_tokens(tokens)), dim())])
        }
        Deco::Codex => Line::from(vec![Span::styled(CODEX_WORKING, bold()), Span::styled(format!(" ({} • {CODEX_INTERRUPT})", codex_duration(elapsed_s)), dim())]),
        Deco::Grok => Line::from(Span::styled(GROK_RESPONDING, Style::new().fg(grok_text_secondary()))),
    }
}

/// Grok 工作行右侧：耗时、token、停止按钮。
pub(super) fn grok_work_right(elapsed_s: u64, tokens: usize) -> String {
    format!("{} {GROK_TOKENS_GLYPH}{} {GROK_STOP}", grok_duration(elapsed_s * 10), grok_tokens(tokens))
}

pub(super) fn claude_placeholder(index: usize) -> String {
    format!("{CLAUDE_TRY} \"summarize {}\"", fake_path(index))
}

pub(super) fn codex_context(percent: usize) -> String {
    format!("{}% context left", 100usize.saturating_sub(percent))
}

/// Codex 会话头方框的第一行：`>_ OpenAI Codex (v0.154.0)`。
pub(super) fn codex_banner_title() -> Line<'static> {
    Line::from(vec![Span::styled(format!("{CODEX_BANNER_MARK} "), dim()), Span::styled(CODEX_BANNER_NAME, bold()), Span::styled(format!(" {CODEX_VERSION}"), dim())])
}

/// 会话头里 `model:` / `directory:` 标签列的宽度（最长标签加一个空格），值从这一列之后起。
pub(super) const CODEX_LABEL_COL: u16 = 11;
pub(super) const CODEX_LABEL_MODEL: &str = "model:";
pub(super) const CODEX_LABEL_DIRECTORY: &str = "directory:";
/// model 值与 `/model to change` 之间的间隔列数。
pub(super) const CODEX_MODEL_GAP: u16 = 3;

pub(super) fn codex_tip() -> Line<'static> {
    Line::from(vec![Span::styled(CODEX_TIP_LABEL, bold()), Span::raw(CODEX_TIP_HEAD), Span::styled(CODEX_TIP_BOLD, bold()), Span::raw(CODEX_TIP_TAIL)])
}

pub(super) fn codex_model_hint() -> Line<'static> {
    Line::from(vec![Span::styled(CODEX_MODEL_HINT, Style::new().fg(Color::Cyan)), Span::styled(CODEX_MODEL_HINT_TAIL, dim())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_hash_is_40_hex_and_stable() {
        let h = fake_hash("7143038691944915470");
        assert_eq!(h.len(), 40);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(h, fake_hash("7143038691944915470"));
        assert_ne!(fake_hash("a"), fake_hash("b"));
    }

    #[test]
    fn fake_path_pads_index() {
        assert_eq!(fake_path(11), "docs/ch-0012.md");
        assert_eq!(fake_path(0), "docs/ch-0001.md");
        assert_eq!(fake_man_name(11), "ch-0012(7)");
    }

    #[test]
    fn diff_header_matches_git_new_file() {
        let h = diff_header(11, "x", 214);
        assert_eq!(h[0], "diff --git a/docs/ch-0012.md b/docs/ch-0012.md");
        assert_eq!(h[1], "new file mode 100644");
        assert!(h[2].starts_with("index 0000000..") && h[2].len() == "index 0000000..".len() + 7);
        assert_eq!(h[3], "--- /dev/null");
        assert_eq!(h[4], "+++ b/docs/ch-0012.md");
        assert_eq!(h[5], "@@ -0,0 +1,214 @@");
        assert_eq!(diff_header(0, "x", 1)[5], "@@ -0,0 +1 @@");
    }

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19723), (2024, 1, 1));
        assert_eq!(civil_from_days(20712), (2026, 9, 16));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn fake_date_format_and_weekday() {
        // 2026-09-16（周三）01:00 UTC，item_id 固定，hash % 365 = 64 天，往前落在 2026-07-14 周二。
        let now_ms = 20712 * 86_400_000 + 3_600_000;
        let s = fake_date("7143038691944915470", now_ms);
        assert!(regex_lite(&s), "unexpected date format: {s}");
        assert!(s.starts_with("Tue Jul 14 "), "{s}");
        assert!(s.ends_with(" 2026 +0800"), "{s}");
        assert_eq!(s, fake_date("7143038691944915470", now_ms + 60_000));
        assert_eq!(man_date("7143038691944915470", now_ms), "July 14, 2026");
        let clock = fake_clock("7143038691944915470", now_ms, 0);
        assert!(clock.ends_with(" AM") || clock.ends_with(" PM"), "{clock}");
        assert_ne!(clock, fake_clock("7143038691944915470", now_ms, 1));
    }

    /// 手写匹配 `^[A-Z][a-z]{2} [A-Z][a-z]{2} \d{1,2} \d\d:\d\d:\d\d \d{4} \+0800$`，不引入 regex 依赖。
    fn regex_lite(s: &str) -> bool {
        let parts: Vec<&str> = s.split(' ').collect();
        if parts.len() != 6 {
            return false;
        }
        let word = |w: &str| w.len() == 3 && w.as_bytes()[0].is_ascii_uppercase() && w.as_bytes()[1..].iter().all(u8::is_ascii_lowercase);
        let digits = |w: &str| !w.is_empty() && w.bytes().all(|b| b.is_ascii_digit());
        let clock = parts[3].as_bytes();
        let clock_ok = clock.len() == 8 && clock[2] == b':' && clock[5] == b':' && [0, 1, 3, 4, 6, 7].iter().all(|&i| clock[i].is_ascii_digit());
        word(parts[0]) && word(parts[1]) && digits(parts[2]) && parts[2].len() <= 2 && clock_ok && digits(parts[4]) && parts[4].len() == 4 && parts[5] == "+0800"
    }

    #[test]
    fn durations_follow_each_tool() {
        assert_eq!(claude_duration(36), "36s");
        assert_eq!(claude_duration(121), "2m 1s");
        assert_eq!(claude_duration(180), "3m");
        assert_eq!(codex_duration(3), "3s");
        assert_eq!(codex_duration(60), "1m 00s");
        assert_eq!(grok_duration(42), "4.2s");
        assert_eq!(grok_duration(150), "15s");
        assert_eq!(grok_duration(800), "1m20s");
        assert_eq!(claude_tokens(892), "892");
        assert_eq!(claude_tokens(3210), "3.2k");
        assert_eq!(grok_tokens(1234), "1.23k");
        assert_eq!(grok_tokens(12_340), "12.3k");
        assert_eq!(grok_tokens(123_400), "123k");
        assert_eq!(grok_header_tokens(1500), "1.5K");
        assert_eq!(grok_header_tokens(12_000), "12K");
    }

    #[test]
    fn tool_lines_read_like_each_cli() {
        assert_eq!(tool_call(Deco::Claude, 11, 0, 214).to_string(), "Read(docs/ch-0012.md)");
        assert_eq!(tool_result(Deco::Claude, 11, 0, 214).to_string(), "Read 214 lines");
        assert_eq!(tool_call(Deco::Codex, 11, 0, 214).to_string(), "Explored");
        assert_eq!(tool_result(Deco::Codex, 11, 0, 214).to_string(), "Read docs/ch-0012.md");
        assert_eq!(tool_call(Deco::Grok, 11, 0, 214).to_string(), "Read docs/ch-0012.md (1-214)");
        assert_eq!(tool_result(Deco::Grok, 11, 0, 214).to_string(), "");
        // 轮换：index 11 + turn 2 = 13 -> variant 1。
        assert_eq!(tool_call(Deco::Claude, 11, 2, 9).to_string(), "Bash(wc -l docs/ch-0012.md)");
        assert_eq!(tool_result(Deco::Claude, 11, 2, 9).to_string(), "9 docs/ch-0012.md");
        assert_eq!(tool_call(Deco::Codex, 11, 2, 9).to_string(), "Ran wc -l docs/ch-0012.md");
        assert_eq!(thinking_line(Deco::Claude, "x", 1).to_string(), "Thinking…");
        assert!(thinking_line(Deco::Grok, "x", 1).to_string().starts_with("Thought for "));
        assert!(turn_end_line(Deco::Claude, "x", 0).to_string().ends_with('s'));
        assert!(turn_end_line(Deco::Grok, "x", 0).to_string().starts_with("Worked for "));
        assert_eq!(turn_end_line(Deco::Codex, "x", 0).to_string(), "");
        assert_eq!(turn_text(11, 0), "看看 docs/ch-0012.md");
        assert_eq!(turn_text(11, 3), "继续");
    }

    #[test]
    fn work_lines_and_spinners() {
        assert!(work_line(Deco::Claude, "x", 12, 1200).to_string().ends_with("… (esc to interrupt · 12s · ↓ 1.2k tokens)"));
        assert_eq!(work_line(Deco::Codex, "x", 3, 0).to_string(), "Working (3s • esc to interrupt)");
        assert_eq!(work_line(Deco::Grok, "x", 3, 0).to_string(), "Responding…");
        assert_eq!(grok_work_right(80, 12_340), "1m20s ⇣12.3k [stop]");
        assert_eq!(spinner_frame(Deco::Claude, 0), "·");
        assert_eq!(spinner_frame(Deco::Claude, 120 * 5), "✽");
        assert_eq!(spinner_frame(Deco::Claude, 120 * 6), "✽");
        assert_eq!(spinner_frame(Deco::Claude, 120 * 11), "·");
        assert_eq!(spinner_frame(Deco::Grok, 130 * 8), "⠋");
    }

    #[test]
    fn titles_and_names_cover_every_skin() {
        assert_eq!(cover_title(LayoutId::Native), "tomato");
        assert_eq!(cover_title(LayoutId::Grok), "grok");
        assert_eq!(cover_window_title(LayoutId::Man, Some(11)), "man ch-0012(7)");
        assert_eq!(cover_window_title(LayoutId::Man, None), "man");
        assert_eq!(cover_window_title(LayoutId::Claude, Some(3)), CLAUDE_TITLE);
        for id in LayoutId::ALL {
            assert!(!cover_title(id).is_empty());
            assert!(!id.name().is_empty());
        }
    }
}
