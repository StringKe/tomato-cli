mod disguise;
mod layout;
mod overlay;
mod stage;
mod workspace;

pub(crate) use disguise::{cover_window_title, reflow_note};

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, Overlay, Screen};
use crate::theme::palette;
use layout::{dim, draw_hints, width_of};
use overlay::{draw_cookie, draw_folder_input, draw_folder_pick, draw_help, draw_jump, draw_organize, draw_profile, draw_settings, draw_toc};
use stage::{draw_home, draw_login};
use workspace::{draw_book, draw_rank, draw_reader, draw_search, draw_shelf};

/// 页脚右侧各段之间的间隔列数。
const FOOTER_GAP: u16 = 2;

pub fn draw(app: &mut App, frame: &mut ratatui::Frame) {
    app.image_rects.clear();
    // 伪装态整帧交给 disguise，先清掉原生布局的命中区，避免鼠标点到不存在的头部元素。
    if app.covered() {
        app.help_hit = Rect::default();
        app.profile_hit = Rect::default();
        app.menu_hits.clear();
        app.folder_tabs.clear();
        if app.overlay != Overlay::Toc {
            app.overlay_area = Rect::default();
        }
        disguise::draw_cover(app, frame);
        return;
    }
    let p = palette(app.state.settings.theme);
    if !p.inherit {
        frame.render_widget(Block::new().style(Style::new().bg(p.bg).fg(p.fg)), frame.area());
    }
    let stage = matches!(app.screen(), Screen::Home | Screen::Login);
    if stage {
        let [body, footer] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        app.help_hit = Rect::default();
        app.profile_hit = Rect::default();
        match app.screen() {
            Screen::Home => draw_home(app, frame, body),
            Screen::Login => draw_login(app, frame, body),
            _ => {}
        }
        draw_footer(app, frame, footer, p);
    } else {
        let [header, body, footer] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        draw_header(app, frame, header, p);
        match app.screen() {
            Screen::Shelf => draw_shelf(app, frame, body),
            Screen::Search => draw_search(app, frame, body),
            Screen::Rank => draw_rank(app, frame, body),
            Screen::Book => draw_book(app, frame, body),
            Screen::Reader => draw_reader(app, frame, body),
            _ => {}
        }
        draw_footer(app, frame, footer, p);
    }
    match app.overlay {
        Overlay::None => app.overlay_area = Rect::default(),
        Overlay::Help => draw_help(app, frame),
        Overlay::Toc => draw_toc(app, frame),
        Overlay::Jump => draw_jump(app, frame),
        Overlay::FolderPick => draw_folder_pick(app, frame),
        Overlay::FolderInput => draw_folder_input(app, frame),
        Overlay::Cookie => draw_cookie(app, frame),
        Overlay::Settings => draw_settings(app, frame),
        Overlay::Profile => draw_profile(app, frame),
        Overlay::Organize => draw_organize(app, frame),
    }
}

fn draw_header(app: &mut App, frame: &mut ratatui::Frame, header: Rect, p: crate::theme::Palette) {
    let user = app.state.user.as_ref().map(|u| u.name.as_str()).unwrap_or("未登录");
    let title = match app.screen() {
        Screen::Home => "首页",
        Screen::Shelf => "书架",
        Screen::Search => "搜索",
        Screen::Rank => "榜单",
        Screen::Book => "目录",
        Screen::Reader => "阅读",
        Screen::Login => "登录",
    };
    let tomato = " tomato ";
    let ver = format!("v{}", env!("CARGO_PKG_VERSION"));
    let busy_w = u16::from(app.busy);
    let [brand, title_a, rest] = Layout::horizontal([Constraint::Length(width_of(tomato)), Constraint::Length(width_of(title)), Constraint::Fill(1)]).spacing(1).areas(header);
    let [user_a, ver_a, busy_a] = Layout::horizontal([Constraint::Length(width_of(user).max(1)), Constraint::Length(width_of(&ver)), Constraint::Length(busy_w)]).spacing(FOOTER_GAP).flex(Flex::End).areas(rest);
    app.help_hit = brand;
    app.profile_hit = user_a;
    frame.render_widget(Paragraph::new(Span::styled(tomato, Style::new().fg(Color::Black).bg(p.accent).add_modifier(Modifier::BOLD))), brand);
    frame.render_widget(Paragraph::new(Span::styled(title, Style::new().fg(p.title))), title_a);
    frame.render_widget(Paragraph::new(Span::styled(user, Style::new().fg(p.title))), user_a);
    frame.render_widget(Paragraph::new(Span::styled(ver, dim(p))), ver_a);
    if app.busy {
        frame.render_widget(Paragraph::new(Span::styled("…", Style::new().fg(p.accent))), busy_a);
    }
}

/// 页脚：左侧当前上下文的按键提示，右侧状态消息、新版本提示和忙碌标记。
fn draw_footer(app: &App, frame: &mut ratatui::Frame, footer: Rect, p: crate::theme::Palette) {
    let stage = matches!(app.screen(), Screen::Home | Screen::Login);
    let update = app.update_hint.as_deref().map(|v| format!("新版本 {v}"));
    let mut right: Vec<(&str, Style)> = Vec::new();
    if !app.status.is_empty() {
        right.push((app.status.as_str(), dim(p)));
    }
    if let Some(u) = update.as_deref() {
        right.push((u, Style::new().fg(p.accent)));
    }
    if stage && app.busy {
        right.push(("…", Style::new().fg(p.accent)));
    }
    let right_w: u16 = right.iter().enumerate().map(|(i, (s, _))| width_of(s) + if i == 0 { 0 } else { FOOTER_GAP }).sum();
    let right_w = right_w.min(footer.width.saturating_mul(2) / 3);
    let [left, right_a] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(right_w)]).spacing(if right_w == 0 { 0 } else { FOOTER_GAP }).areas(footer);
    let flex = if stage && right.is_empty() { Flex::Center } else { Flex::Start };
    draw_hints(frame, left, app.hints(), flex, p);
    if right.is_empty() {
        return;
    }
    let cells = Layout::horizontal(right.iter().map(|(s, _)| Constraint::Length(width_of(s)))).spacing(FOOTER_GAP).flex(Flex::End).split(right_a);
    for ((text, style), cell) in right.iter().zip(cells.iter()) {
        frame.render_widget(Paragraph::new(*text).style(*style), *cell);
    }
}
