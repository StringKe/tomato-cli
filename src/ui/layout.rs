use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::Hint;
use crate::model::Chapter;
use crate::theme::Palette;

const STAGE_WIDTH: u16 = 48;
const STAGE_MAX: u16 = 56;
const WORK_WIDTH: u16 = 88;
/// 相邻按键提示之间的间隔列数。所有提示排布只用这一处常量，不在文案里补空格。
pub(super) const HINT_GAP: u16 = 3;

pub(super) fn width_of(s: &str) -> u16 {
    UnicodeWidthStr::width(s) as u16
}

pub(super) fn hl(p: Palette) -> Style {
    if p.reversed {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new().bg(p.highlight_bg).fg(p.highlight_fg).add_modifier(Modifier::BOLD)
    }
}

pub(super) fn dim(p: Palette) -> Style {
    Style::new().fg(p.dim)
}

fn key_style(p: Palette) -> Style {
    Style::new().fg(p.fg)
}

pub(super) fn block<'a>(title: &str, p: Palette) -> Block<'a> {
    let mut b = Block::bordered().border_style(Style::new().fg(p.border));
    if !title.is_empty() {
        b = b.title(title.to_string());
    }
    if p.inherit {
        b
    } else {
        b.style(Style::new().bg(p.bg).fg(p.fg))
    }
}

pub(super) fn popup(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2)).max(1);
    let h = h.min(area.height.saturating_sub(2)).max(1);
    Rect { x: area.x + (area.width.saturating_sub(w)) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h }
}

pub(super) fn inner(area: Rect) -> Rect {
    if area.width < 3 || area.height < 3 {
        return area;
    }
    Rect { x: area.x + 1, y: area.y + 1, width: area.width - 2, height: area.height - 2 }
}

pub(super) fn center_h(area: Rect, width: u16) -> Rect {
    let w = width.min(area.width).max(1);
    let [_, mid, _] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(w), Constraint::Fill(1)]).flex(Flex::Center).areas(area);
    mid
}

pub(super) fn center_box(area: Rect, width: u16, height: u16) -> Rect {
    let h = height.min(area.height).max(1);
    let [_, mid, _] = Layout::vertical([Constraint::Fill(1), Constraint::Length(h), Constraint::Fill(1)]).flex(Flex::Center).areas(area);
    center_h(mid, width)
}

pub(super) fn stage_inset(area: Rect) -> u16 {
    if area.width < 40 { 2 } else { 4 }
}

pub(super) fn stage_col_width(area: Rect, extra: u16) -> u16 {
    let avail = area.width.saturating_sub(stage_inset(area)).max(1);
    STAGE_WIDTH.max(extra).min(STAGE_MAX).min(avail).max(1)
}

pub(super) fn work_col(area: Rect) -> Rect {
    let v: u16 = u16::from(area.height > 4);
    let hpad: u16 = if area.width < 64 { 1 } else { 2 };
    let body = Rect { x: area.x, y: area.y.saturating_add(v), width: area.width, height: area.height.saturating_sub(v.saturating_mul(2)).max(1) };
    let w = if body.width < 64 { body.width.saturating_sub(hpad.saturating_mul(2)).max(1) } else { WORK_WIDTH.min(body.width.saturating_sub(hpad.saturating_mul(2))).max(1) };
    center_h(body, w)
}

pub(super) fn hint_width(h: &Hint) -> u16 {
    width_of(h.key) + 1 + width_of(h.label)
}

/// 从头开始数能放进 width 的提示条数。整条放不下就整条丢弃，不截断。
pub(super) fn fit_hints(hints: &[Hint], width: u16) -> usize {
    let mut used = 0u16;
    for (i, h) in hints.iter().enumerate() {
        let need = if i == 0 { hint_width(h) } else { used.saturating_add(HINT_GAP).saturating_add(hint_width(h)) };
        if need > width {
            return i;
        }
        used = need;
    }
    hints.len()
}

pub(super) fn hints_width(hints: &[Hint]) -> u16 {
    hints.iter().enumerate().map(|(i, h)| hint_width(h) + if i == 0 { 0 } else { HINT_GAP }).sum()
}

/// 把提示按 flex 排进一行，键和说明分列渲染。返回实际绘制的条数。
pub(super) fn draw_hints(frame: &mut ratatui::Frame, area: Rect, hints: &[Hint], flex: Flex, p: Palette) -> usize {
    if area.height == 0 {
        return 0;
    }
    let n = fit_hints(hints, area.width);
    let shown = &hints[..n];
    if shown.is_empty() {
        return 0;
    }
    let cells = Layout::horizontal(shown.iter().map(|h| Constraint::Length(hint_width(h)))).spacing(HINT_GAP).flex(flex).split(area);
    for (h, cell) in shown.iter().zip(cells.iter()) {
        let [k, l] = Layout::horizontal([Constraint::Length(width_of(h.key)), Constraint::Fill(1)]).spacing(1).areas(*cell);
        frame.render_widget(Paragraph::new(h.key).style(key_style(p)), k);
        frame.render_widget(Paragraph::new(h.label).style(dim(p)), l);
    }
    n
}

