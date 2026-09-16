use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::widgets::{Clear, List, ListItem, Paragraph, Wrap};

use super::layout::{block, bottom_hints, chapter_item, dim, draw_hints, draw_kv, draw_menu, hints_width, hl, inner, label_width, popup, set_cursor_in, set_input_cursor, width_of};
use crate::app::{App, HELP_SECTIONS, Overlay, PROFILE_ITEMS};
use crate::store;
use crate::theme::palette;

pub(super) fn draw_settings(app: &mut App, frame: &mut ratatui::Frame) {
    let p = palette(app.state.settings.theme);
    let rows = app.settings_pairs();
    let h = (rows.len() as u16 + 2).max(9);
    let area = popup(frame.area(), 52, h);
    app.overlay_area = area;
    frame.render_widget(Clear, area);
    frame.render_widget(block("设置", p), area);
    let body = inner(area);
    app.settings_list.select(Some(app.settings_idx));
    app.list_area = body;
    let label_w = label_width(rows.iter().map(|(l, _)| *l));
    for (i, (label, value)) in rows.iter().enumerate() {
        let y = body.y.saturating_add(i as u16);
        if y >= body.y.saturating_add(body.height) {
            break;
        }
        let row = Rect { x: body.x, y, width: body.width, height: 1 };
        draw_kv(frame, row, label, value, i == app.settings_idx, p, label_w);
    }
    bottom_hints(frame, area, app.hints(), p);
}

pub(super) fn draw_profile(app: &mut App, frame: &mut ratatui::Frame) {
    let p = palette(app.state.settings.theme);
    let (name, id, vip, desc) = match &app.state.user {
        Some(u) => (u.name.as_str(), u.id.as_str(), if u.is_vip { "是" } else { "否" }, u.desc.as_str()),
        None => ("未登录", "-", "-", ""),
    };
    let cfg = store::config_dir().ok().map(|d| d.display().to_string()).unwrap_or_else(|| "-".into());
    let shelf_n = format!("{} 本", app.state.shelf.len());
    let prog_n = format!("{} 本", app.state.progress.len());
    let folder_n = store::all_folders(&app.state).len().to_string();
    let rows = [("昵称", name), ("ID", id), ("VIP", vip), ("简介", desc), ("书架", shelf_n.as_str()), ("进度", prog_n.as_str()), ("文件夹", folder_n.as_str()), ("配置", cfg.as_str()), ("版本", env!("CARGO_PKG_VERSION"))];
    let label_w = label_width(rows.iter().map(|(l, _)| *l));
    let value_w = rows.iter().map(|(_, v)| width_of(v)).max().unwrap_or(0);
    // 宽度按最长的值（通常是配置路径）算，避免截断。
    let area = popup(frame.area(), (label_w + 2 + value_w + 2).max(52), rows.len() as u16 + PROFILE_ITEMS.len() as u16 + 4);
    app.overlay_area = area;
    frame.render_widget(Clear, area);
    frame.render_widget(block("资料", p), area);
    let inner_area = inner(area);
    let actions_h = PROFILE_ITEMS.len() as u16;
    let [info, actions] = Layout::vertical([Constraint::Fill(1), Constraint::Length(actions_h)]).areas(inner_area);
    for (i, (label, value)) in rows.iter().enumerate() {
        let y = info.y.saturating_add(i as u16);
        if y >= info.y.saturating_add(info.height) {
            break;
        }
        let row = Rect { x: info.x, y, width: info.width, height: 1 };
        draw_kv(frame, row, label, value, false, p, label_w);
    }
    let (col, _) = draw_menu(frame, actions, &PROFILE_ITEMS, app.profile_list.selected().unwrap_or(0), false, p);
    app.list_area = col;
    bottom_hints(frame, area, app.hints(), p);
}

