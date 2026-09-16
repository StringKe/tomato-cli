//! 伪装布局：整帧画成别的 CLI 的样子。正文仍走 WrapCache 预折行，agent 皮肤按每行的 LineKind 把它画成多轮对话（用户输入、
//! 工具调用、结果、思考、回合统计），pager 皮肤画成 diff / 手册页。文案、字形、颜色、行数在 skin.rs。

mod skin;

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, ListItem, ListState, Paragraph};

use super::layout::{dim, draw_hints, hl, width_of};
use super::workspace::reader_window;
use crate::app::{App, Hint, Overlay, Screen};
use crate::model::LayoutId;
use crate::reader::{Deco, LineKind, clamp_offset};
use crate::theme::{Palette, ThemeId, palette};
use skin::*;

pub(crate) use skin::{cover_window_title, reflow_note};

/// 内联目录列表最多占的行数。
const PICKER_MAX: usize = 8;
/// agent 皮肤状态行和目录列表的左缩进。
const STATUS_INDENT: u16 = 2;
/// 状态行左右两段之间的间隔。
const STATUS_GAP: u16 = 2;
/// Claude 状态行里显示「距离压缩还剩」的阅读进度阈值：真实 CLI 只在上下文快满时才显示这一段。
const CLAUDE_COMPACT_FROM: usize = 60;

/// 一帧伪装所需的全部状态，从 App 里抽出来，绘制函数不再碰 App。
struct CoverCtx<'a> {
    /// 章节序号（0 起）。
    pub index: usize,
    pub item_id: &'a str,
    /// cache.len()。
    pub lines_total: usize,
    /// read_percent(end, lines_total)。
    pub percent: usize,
    pub at_end: bool,
    /// 空闲会话：非阅读屏幕按老板键。
    pub idle: bool,
    /// busy 或正文还没到手，画工作行。
    pub working: bool,
    /// 底栏退化成 compact_bottom 行。
    pub compact: bool,
    /// 提示符后要显示的输入：Jump 时是 jump_buf，Toc 时是 toc_filter，其余 None。
    pub input: Option<&'a str>,
    pub picker_open: bool,
    pub picker_rows: u16,
    pub hints: &'static [Hint],
    /// 一次性英文提示（如切换整理模式），有值时顶掉状态行右侧的进度文本 / pager 的提示符。
    pub note: Option<&'a str>,
    /// 工作行的耗时（秒）和已读正文换算的 token 数。
    pub elapsed_s: u64,
    pub tokens: usize,
    pub now_ms: u64,
}

/// 按高度和是否在章首决定实际顶底栏行数。返回 (top, bottom)。
fn chrome_rows(skin: &Skin, height: u16, at_top: bool, busy: bool) -> (u16, u16) {
    if skin.id == LayoutId::Native {
        return (0, 0);
    }
    let work = if busy { skin.work_rows } else { 0 };
    let mut top = if at_top || skin.top_fixed { skin.top_rows } else { 0 };
    let mut bottom = skin.bottom_rows + work;
    if height < top + bottom + 6 {
        top = 0;
    }
    if height < bottom + 4 {
        bottom = skin.compact_bottom + work;
    }
    if height < 3 {
        top = 0;
        bottom = 0;
    }
    (top, bottom)
}

/// 正文可用宽度：窗格宽减左缩进和右留白，再受 text_width 上限，最小 4。
fn content_width(skin: &Skin, pane_w: u16, text_width: u16) -> u16 {
    let mut w = pane_w.saturating_sub(skin.indent + skin.right_pad);
    if text_width > 0 {
        w = w.min(text_width);
    }
    w.max(4)
}

/// 区域里的第 i 行；越界返回零高 Rect，渲染时自动成为空操作。
fn row(area: Rect, i: u16) -> Rect {
    if i >= area.height {
        return Rect { x: area.x, y: area.y.saturating_add(area.height), width: area.width, height: 0 };
    }
    Rect { x: area.x, y: area.y + i, width: area.width, height: 1 }
}

/// 从行首起切 n 列，返回 (前 n 列, 余下)。
fn split_cols(area: Rect, n: u16) -> (Rect, Rect) {
    let [a, b] = Layout::horizontal([Constraint::Length(n), Constraint::Fill(1)]).areas(area);
    (a, b)
}

/// 右侧留 pad 列后，宽 w 的靠右区域。
fn right_cell(area: Rect, w: u16, pad: u16) -> Rect {
    let [_, cell, _] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(w), Constraint::Length(pad)]).areas(area);
    cell
}

/// glyph 单独画在固定列宽的 Rect 里，正文列起点由缩进固定，Ambiguous 宽度终端画 2 列时吃掉的是空格。
fn draw_glyph(frame: &mut ratatui::Frame, area: Rect, glyph: &str, style: Style) -> Rect {
    let (g, rest) = split_cols(area, 2);
    frame.render_widget(Paragraph::new(glyph).style(style), g);
    rest
}

/// 光标放在 text 之后，不超出区域右边界。
fn cursor_after(area: Rect, text: &str) -> Option<(u16, u16)> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let x = area.x.saturating_add(width_of(text).min(area.width.saturating_sub(1)));
    Some((x, area.y))
}

fn dim_style() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

fn fill_bg(frame: &mut ratatui::Frame, area: Rect, bg: Option<Color>) {
    if let Some(bg) = bg {
        frame.render_widget(Block::new().style(Style::new().bg(bg)), area);
    }
}