/// 在带边框区域的底边居中画提示，先清掉被覆盖的边框段。
pub(super) fn bottom_hints(frame: &mut ratatui::Frame, area: Rect, hints: &[Hint], p: Palette) {
    if area.height < 2 || area.width < 5 {
        return;
    }
    let row = Rect { x: area.x + 2, y: area.y + area.height - 1, width: area.width - 4, height: 1 };
    let n = fit_hints(hints, row.width.saturating_sub(2));
    if n == 0 {
        return;
    }
    let strip = center_h(row, hints_width(&hints[..n]) + 2);
    frame.render_widget(Clear, strip);
    if !p.inherit {
        frame.render_widget(Block::new().style(Style::new().bg(p.bg)), strip);
    }
    let body = Rect { x: strip.x + 1, y: strip.y, width: strip.width.saturating_sub(2), height: 1 };
    draw_hints(frame, body, &hints[..n], Flex::Start, p);
}

/// 键在左、说明在右的纵向菜单。centered 为 false 时靠区域左边。
pub(super) fn draw_menu(frame: &mut ratatui::Frame, area: Rect, items: &[Hint], selected: usize, centered: bool, p: Palette) -> (Rect, Vec<(Rect, usize)>) {
    let key_w = items.iter().map(|h| width_of(h.key)).max().unwrap_or(1).max(2);
    let w = items.iter().map(|h| key_w + 1 + width_of(h.label)).max().unwrap_or(8).min(area.width).max(1);
    let h = (items.len() as u16).min(area.height).max(1);
    let x = if centered { center_h(area, w).x } else { area.x };
    let col = Rect { x, y: area.y, width: w, height: h };
    let mut hits = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let y = col.y.saturating_add(i as u16);
        if y >= col.y.saturating_add(col.height) {
            break;
        }
        let row = Rect { x: col.x, y, width: col.width, height: 1 };
        let style = if i == selected { hl(p) } else { Style::new() };
        if i == selected {
            frame.render_widget(Block::new().style(style), row);
        }
        let [k, l] = Layout::horizontal([Constraint::Length(key_w), Constraint::Fill(1)]).spacing(1).areas(row);
        frame.render_widget(Paragraph::new(item.key).style(if i == selected { style } else { dim(p) }), k);
        frame.render_widget(Paragraph::new(item.label).style(style), l);
        hits.push((row, i));
    }
    (col, hits)
}

/// 标签列和值列之间的间隔。
const KV_GAP: u16 = 2;

/// 一组 kv 行共用的标签列宽，按最宽标签算。
pub(super) fn label_width<'a>(labels: impl IntoIterator<Item = &'a str>) -> u16 {
    labels.into_iter().map(width_of).max().unwrap_or(1)
}

pub(super) fn draw_kv(frame: &mut ratatui::Frame, area: Rect, label: &str, value: &str, selected: bool, p: Palette, label_w: u16) {
    if selected {
        frame.render_widget(Block::new().style(hl(p)), area);
    }
    let [lab, val] = Layout::horizontal([Constraint::Length(label_w), Constraint::Fill(1)]).spacing(KV_GAP).areas(area);
    let lab_style = if selected { hl(p) } else { dim(p) };
    let val_style = if selected { hl(p) } else { Style::new() };
    frame.render_widget(Paragraph::new(label).style(lab_style), lab);
    frame.render_widget(Paragraph::new(value).style(val_style), val);
}

/// 章节列表行：序号加标题，付费章节追加弱化标记。
pub(super) fn chapter_item(index: usize, ch: &Chapter, p: Palette) -> ListItem<'static> {
    let mut spans = vec![Span::raw(format!("{}. {}", index + 1, ch.title))];
    if ch.need_pay {
        spans.push(Span::raw(" "));
        spans.push(Span::styled("付费", dim(p)));
    }
    ListItem::new(Line::from(spans))
}

pub(super) fn set_cursor_in(frame: &mut ratatui::Frame, content: Rect, text: &str) {
    if content.width == 0 || content.height == 0 {
        return;
    }
    let col = width_of(text);
    let x = content.x.saturating_add(col.min(content.width.saturating_sub(1)));
    frame.set_cursor_position((x, content.y));
}

pub(super) fn set_input_cursor(frame: &mut ratatui::Frame, box_area: Rect, text: &str) {
    set_cursor_in(frame, inner(box_area), text);
}

pub(super) fn draw_wordmark(frame: &mut ratatui::Frame, area: Rect, subtitle: &str, p: Palette) -> Rect {
    let tomato = Line::from(Span::styled("tomato", Style::new().fg(p.accent).add_modifier(Modifier::BOLD))).alignment(Alignment::Center);
    if subtitle.is_empty() || area.height < 2 {
        frame.render_widget(Paragraph::new(tomato), area);
    } else {
        let sub = Line::from(Span::styled(subtitle, dim(p))).alignment(Alignment::Center);
        frame.render_widget(Paragraph::new(vec![tomato, sub]), area);
    }
    Rect { x: area.x + area.width.saturating_sub(6) / 2, y: area.y, width: 6.min(area.width), height: 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::hint;

    #[test]
    fn fit_hints_drops_whole_entries() {
        let hints = [hint("enter", "确认"), hint("esc", "关闭"), hint("q", "退出")];
        // "enter 确认" = 10, gap 3, "esc 关闭" = 8, gap 3, "q 退出" = 6
        assert_eq!(fit_hints(&hints, 30), 3);
        assert_eq!(fit_hints(&hints, 29), 2);
        assert_eq!(fit_hints(&hints, 21), 2);
        assert_eq!(fit_hints(&hints, 20), 1);
        assert_eq!(fit_hints(&hints, 9), 0);
        assert_eq!(hints_width(&hints), 30);
    }
}
