use qrcode::{EcLevel, QrCode};
use ratatui::layout::{Alignment, Constraint, Layout, Rect, Size};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use tui_qrcode::{Colors, QrCodeWidget, QuietZone, Scaling};

use super::layout::{center_box, dim, draw_menu, draw_wordmark, stage_col_width};
use crate::app::{App, HOME_ITEMS, LOGIN_ITEMS};
use crate::theme::palette;

const MARK_H: u16 = 2;
const GAP: u16 = 1;
const MSG_H: u16 = 1;
/// 二维码静区：左右各 2 列，上下各 1 行（半块字符一行两模块），四边都是 2 模块。
const QR_PAD_X: u16 = 2;
const QR_PAD_Y: u16 = 1;
/// 放大倍数上限。1 倍已可扫，2 倍留给大屏。
const QR_MAX_SCALE: u16 = 2;

pub(super) fn draw_home(app: &mut App, frame: &mut ratatui::Frame, area: Rect) {
    let p = palette(app.state.settings.theme);
    app.folder_tabs.clear();
    let menu_h = HOME_ITEMS.len() as u16;
    let card_w = stage_col_width(area, 0);
    let card_h = MARK_H + GAP + menu_h + GAP + 1;
    let card = center_box(area, card_w, card_h);
    let [mark, _, menu, _, ver] = Layout::vertical([Constraint::Length(MARK_H), Constraint::Length(GAP), Constraint::Length(menu_h), Constraint::Length(GAP), Constraint::Length(1)]).areas(card);
    app.help_hit = draw_wordmark(frame, mark, "番茄小说阅读器", p);
    let (col, hits) = draw_menu(frame, menu, &HOME_ITEMS, app.home_list.selected().unwrap_or(0), true, p);
    app.menu_hits = hits;
    app.list_area = col;
    frame.render_widget(Paragraph::new(Line::from(Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), dim(p))).alignment(Alignment::Center)), ver);
}

/// 登录页可选的装饰行。二维码放不下时依次去掉菜单和字标，给二维码让位。
#[derive(Clone, Copy)]
struct Chrome {
    mark: bool,
    menu: bool,
}

impl Chrome {
    const LEVELS: [Chrome; 3] = [Chrome { mark: true, menu: true }, Chrome { mark: true, menu: false }, Chrome { mark: false, menu: false }];

    fn height(self) -> u16 {
        let menu_h = LOGIN_ITEMS.len() as u16;
        u16::from(self.mark) * (MARK_H + GAP) + u16::from(self.menu) * (menu_h + GAP) + GAP + MSG_H
    }
}

enum QrPlan {
    Waiting,
    Invalid,
    TooSmall { need: Size },
    Fit { widget: QrCodeWidget, size: Size, chrome: Chrome },
}

/// 二维码在 max_w 列 max_h 行内能用的最大整数倍率。
fn qr_scale(modules: u16, max_w: u16, max_h: u16) -> Option<u16> {
    (1..=QR_MAX_SCALE).rev().find(|s| qr_size(modules, *s).width <= max_w && qr_size(modules, *s).height <= max_h)
}

/// 含静区的二维码占位尺寸。半块字符一行两模块。
fn qr_size(modules: u16, scale: u16) -> Size {
    let side = modules.saturating_mul(scale);
    Size::new(side + QR_PAD_X * 2, side.div_ceil(2) + QR_PAD_Y * 2)
}

fn plan_qr(payload: &str, area: Rect) -> QrPlan {
    if payload.is_empty() {
        return QrPlan::Waiting;
    }
    // 纠错级别 L 比默认的 M 少 1 到 2 个版本，等价于省下 4 到 8 行。
    let Ok(code) = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::L) else {
        return QrPlan::Invalid;
    };
    let modules = code.width() as u16;
    for chrome in Chrome::LEVELS {
        let max_h = area.height.saturating_sub(chrome.height());
        if let Some(scale) = qr_scale(modules, area.width, max_h) {
            let widget = QrCodeWidget::new(code).quiet_zone(QuietZone::Disabled).scaling(Scaling::Exact(scale, scale)).colors(Colors::Normal).style(Style::new().fg(Color::Black).bg(Color::White));
            return QrPlan::Fit { widget, size: qr_size(modules, scale), chrome };
        }
    }
    let bare = qr_size(modules, 1);
    QrPlan::TooSmall { need: Size::new(bare.width, bare.height + Chrome::LEVELS[2].height()) }
}