/// 伪装态整帧入口。配色固定 Auto：不涂背景，正文不设前景色，揭开后用户主题原样生效。
pub(super) fn draw_cover(app: &mut App, frame: &mut ratatui::Frame) {
    let p = palette(ThemeId::Auto);
    let skin = skin(app.state.settings.layout);
    let area = frame.area();
    let now_ms = crate::store::now_ms();
    let elapsed_s = app.cover_work_since.map(|t| t.elapsed().as_secs()).unwrap_or(0);
    if app.screen() != Screen::Reader || app.reader.is_none() {
        draw_idle(app, frame, &skin, area, now_ms, p);
        return;
    }
    let hints = app.hints();
    let overlay = app.overlay;
    let input = match overlay {
        Overlay::Jump => Some(app.jump_buf.clone()),
        Overlay::Toc => Some(app.toc_filter.clone()),
        _ => None,
    };
    let picker_open = overlay == Overlay::Toc;
    let picker_items: Vec<String> = if picker_open { app.toc_items().into_iter().map(|(i, _)| fake_path(i)).collect() } else { Vec::new() };
    // 伪装态的段落样式由皮肤决定：被模仿的工具不缩进、段间空行，agent 皮肤还要插假对话。用户的缩进和空行设置只管原生阅读页。
    let opts = crate::reader::WrapOpts { para_indent: false, para_blank: true, deco: deco_of(skin.id), ..app.wrap_opts() };
    let text_width = app.state.settings.text_width;
    let busy = app.busy;
    let note = app.cover_note.as_ref().map(|(s, _)| s.clone());

    let Some(session) = app.reader.as_mut() else { return };
    let loading = session.body.item_id.is_empty();
    let working = busy || loading;
    // 窗格宽等于整帧宽，先按最终宽度折行并结算待恢复的正文位置，下面的 offset == 0 才是本帧真实的章首。
    session.prepare_wrap(content_width(&skin, area.width, text_width), opts);
    // 再按「章首」的最小窗格夹一次偏移：短章也能滚到最后几行。
    let (top_full, bottom_full) = chrome_rows(&skin, area.height, true, working);
    let picker_rows = (picker_items.len().min(PICKER_MAX) as u16).min(area.height.saturating_sub(top_full + bottom_full + 2));
    let pane_min = area.height.saturating_sub(top_full + bottom_full + picker_rows).max(1) as usize;
    session.offset = clamp_offset(session.offset, session.cache.len(), pane_min);
    let at_top = session.offset == 0;
    let (top, bottom) = chrome_rows(&skin, area.height, at_top, working);
    let compact = bottom < skin.bottom_rows + if working { skin.work_rows } else { 0 };
    let [head, pane, foot] = Layout::vertical([Constraint::Length(top), Constraint::Fill(1), Constraint::Length(bottom + picker_rows)]).areas(area);
    let text_w = content_width(&skin, pane.width, text_width);
    let text_area = Rect { x: pane.x.saturating_add(skin.indent), y: pane.y, width: text_w, height: pane.height }.intersection(pane);
    let (start, end, percent) = reader_window(session, text_area, opts);
    // start 只用来切可见行；工作行的 token 数按窗口末尾的正文位置算。
    let rows: Vec<(String, LineKind)> = session.cache.lines()[start..end].iter().cloned().zip(session.cache.kinds()[start..end].iter().copied()).collect();
    let index = session.index;
    let item_id = session.body.item_id.clone();
    let lines_total = session.cache.len();
    let at_end = session.at_end();
    let tokens = tokens_for(session.cache.anchor_of(end.saturating_sub(1)).unwrap_or(0));

    let ctx = CoverCtx { index, item_id: &item_id, lines_total, percent, at_end, idle: false, working, compact, input: input.as_deref(), picker_open, picker_rows, hints, note: note.as_deref(), elapsed_s, tokens, now_ms };
    if !loading {
        draw_body(frame, &skin, pane, text_area, &rows, &ctx);
    }
    if top > 0 {
        draw_top(frame, &skin, head, &ctx);
    }
    let (picker_area, cursor) = draw_bottom(frame, &skin, foot, &ctx, p);
    match picker_area {
        Some(list) if picker_open => {
            draw_picker(frame, &skin, list, picker_items, &mut app.toc_list, p);
            app.list_area = list;
            app.overlay_area = list;
        }
        _ => app.list_area = pane,
    }
    if let Some((x, y)) = cursor {
        frame.set_cursor_position((x, y));
    }
}