/// 帮助表：左列区域名，右列该区域的按键提示，按宽度自动丢弃放不下的条目。
pub(super) fn draw_help(app: &mut App, frame: &mut ratatui::Frame) {
    let p = palette(app.state.settings.theme);
    let label_w = HELP_SECTIONS.iter().map(|(name, _)| width_of(name)).max().unwrap_or(4);
    let widest = HELP_SECTIONS.iter().map(|(_, hints)| hints_width(hints)).max().unwrap_or(0);
    // 宽度按最长一行算，窄终端时 popup 会收缩，draw_hints 再按行丢弃放不下的条目。
    let area = popup(frame.area(), label_w + 2 + widest + 4, HELP_SECTIONS.len() as u16 + 2);
    app.overlay_area = area;
    frame.render_widget(Clear, area);
    frame.render_widget(block("帮助", p), area);
    let body = inner(area);
    for (i, (name, hints)) in HELP_SECTIONS.iter().enumerate() {
        let y = body.y.saturating_add(i as u16);
        if y >= body.y.saturating_add(body.height) {
            break;
        }
        let row = Rect { x: body.x, y, width: body.width, height: 1 };
        let [lab, rest] = Layout::horizontal([Constraint::Length(label_w), Constraint::Fill(1)]).spacing(2).areas(row);
        frame.render_widget(Paragraph::new(*name).style(dim(p)), lab);
        draw_hints(frame, rest, hints, Flex::Start, p);
    }
    bottom_hints(frame, area, app.hints(), p);
}

pub(super) fn draw_toc(app: &mut App, frame: &mut ratatui::Frame) {
    let p = palette(app.state.settings.theme);
    let area = popup(frame.area(), 60, 20);
    app.overlay_area = area;
    frame.render_widget(Clear, area);
    let items: Vec<ListItem> = app.toc_items().into_iter().map(|(i, c)| chapter_item(i, c, p)).collect();
    let title = if app.toc_filter.is_empty() { "目录".to_string() } else { format!("目录：{}", app.toc_filter) };
    let list = List::new(items).block(block(&title, p)).highlight_style(hl(p));
    app.list_area = inner(area);
    frame.render_stateful_widget(list, area, &mut app.toc_list);
    bottom_hints(frame, area, app.hints(), p);
}

pub(super) fn draw_cookie(app: &mut App, frame: &mut ratatui::Frame) {
    let p = palette(app.state.settings.theme);
    let area = popup(frame.area(), 72, 8);
    app.overlay_area = area;
    frame.render_widget(Clear, area);
    let empty = app.cookie_buf.is_empty();
    let shown = if empty { "粘贴浏览器 Cookie" } else { app.cookie_buf.as_str() };
    let para = Paragraph::new(shown).wrap(Wrap { trim: false }).block(block("Cookie", p));
    frame.render_widget(if empty { para.style(dim(p)) } else { para }, area);
    bottom_hints(frame, area, app.hints(), p);
    if app.overlay == Overlay::Cookie {
        set_input_cursor(frame, area, if empty { "" } else { &app.cookie_buf });
    }
}

pub(super) fn draw_jump(app: &mut App, frame: &mut ratatui::Frame) {
    let p = palette(app.state.settings.theme);
    let area = popup(frame.area(), 40, 5);
    app.overlay_area = area;
    frame.render_widget(Clear, area);
    frame.render_widget(block("跳转", p), area);
    let body = inner(area);
    let [label, value] = Layout::horizontal([Constraint::Length(width_of("第")), Constraint::Fill(1)]).spacing(1).areas(body);
    frame.render_widget(Paragraph::new("第").style(dim(p)), label);
    frame.render_widget(Paragraph::new(app.jump_buf.as_str()), value);
    bottom_hints(frame, area, app.hints(), p);
    if app.overlay == Overlay::Jump {
        set_cursor_in(frame, value, &app.jump_buf);
    }
}

pub(super) fn draw_folder_pick(app: &mut App, frame: &mut ratatui::Frame) {
    let p = palette(app.state.settings.theme);
    let folders = store::all_folders(&app.state);
    let area = popup(frame.area(), 40, (folders.len() as u16 + 3).max(6));
    app.overlay_area = area;
    frame.render_widget(Clear, area);
    let items: Vec<ListItem> = folders.iter().map(|f| ListItem::new(f.as_str())).collect();
    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(app.folder_idx));
    let list = List::new(items).block(block("移动到", p)).highlight_style(hl(p));
    app.list_area = inner(area);
    frame.render_stateful_widget(list, area, &mut state);
    bottom_hints(frame, area, app.hints(), p);
}

pub(super) fn draw_folder_input(app: &mut App, frame: &mut ratatui::Frame) {
    let p = palette(app.state.settings.theme);
    let area = popup(frame.area(), 48, 5);
    app.overlay_area = area;
    frame.render_widget(Clear, area);
    let title = if app.folder_rename { "重命名" } else { "新建文件夹" };
    frame.render_widget(Paragraph::new(app.folder_buf.as_str()).block(block(title, p)), area);
    bottom_hints(frame, area, app.hints(), p);
    if matches!(app.overlay, Overlay::FolderInput) {
        set_input_cursor(frame, area, &app.folder_buf);
    }
}