pub(super) fn draw_login(app: &mut App, frame: &mut ratatui::Frame, area: Rect) {
    let p = palette(app.state.settings.theme);
    app.folder_tabs.clear();
    app.menu_hits.clear();
    app.list_area = Rect::default();
    let plan = plan_qr(&app.login_qr, area);
    let (chrome, slot_h, slot_w) = match &plan {
        QrPlan::Fit { size, chrome, .. } => (*chrome, size.height, size.width),
        QrPlan::Waiting => (Chrome::LEVELS[0], 0, 0),
        QrPlan::Invalid | QrPlan::TooSmall { .. } => (Chrome::LEVELS[0], 1, 0),
    };
    let menu_h = LOGIN_ITEMS.len() as u16;
    let mark_h = u16::from(chrome.mark) * MARK_H;
    let mark_gap = u16::from(chrome.mark) * GAP;
    let menu_rows = u16::from(chrome.menu) * menu_h;
    let menu_gap = u16::from(chrome.menu) * GAP;
    let card_w = slot_w.max(stage_col_width(area, 0)).min(area.width);
    let card_h = (mark_h + mark_gap + menu_rows + menu_gap + slot_h + GAP + MSG_H).min(area.height);
    let card = center_box(area, card_w, card_h);
    let [mark, _, menu, _, slot, _, msg] = Layout::vertical([
        Constraint::Length(mark_h),
        Constraint::Length(mark_gap),
        Constraint::Length(menu_rows),
        Constraint::Length(menu_gap),
        Constraint::Length(slot_h),
        Constraint::Length(GAP),
        Constraint::Length(MSG_H),
    ])
    .areas(card);
    if chrome.mark {
        app.help_hit = draw_wordmark(frame, mark, "登录", p);
    } else {
        app.help_hit = Rect::default();
    }
    if chrome.menu {
        let (col, hits) = draw_menu(frame, menu, &LOGIN_ITEMS, app.login_list.selected().unwrap_or(0), true, p);
        app.menu_hits = hits;
        app.list_area = col;
    }
    match plan {
        QrPlan::Fit { widget, size, .. } => {
            let boxed = center_box(slot, size.width, size.height);
            frame.render_widget(Block::new().style(Style::new().bg(Color::White)), boxed);
            let code = Rect { x: boxed.x + QR_PAD_X, y: boxed.y + QR_PAD_Y, width: boxed.width.saturating_sub(QR_PAD_X * 2), height: boxed.height.saturating_sub(QR_PAD_Y * 2) };
            frame.render_widget(widget, code);
        }
        QrPlan::TooSmall { need } => {
            let text = format!("二维码需要 {}×{} 的窗口，当前 {}×{}", need.width, need.height + 1, area.width, area.height + 1);
            frame.render_widget(Paragraph::new(text).style(dim(p)).alignment(Alignment::Center), full_row(area, slot));
        }
        QrPlan::Invalid => frame.render_widget(Paragraph::new("二维码内容无法编码，请改用 Cookie 登录").style(dim(p)).alignment(Alignment::Center), full_row(area, slot)),
        QrPlan::Waiting => {}
    }
    frame.render_widget(Paragraph::new(app.login_msg.as_str()).style(dim(p)).alignment(Alignment::Center), full_row(area, msg));
}

/// 文案行不受卡片宽度限制，横跨整个区域居中。
fn full_row(area: Rect, row: Rect) -> Rect {
    Rect { x: area.x, y: row.y, width: area.width, height: row.height }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_size_uses_half_blocks_and_pad() {
        assert_eq!(qr_size(41, 1), Size::new(45, 23));
        assert_eq!(qr_size(41, 2), Size::new(86, 43));
    }

    #[test]
    fn qr_scale_prefers_two_then_one() {
        assert_eq!(qr_scale(41, 200, 60), Some(2));
        assert_eq!(qr_scale(41, 200, 30), Some(1));
        assert_eq!(qr_scale(41, 44, 30), None);
        assert_eq!(qr_scale(41, 200, 22), None);
    }

    #[test]
    fn long_payload_fits_54_rows_by_dropping_menu() {
        let payload: String = "https://sso.douyin.com/check_qrconnect/?next=https%3A%2F%2Ffanqienovel.com%2F&token=".chars().chain(std::iter::repeat_n('a', 600)).collect();
        let area = Rect { x: 0, y: 0, width: 200, height: 53 };
        match plan_qr(&payload, area) {
            QrPlan::Fit { chrome, size, .. } => {
                assert!(!chrome.menu);
                assert!(size.height + chrome.height() <= area.height);
            }
            _ => panic!("600 字节负载应在 200×53 内放下"),
        }
    }

    #[test]
    fn short_payload_keeps_menu() {
        let area = Rect { x: 0, y: 0, width: 200, height: 53 };
        match plan_qr("https://fanqienovel.com/?token=abcdef", area) {
            QrPlan::Fit { chrome, .. } => assert!(chrome.menu && chrome.mark),
            _ => panic!("短负载应完整显示"),
        }
    }

    #[test]
    fn tiny_terminal_reports_needed_size() {
        let area = Rect { x: 0, y: 0, width: 40, height: 10 };
        assert!(matches!(plan_qr("https://fanqienovel.com/?token=abcdef", area), QrPlan::TooSmall { .. }));
        assert!(matches!(plan_qr("", area), QrPlan::Waiting));
    }
}