/// 空闲会话：非阅读屏幕按老板键时画成刚打开还没输入的 CLI。Codex 的会话头和输入框贴在顶部，Grok 有固定 header，其余只有底栏。
fn draw_idle(app: &mut App, frame: &mut ratatui::Frame, skin: &Skin, area: Rect, now_ms: u64, p: Palette) {
    let (_, bottom) = chrome_rows(skin, area.height, false, false);
    let compact = bottom < skin.bottom_rows;
    let ctx = CoverCtx { index: 0, item_id: "", lines_total: 0, percent: 0, at_end: false, idle: true, working: false, compact, input: None, picker_open: false, picker_rows: 0, hints: app.hints(), note: None, elapsed_s: 0, tokens: 0, now_ms };
    let cursor = match skin.id {
        LayoutId::Codex => {
            let top = chrome_rows(skin, area.height, true, false).0;
            let [head, foot, _] = Layout::vertical([Constraint::Length(top), Constraint::Length(bottom), Constraint::Fill(1)]).areas(area);
            if top > 0 {
                draw_top(frame, skin, head, &ctx);
            }
            draw_bottom(frame, skin, foot, &ctx, p).1
        }
        LayoutId::Grok => {
            let top = chrome_rows(skin, area.height, true, false).0;
            let [head, _, foot] = Layout::vertical([Constraint::Length(top), Constraint::Fill(1), Constraint::Length(bottom)]).areas(area);
            if top > 0 {
                draw_top(frame, skin, head, &ctx);
            }
            draw_bottom(frame, skin, foot, &ctx, p).1
        }
        _ => {
            let [_, foot] = Layout::vertical([Constraint::Fill(1), Constraint::Length(bottom)]).areas(area);
            draw_bottom(frame, skin, foot, &ctx, p).1
        }
    };
    app.list_area = Rect::default();
    if let Some((x, y)) = cursor {
        frame.set_cursor_position((x, y));
    }
}

/// 章节标题在各 agent 皮肤里的 markdown 一级标题样式。
fn heading_style(id: LayoutId) -> Style {
    match id {
        LayoutId::Claude => bold().add_modifier(Modifier::ITALIC | Modifier::UNDERLINED),
        LayoutId::Codex => bold().add_modifier(Modifier::UNDERLINED),
        LayoutId::Grok => bold().fg(grok_teal()),
        LayoutId::Man => bold(),
        LayoutId::Native | LayoutId::GitLog => Style::new(),
    }
}

/// 正文逐行画。左侧缩进列是 glyph 栏，正文从 text_area 起。agent 皮肤按行身份画用户输入、工具调用、结果、思考和回合统计；
/// diff 皮肤每行前加 `+` 并整行绿色；man 皮肤到底后补页脚。
fn draw_body(frame: &mut ratatui::Frame, skin: &Skin, pane: Rect, text_area: Rect, rows: &[(String, LineKind)], ctx: &CoverCtx) {
    for (i, (text, kind)) in rows.iter().enumerate() {
        let y = text_area.y + i as u16;
        if y >= text_area.y + text_area.height {
            break;
        }
        let full = Rect { x: pane.x, y, width: pane.width, height: 1 };
        let gutter = Rect { x: pane.x, y, width: skin.indent, height: 1 }.intersection(pane);
        let line = Rect { x: text_area.x, y, width: text_area.width, height: 1 };
        match skin.id {
            LayoutId::GitLog => {
                frame.render_widget(Paragraph::new(DIFF_PLUS).style(Style::new().fg(DIFF_ADD_COLOR)), gutter);
                frame.render_widget(Paragraph::new(text.as_str()).style(Style::new().fg(DIFF_ADD_COLOR)), line);
            }
            LayoutId::Man if *kind == LineKind::Footer => draw_man_footer(frame, full, ctx),
            LayoutId::Man => {
                let style = if matches!(kind, LineKind::Heading | LineKind::ReplyHeading) { heading_style(skin.id) } else { Style::new() };
                frame.render_widget(Paragraph::new(text.as_str()).style(style), line);
            }
            LayoutId::Claude => draw_claude_row(frame, full, gutter, line, text, *kind, ctx),
            LayoutId::Codex => draw_codex_row(frame, gutter, line, text, *kind, ctx),
            LayoutId::Grok => draw_grok_row(frame, pane, line, text, *kind, ctx),
            LayoutId::Native => frame.render_widget(Paragraph::new(text.as_str()), line),
        }
    }
}

fn draw_claude_row(frame: &mut ratatui::Frame, full: Rect, gutter: Rect, line: Rect, text: &str, kind: LineKind, ctx: &CoverCtx) {
    let deco = Deco::Claude;
    match kind {
        LineKind::Text => frame.render_widget(Paragraph::new(text), line),
        LineKind::Blank | LineKind::Band(_) | LineKind::Footer => {}
        LineKind::Heading => frame.render_widget(Paragraph::new(text).style(heading_style(LayoutId::Claude)), line),
        LineKind::ReplyStart | LineKind::ReplyHeading => {
            frame.render_widget(Paragraph::new(CLAUDE_GLYPH), gutter);
            let style = if kind == LineKind::ReplyHeading { heading_style(LayoutId::Claude) } else { Style::new() };
            frame.render_widget(Paragraph::new(text).style(style), line);
        }
        LineKind::Turn(t) => {
            // 用户消息整行铺底色，右侧留 1 列。
            let band = Rect { width: full.width.saturating_sub(1), ..full };
            fill_bg(frame, band, claude_user_bg());
            frame.render_widget(Paragraph::new(CLAUDE_PROMPT).style(Style::new().fg(claude_subtle())), gutter);
            frame.render_widget(Paragraph::new(turn_text(ctx.index, t)), line);
        }
        LineKind::ToolCall(t) => {
            frame.render_widget(Paragraph::new(CLAUDE_GLYPH).style(Style::new().fg(claude_success())), gutter);
            frame.render_widget(Paragraph::new(tool_call(deco, ctx.index, t, ctx.lines_total)), line);
        }
        LineKind::ToolResult(t) => {
            let mut spans = vec![Span::styled(format!("  {CLAUDE_BRANCH} \u{a0}"), dim_style())];
            spans.extend(tool_result(deco, ctx.index, t, ctx.lines_total).spans);
            frame.render_widget(Paragraph::new(Line::from(spans)), full);
        }
        LineKind::Thinking(t) => {
            frame.render_widget(Paragraph::new(CLAUDE_STAR).style(dim_style()), gutter);
            frame.render_widget(Paragraph::new(thinking_line(deco, ctx.item_id, t)), line);
        }
        LineKind::TurnEnd(t) => {
            frame.render_widget(Paragraph::new(CLAUDE_STAR).style(dim_style()), gutter);
            frame.render_widget(Paragraph::new(turn_end_line(deco, ctx.item_id, t)), line);
        }
    }
}

fn draw_codex_row(frame: &mut ratatui::Frame, gutter: Rect, line: Rect, text: &str, kind: LineKind, ctx: &CoverCtx) {
    let deco = Deco::Codex;
    let full = Rect { x: gutter.x, width: gutter.width + line.width, ..line };
    match kind {
        LineKind::Text => frame.render_widget(Paragraph::new(text), line),
        LineKind::Blank | LineKind::Band(_) | LineKind::TurnEnd(_) | LineKind::Footer => {}
        LineKind::Heading => frame.render_widget(Paragraph::new(Line::from(vec![Span::raw("# "), Span::styled(text.to_string(), heading_style(LayoutId::Codex))])), line),
        LineKind::ReplyStart | LineKind::ReplyHeading => {
            frame.render_widget(Paragraph::new(CODEX_BULLET).style(dim_style()), gutter);
            if kind == LineKind::ReplyHeading {
                frame.render_widget(Paragraph::new(Line::from(vec![Span::raw("# "), Span::styled(text.to_string(), heading_style(LayoutId::Codex))])), line);
            } else {
                frame.render_widget(Paragraph::new(text), line);
            }
        }
        LineKind::Turn(t) => {
            frame.render_widget(Paragraph::new(CODEX_PROMPT).style(bold().add_modifier(Modifier::DIM)), gutter);
            frame.render_widget(Paragraph::new(turn_text(ctx.index, t)), line);
        }
        LineKind::ToolCall(t) => {
            // Ran 的点是绿色粗体（命令成功），Explored 的点 dim。
            let ran = tool_call(deco, ctx.index, t, ctx.lines_total).to_string().starts_with(CODEX_RAN);
            let style = if ran { bold().fg(Color::Green) } else { dim_style() };
            frame.render_widget(Paragraph::new(CODEX_BULLET).style(style), gutter);
            frame.render_widget(Paragraph::new(tool_call(deco, ctx.index, t, ctx.lines_total)), line);
        }
        LineKind::ToolResult(t) => {
            let mut spans = vec![Span::styled(format!("  {CODEX_BRANCH} "), dim_style())];
            spans.extend(tool_result(deco, ctx.index, t, ctx.lines_total).spans);
            frame.render_widget(Paragraph::new(Line::from(spans)), full);
        }
        LineKind::Thinking(t) => {
            frame.render_widget(Paragraph::new(CODEX_BULLET).style(dim_style()), gutter);
            frame.render_widget(Paragraph::new(thinking_line(deco, ctx.item_id, t)), line);
        }
    }
}

fn draw_grok_row(frame: &mut ratatui::Frame, pane: Rect, line: Rect, text: &str, kind: LineKind, ctx: &CoverCtx) {
    let deco = Deco::Grok;
    // 用户输入带从外边距之后铺到右外边距之前。
    let band = Rect { x: pane.x + GROK_BOX_PAD, y: line.y, width: pane.width.saturating_sub(GROK_BOX_PAD * 2), height: 1 }.intersection(pane);
    let bullet = |s: &'static str, style: Style| Span::styled(format!("{s} "), style);
    match kind {
        LineKind::Text | LineKind::ReplyStart => frame.render_widget(Paragraph::new(text), line),
        LineKind::Blank | LineKind::Footer => {}
        LineKind::Heading | LineKind::ReplyHeading => frame.render_widget(Paragraph::new(text).style(heading_style(LayoutId::Grok)), line),
        LineKind::Band(_) => fill_bg(frame, band, grok_band()),
        LineKind::Turn(t) => {
            let bg = grok_band();
            fill_bg(frame, band, bg);
            let text_style = if bg.is_some() { Style::new() } else { bold() };
            frame.render_widget(Paragraph::new(Line::from(vec![bullet(GROK_PROMPT, Style::new().fg(grok_text_secondary())), Span::styled(turn_text(ctx.index, t), text_style)])), line);
            let clock = fake_clock(ctx.item_id, ctx.now_ms, t);
            let cell = right_cell(line, width_of(&clock), 0);
            if cell.width >= width_of(&clock) && line.width > width_of(&clock) + 12 {
                frame.render_widget(Paragraph::new(clock).style(Style::new().fg(grok_gray())), cell);
            }
        }
        LineKind::ToolCall(t) => {
            let mut spans = vec![bullet(GROK_BULLET, Style::new().fg(grok_gray()))];
            spans.extend(tool_call(deco, ctx.index, t, ctx.lines_total).spans);
            frame.render_widget(Paragraph::new(Line::from(spans)), line);
        }
        LineKind::ToolResult(_) => {}
        LineKind::Thinking(t) => {
            let mut spans = vec![bullet(GROK_BULLET, Style::new().fg(grok_gray()))];
            spans.extend(thinking_line(deco, ctx.item_id, t).spans);
            frame.render_widget(Paragraph::new(Line::from(spans)), line);
        }
        LineKind::TurnEnd(t) => frame.render_widget(Paragraph::new(turn_end_line(deco, ctx.item_id, t)), line),
    }
}

/// man 页末尾的页脚行：左 OS 名、中日期、右手册名，宽度为终端宽减 2。
fn draw_man_footer(frame: &mut ratatui::Frame, full: Rect, ctx: &CoverCtx) {
    let line = Rect { width: full.width.saturating_sub(2), ..full };
    let name = fake_man_name(ctx.index).to_uppercase();
    let date = man_date(ctx.item_id, ctx.now_ms);
    let os = format!("\u{f8ff} {MAN_OS}");
    man_three_columns(frame, line, &os, &date, &name);
}

/// man 的三段标题 / 页脚：左段贴左，右段贴右，中段居中；放不下时中段整条丢弃。
fn man_three_columns(frame: &mut ratatui::Frame, line: Rect, left: &str, center: &str, right: &str) {
    let (lw, cw, rw) = (width_of(left), width_of(center), width_of(right));
    if line.width >= lw + cw + rw + 2 {
        let [l, c, r] = Layout::horizontal([Constraint::Length(lw), Constraint::Length(cw), Constraint::Length(rw)]).flex(Flex::SpaceBetween).areas(line);
        frame.render_widget(Paragraph::new(left), l);
        frame.render_widget(Paragraph::new(center), c);
        frame.render_widget(Paragraph::new(right), r);
    } else {
        let [l, r] = Layout::horizontal([Constraint::Length(lw), Constraint::Length(rw)]).flex(Flex::SpaceBetween).areas(line);
        frame.render_widget(Paragraph::new(left), l);
        frame.render_widget(Paragraph::new(right), r);
    }
}

/// 顶栏：Codex 的会话头方框、Grok 的固定 header、git log 的 commit 头加 diff 头、man 的标题和 NAME 节。Claude 没有顶栏。
fn draw_top(frame: &mut ratatui::Frame, skin: &Skin, head: Rect, ctx: &CoverCtx) {
    match skin.id {
        LayoutId::Codex => {
            let inner_w = head.width.saturating_sub(4).min(CODEX_BANNER_INNER);
            let boxed = Rect { x: head.x, y: head.y, width: inner_w + 4, height: 6.min(head.height) }.intersection(head);
            let block = Block::bordered().border_type(BorderType::Rounded).border_style(dim_style());
            let inner = block.inner(boxed);
            frame.render_widget(block, boxed);
            let (_, inner) = split_cols(inner, 1);
            frame.render_widget(Paragraph::new(codex_banner_title()), row(inner, 0));
            let (label, value) = split_cols(row(inner, 2), CODEX_LABEL_COL);
            frame.render_widget(Paragraph::new(CODEX_LABEL_MODEL).style(dim_style()), label);
            let hint = codex_model_hint();
            let [model_a, hint_a] = Layout::horizontal([Constraint::Length(width_of(CODEX_MODEL)), Constraint::Length(hint.width() as u16)]).spacing(CODEX_MODEL_GAP).areas(value);
            frame.render_widget(Paragraph::new(CODEX_MODEL), model_a);
            frame.render_widget(Paragraph::new(hint), hint_a);
            let (label, value) = split_cols(row(inner, 3), CODEX_LABEL_COL);
            frame.render_widget(Paragraph::new(CODEX_LABEL_DIRECTORY).style(dim_style()), label);
            frame.render_widget(Paragraph::new(CODEX_DIRECTORY), value);
            let (_, tip) = split_cols(row(head, 7), STATUS_INDENT);
            frame.render_widget(Paragraph::new(codex_tip()), tip);
        }
        LayoutId::Grok => {
            let line = row(head, 1);
            let (_, left) = split_cols(line, GROK_BOX_PAD);
            frame.render_widget(Paragraph::new(GROK_CWD).style(Style::new().fg(grok_gray_bright())), left);
            if !ctx.idle {
                let right = Line::from(vec![Span::styled(format!("{} / {GROK_CONTEXT}", grok_header_tokens(ctx.tokens)), Style::new().fg(grok_text())), Span::styled(" │ ", Style::new().fg(grok_gray_dim())), Span::styled(GROK_DASHBOARD, Style::new().fg(grok_gray()))]);
                let w = right.width() as u16;
                if line.width > w + GROK_BOX_PAD + width_of(GROK_CWD) + 4 {
                    frame.render_widget(Paragraph::new(right), right_cell(line, w, GROK_BOX_PAD));
                }
            }
        }
        LayoutId::GitLog => {
            let mut commit = vec![Span::styled(format!("commit {} ", fake_hash(ctx.item_id)), Style::new().fg(GITLOG_HEAD_COLOR))];
            commit.extend(gitlog_decoration());
            frame.render_widget(Paragraph::new(Line::from(commit)), row(head, 0));
            frame.render_widget(Paragraph::new(GITLOG_AUTHOR), row(head, 1));
            let (label, value) = split_cols(row(head, 2), GITLOG_VALUE_COL);
            frame.render_widget(Paragraph::new(GITLOG_DATE_LABEL), label);
            frame.render_widget(Paragraph::new(fake_date(ctx.item_id, ctx.now_ms)), value);
            let (_, subject) = split_cols(row(head, 4), GITLOG_SUBJECT_COL);
            frame.render_widget(Paragraph::new(gitlog_subject(ctx.index)), subject);
            let header = diff_header(ctx.index, ctx.item_id, ctx.lines_total);
            for (i, text) in header.iter().enumerate() {
                let style = if i == 5 { Style::new().fg(DIFF_HUNK_COLOR) } else { bold() };
                frame.render_widget(Paragraph::new(text.as_str()).style(style), row(head, 6 + i as u16));
            }
        }
        LayoutId::Man => {
            let name = fake_man_name(ctx.index).to_uppercase();
            let title = Rect { width: head.width.saturating_sub(2), ..row(head, 0) };
            man_three_columns(frame, title, &name, MAN_CENTER, &name);
            frame.render_widget(Paragraph::new(MAN_NAME).style(bold()), row(head, 2));
            let (_, name_line) = split_cols(row(head, 3), MAN_INDENT);
            frame.render_widget(Paragraph::new(Line::from(vec![Span::styled(format!("ch-{:04}", ctx.index + 1), bold()), Span::raw(format!(" {MAN_DASH} chapter {}", ctx.index + 1))])), name_line);
            frame.render_widget(Paragraph::new(MAN_SECTION).style(bold()), row(head, 5));
        }
        LayoutId::Claude | LayoutId::Native => {}
    }
}

/// 底栏：输入框 / 提示符 / 状态行，目录列表的位置也在这里定。返回 (目录列表区, 光标位置)。
fn draw_bottom(frame: &mut ratatui::Frame, skin: &Skin, foot: Rect, ctx: &CoverCtx, p: Palette) -> (Option<Rect>, Option<(u16, u16)>) {
    if foot.height == 0 {
        return (None, None);
    }
    let work = if ctx.working { skin.work_rows } else { 0 };
    let picker = Constraint::Length(ctx.picker_rows);
    let mut picker_area = None;
    let cursor;
    match skin.id {
        LayoutId::Claude => {
            let [work_a, top_a, line, bottom_a, picker_a, status_a] = if ctx.compact {
                Layout::vertical([Constraint::Length(work), Constraint::Length(0), Constraint::Length(1), Constraint::Length(0), picker, Constraint::Length(0)]).areas(foot)
            } else {
                Layout::vertical([Constraint::Length(work), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), picker, Constraint::Length(1)]).areas(foot)
            };
            draw_work(frame, skin, work_a, ctx);
            let rule = "─".repeat(foot.width as usize);
            let rule_style = Style::new().fg(claude_prompt_border());
            frame.render_widget(Paragraph::new(rule.as_str()).style(rule_style), top_a);
            frame.render_widget(Paragraph::new(rule.as_str()).style(rule_style), bottom_a);
            let placeholder = claude_placeholder(ctx.index);
            cursor = draw_prompt(frame, line, CLAUDE_PROMPT, dim_style(), ctx, (!ctx.idle).then_some(placeholder.as_str()), p);
            picker_area = Some(picker_a);
            let context = (!ctx.idle && ctx.percent >= CLAUDE_COMPACT_FROM).then(|| claude_context(ctx.percent));
            let mode = Span::styled(CLAUDE_MODE, Style::new().fg(claude_auto_accept()));
            let full = Line::from(vec![mode.clone(), Span::styled(format!(" {CLAUDE_MODE_HINT} · "), dim_style())]);
            let short = Line::from(vec![mode, Span::styled(" · ", dim_style())]);
            let right = ctx.note.map(|n| Line::from(Span::styled(n.to_string(), dim(p)))).or(context);
            draw_status(frame, status_a, ctx, &[full, short], right, p);
        }
        LayoutId::Codex => {
            let [work_a, _, line, _, picker_a, status_a] = if ctx.compact {
                Layout::vertical([Constraint::Length(work), Constraint::Length(0), Constraint::Length(1), Constraint::Length(0), picker, Constraint::Length(1)]).areas(foot)
            } else {
                Layout::vertical([Constraint::Length(work), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), picker, Constraint::Length(1)]).areas(foot)
            };
            draw_work(frame, skin, work_a, ctx);
            cursor = draw_prompt(frame, line, CODEX_PROMPT, bold(), ctx, Some(CODEX_PLACEHOLDER), p);
            picker_area = Some(picker_a);
            let right = ctx.note.map(str::to_string).or_else(|| (!ctx.idle).then(|| codex_context(ctx.percent))).map(|s| Line::from(Span::styled(s, dim(p))));
            draw_status(frame, status_a, ctx, &[], right, p);
        }
        LayoutId::Grok => {
            let [work_a, picker_a, box_a, _, bar_a, _] = if ctx.compact {
                Layout::vertical([Constraint::Length(work), picker, Constraint::Length(3), Constraint::Length(0), Constraint::Length(0), Constraint::Length(0)]).areas(foot)
            } else {
                Layout::vertical([Constraint::Length(work), picker, Constraint::Length(3), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)]).areas(foot)
            };
            draw_work(frame, skin, work_a, ctx);
            picker_area = Some(picker_a);
            cursor = draw_grok_box(frame, box_a, ctx);
            draw_grok_bar(frame, bar_a, ctx);
        }
        LayoutId::GitLog | LayoutId::Man => {
            let [picker_a, line] = Layout::vertical([picker, Constraint::Length(1)]).areas(foot);
            picker_area = Some(picker_a);
            cursor = if let Some(input) = ctx.input {
                // less 里过滤目录对应搜索（`/` + 模式），跳号对应 `:` + 数字。
                let (prompt, rest) = split_cols(line, 1);
                frame.render_widget(Paragraph::new(if ctx.picker_open { PAGER_SEARCH } else { PAGER_PROMPT }), prompt);
                frame.render_widget(Paragraph::new(input), rest);
                cursor_after(rest, input)
            } else if let Some(note) = ctx.note {
                // less 把一次性消息画在底行提示符的位置。
                frame.render_widget(Paragraph::new(note).style(hl(p)), line);
                cursor_after(line, note)
            } else if ctx.at_end {
                frame.render_widget(Paragraph::new(PAGER_END).style(hl(p)), line);
                cursor_after(line, PAGER_END)
            } else {
                frame.render_widget(Paragraph::new(PAGER_PROMPT), line);
                cursor_after(line, PAGER_PROMPT)
            };
        }
        LayoutId::Native => cursor = None,
    }
    (picker_area.filter(|r| r.height > 0), cursor)
}

/// 工作行：spinner 栏 + 文案。Claude 一行，Codex / Grok 一行加一个空行，Grok 右侧还有耗时、token 和停止按钮。
fn draw_work(frame: &mut ratatui::Frame, skin: &Skin, area: Rect, ctx: &CoverCtx) {
    if area.height == 0 || !ctx.working {
        return;
    }
    let deco = deco_of(skin.id);
    let line = row(area, 0);
    let frame_glyph = spinner_frame(deco, ctx.now_ms);
    let text = work_line(deco, ctx.item_id, ctx.elapsed_s, ctx.tokens);
    match skin.id {
        LayoutId::Claude => {
            let rest = draw_glyph(frame, line, frame_glyph, Style::new().fg(CLAUDE_ORANGE));
            frame.render_widget(Paragraph::new(text), rest);
        }
        LayoutId::Codex => {
            let rest = draw_glyph(frame, line, frame_glyph, bold());
            frame.render_widget(Paragraph::new(text), rest);
        }
        LayoutId::Grok => {
            let (_, line) = split_cols(line, GROK_BOX_PAD);
            let rest = draw_glyph(frame, line, frame_glyph, Style::new().fg(grok_magenta()));
            frame.render_widget(Paragraph::new(text), rest);
            let right = grok_work_right(ctx.elapsed_s, ctx.tokens);
            let w = width_of(&right);
            if line.width > w + 20 {
                frame.render_widget(Paragraph::new(right).style(Style::new().fg(grok_gray())), right_cell(line, w, GROK_BOX_PAD));
            }
        }
        LayoutId::GitLog | LayoutId::Man | LayoutId::Native => {}
    }
}

/// 提示符行：提示符、输入、占位文案。目录打开时 agent 皮肤先画 @ 再画过滤词，像文件补全。返回光标位置。
fn draw_prompt(frame: &mut ratatui::Frame, line: Rect, prompt: &str, prompt_style: Style, ctx: &CoverCtx, placeholder: Option<&str>, p: Palette) -> Option<(u16, u16)> {
    if line.height == 0 {
        return None;
    }
    let [prompt_a, rest] = Layout::horizontal([Constraint::Length(1), Constraint::Fill(1)]).spacing(1).areas(line);
    frame.render_widget(Paragraph::new(prompt).style(prompt_style), prompt_a);
    let shown = match ctx.input {
        Some(filter) if ctx.picker_open => format!("@{filter}"),
        Some(input) => input.to_string(),
        None => String::new(),
    };
    if shown.is_empty() {
        if let Some(ph) = placeholder {
            frame.render_widget(Paragraph::new(ph).style(dim(p)), rest);
        }
    } else {
        frame.render_widget(Paragraph::new(shown.as_str()).style(Style::new().fg(p.fg)), rest);
    }
    cursor_after(rest, &shown)
}

/// Grok 的三行圆角输入框，底边右侧嵌模型和模式标签。返回光标位置。
fn draw_grok_box(frame: &mut ratatui::Frame, area: Rect, ctx: &CoverCtx) -> Option<(u16, u16)> {
    if area.height < 3 || area.width < GROK_BOX_PAD * 2 + 8 {
        return None;
    }
    let boxed = Rect { x: area.x + GROK_BOX_PAD, y: area.y, width: area.width - GROK_BOX_PAD * 2, height: 3 };
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(grok_border()));
    let inner = block.inner(boxed);
    frame.render_widget(block, boxed);
    let (_, inner) = split_cols(inner, 1);
    let cursor = draw_prompt(frame, inner, GROK_PROMPT, Style::new().fg(grok_text_secondary()), ctx, Some(GROK_PLACEHOLDER), palette(ThemeId::Auto));
    let label = Line::from(vec![Span::styled(format!(" {GROK_MODEL}"), Style::new().fg(Color::Rgb(136, 136, 136))), Span::styled(GROK_DOT, Style::new().fg(grok_gray_dim())), Span::styled(format!("{GROK_MODE} "), Style::new().fg(grok_gray()))]);
    let w = label.width() as u16;
    if boxed.width > w + 6 {
        let bottom = Rect { x: boxed.x + boxed.width - 2 - w, y: boxed.y + 2, width: w, height: 1 };
        frame.render_widget(Paragraph::new(label), bottom);
    }
    cursor
}

/// Grok 输入框下方的快捷键条：`Key:label` 用 `  │  ` 相接，一次性提示靠右。
fn draw_grok_bar(frame: &mut ratatui::Frame, area: Rect, ctx: &CoverCtx) {
    if area.height == 0 {
        return;
    }
    let (_, area) = split_cols(area, GROK_BOX_PAD);
    let mut spans = Vec::new();
    for (i, (key, label)) in GROK_BAR.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(GROK_BAR_SEP, Style::new().fg(grok_gray()).add_modifier(Modifier::DIM)));
        }
        spans.push(Span::styled(*key, bold().fg(grok_text_secondary())));
        spans.push(Span::styled(format!(":{label}"), Style::new().fg(grok_gray())));
    }
    let left = Line::from(spans);
    let lw = left.width() as u16;
    frame.render_widget(Paragraph::new(left), area);
    if let Some(note) = ctx.note {
        let w = width_of(note);
        if area.width > lw + w + STATUS_GAP {
            frame.render_widget(Paragraph::new(note).style(Style::new().fg(grok_gray())), right_cell(area, w, GROK_BOX_PAD));
        }
    }
}

/// 状态行：左侧可选的前缀（Claude 的模式徽标，给出从长到短的候选，取第一个放得下的）加皮肤的快捷键提示，
/// 右侧上下文余量之类的进度文本。宽度不够时先丢右段，再换短前缀。
fn draw_status(frame: &mut ratatui::Frame, area: Rect, ctx: &CoverCtx, prefixes: &[Line<'static>], right: Option<Line<'static>>, p: Palette) {
    if area.height == 0 {
        return;
    }
    let (_, area) = split_cols(area, STATUS_INDENT);
    let hints_w: u16 = ctx.hints.iter().map(|h| width_of(h.key) + 1 + width_of(h.label)).sum();
    let prefix = prefixes.iter().find(|l| l.width() as u16 + hints_w <= area.width).or(prefixes.last()).cloned();
    let prefix_w = prefix.as_ref().map(|l| l.width() as u16).unwrap_or(0);
    let right_w = right.as_ref().map(|l| l.width() as u16).unwrap_or(0);
    let right_w = if right_w == 0 || prefix_w + hints_w + STATUS_GAP + right_w + STATUS_INDENT > area.width { 0 } else { right_w };
    let [left, right_a, _] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(right_w), Constraint::Length(if right_w == 0 { 0 } else { STATUS_INDENT })]).spacing(if right_w == 0 { 0 } else { STATUS_GAP }).areas(area);
    let (prefix_a, hints_a) = split_cols(left, prefix_w.min(left.width));
    if let Some(line) = prefix {
        frame.render_widget(Paragraph::new(line), prefix_a);
    }
    draw_hints(frame, hints_a, ctx.hints, Flex::Start, p);
    if right_w > 0 && let Some(line) = right {
        frame.render_widget(Paragraph::new(line), right_a);
    }
}

/// 内联目录列表：只显示假文件名。Codex 的补全菜单选中行是青色粗体，其余皮肤用 Auto 主题的反显。
fn draw_picker(frame: &mut ratatui::Frame, skin: &Skin, area: Rect, items: Vec<String>, state: &mut ListState, p: Palette) {
    let (_, list_area) = split_cols(area, STATUS_INDENT);
    let items: Vec<ListItem> = items.into_iter().map(ListItem::new).collect();
    let highlight = if skin.id == LayoutId::Codex { bold().fg(Color::Cyan) } else { hl(p) };
    frame.render_stateful_widget(List::new(items).highlight_style(highlight), list_area, state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_width_respects_indent_and_cap() {
        let man = skin(LayoutId::Man);
        assert_eq!(content_width(&man, 10, 0), 4);
        let claude = skin(LayoutId::Claude);
        assert_eq!(content_width(&claude, 100, 40), 40);
        assert_eq!(content_width(&claude, 100, 0), 100 - 2 - 1);
        assert_eq!(content_width(&man, 80, 0), 80 - 5 - 2);
        assert_eq!(content_width(&skin(LayoutId::Grok), 80, 0), 80 - 5 - 5);
    }

    #[test]
    fn chrome_rows_degrades_by_height() {
        for id in [LayoutId::Claude, LayoutId::Codex, LayoutId::GitLog, LayoutId::Man, LayoutId::Grok] {
            let s = skin(id);
            assert_eq!(chrome_rows(&s, 30, true, false), (s.top_rows, s.bottom_rows), "{id:?}");
            assert_eq!(chrome_rows(&s, 30, false, false), (if s.top_fixed { s.top_rows } else { 0 }, s.bottom_rows), "{id:?}");
            assert_eq!(chrome_rows(&s, 30, false, true).1, s.bottom_rows + s.work_rows, "{id:?}");
            assert_eq!(chrome_rows(&s, 2, true, true), (0, 0), "{id:?}");
        }
        // 高度不够时先丢顶栏，再把底栏退化。
        assert_eq!(chrome_rows(&skin(LayoutId::GitLog), 12, true, false).0, 0);
        assert_eq!(chrome_rows(&skin(LayoutId::Codex), 10, true, false).0, 0);
        assert_eq!(chrome_rows(&skin(LayoutId::Claude), 5, true, false), (0, 1));
        assert_eq!(chrome_rows(&skin(LayoutId::Codex), 5, true, false), (0, 2));
        assert_eq!(chrome_rows(&skin(LayoutId::Grok), 6, true, false), (0, 3));
        assert_eq!(chrome_rows(&skin(LayoutId::Native), 24, true, true), (0, 0));
    }
}
