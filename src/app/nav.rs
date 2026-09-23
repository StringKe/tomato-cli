use std::io;
use std::time::Duration;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;
use ratatui::Terminal;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate, SetTitle};
use ratatui::widgets::ListState;

use crate::store;

use super::backend::WideBackend;
use super::{App, Overlay, Screen};

/// 一帧最多合并处理的积压事件数。
const EVENT_BATCH: usize = 256;
/// 有后台任务或定时动画时的输入等待上限：后台消息只在循环醒来时收，要及时上屏。
const POLL_ACTIVE: Duration = Duration::from_millis(16);
/// 什么都没在跑时的等待上限。没有输入就没有事做，不必每秒醒 60 次。
const POLL_IDLE: Duration = Duration::from_millis(250);

impl App {
    /// 切屏时清空 status，避免上一屏的消息留在新屏的页脚。
    pub(super) fn push(&mut self, s: Screen) {
        if self.screen() != s {
            self.screens.push(s);
            self.status.clear();
        }
        self.mark();
    }

    pub(super) fn back(&mut self) {
        if self.overlay == Overlay::Organize {
            self.cancel_organize();
            return;
        }
        if self.overlay != Overlay::None {
            self.overlay = Overlay::None;
            self.mark();
            return;
        }
        if self.screens.len() > 1 {
            if self.screen() == Screen::Reader {
                self.save_reader_progress();
                // 离开阅读页后预读落盘也不再自动打开那一章。
                self.pending_open = None;
            }
            self.screens.pop();
            self.status.clear();
            self.mark();
        }
    }

    pub(super) fn loop_ui(&mut self, terminal: &mut Terminal<WideBackend<io::Stdout>>) -> Result<()> {
        while !self.should_quit {
            self.drain_worker();
            self.tick_auto_page();
            self.tick_cover_note();
            self.tick_cover_anim();
            if self.dirty {
                // 同步刷新：终端把这一帧攒齐再显示，滚动时整屏正文一起换，不会上半屏新下半屏旧。不支持的终端忽略这两个序列。
                let _ = execute!(io::stdout(), BeginSynchronizedUpdate);
                let drawn = self.draw_frame(terminal);
                let _ = execute!(io::stdout(), EndSynchronizedUpdate);
                drawn?;
                self.dirty = false;
                self.flush_cover_requests();
                self.sync_title();
            }
            if event::poll(self.poll_timeout())? {
                self.handle_event(event::read()?)?;
                // 按住 j 或触控板一划会在几毫秒内送来几十上百个事件。逐个事件画一帧，画面就会落后于手指，松手后还在滚。
                // 把已经到队列里的输入一次处理完再画，一次手势只画一帧；上限防止鼠标移动事件流把绘制饿死。
                let mut drained = 0;
                while drained < EVENT_BATCH && !self.should_quit && event::poll(Duration::ZERO)? {
                    self.handle_event(event::read()?)?;
                    drained += 1;
                }
            }
        }
        self.persist();
        // 清掉窗口标题，shell 下一个提示符会重新设置自己的。
        let _ = execute!(io::stdout(), SetTitle(""));
        Ok(())
    }

    /// 图片协议只在锚点格输出转义序列，其余格标成 Skip。浮层盖过图片后关闭或挪动，差分刷新不重写这些格，锚点格没变图片也不重发，浮层文字会留在图片上，所以这种情况清屏重画一次。
    fn draw_frame(&mut self, terminal: &mut Terminal<WideBackend<io::Stdout>>) -> Result<()> {
        terminal.draw(|f| crate::ui::draw(self, f))?;
        let shown = self.shown_overlay;
        if shown != self.overlay_area && self.image_rects.iter().any(|r| r.intersects(shown)) {
            terminal.clear()?;
            terminal.draw(|f| crate::ui::draw(self, f))?;
        }
        self.shown_overlay = self.overlay_area;
        Ok(())
    }

    /// 后台消息和定时器都靠循环醒来才处理：有任务在飞或有动画时保持短超时，空闲时拉长。
    /// 没标 busy 的零星任务（更新检查、字表刷新、进度拉取）最多晚一个空闲超时上屏，用户察觉不到。
    fn poll_timeout(&self) -> Duration {
        let auto_paging = self.screen() == Screen::Reader && self.overlay == Overlay::None && self.state.settings.auto_page_ms > 0;
        let animating = auto_paging || self.cover_note.is_some();
        let working = self.busy || self.hydrating || self.organizing || self.cover_inflight > 0 || !self.prefetching.is_empty();
        if animating || working { POLL_ACTIVE } else { POLL_IDLE }
    }

    /// 终端窗口标题跟着当前内容走，只在变化时发 OSC，避免每帧刷屏。
    fn sync_title(&mut self) {
        let title = self.window_title();
        if title != self.title_shown {
            let _ = execute!(io::stdout(), SetTitle(&title));
            self.title_shown = title;
        }
    }

    fn window_title(&self) -> String {
        if self.covered() {
            let index = if self.screen() == Screen::Reader { self.reader.as_ref().map(|r| r.index) } else { None };
            return crate::ui::cover_window_title(self.state.settings.layout, index);
        }
        match self.screen() {
            Screen::Reader => match self.reader.as_ref() {
                Some(r) if !r.body.item_id.is_empty() => format!("{} - {}", r.body.title, r.book.title),
                Some(r) => r.book.title.clone(),
                None => "tomato".into(),
            },
            Screen::Book => self.open.as_ref().map(|o| o.book.title.clone()).unwrap_or_else(|| "tomato".into()),
            Screen::Shelf => "tomato - 书架".into(),
            Screen::Search => "tomato - 搜索".into(),
            Screen::Rank => "tomato - 榜单".into(),
            Screen::Home | Screen::Login => "tomato".into(),
        }
    }

    pub(super) fn persist(&mut self) {
        self.state.cookies = self.client.cookies();
        let _ = store::save(&self.state);
    }

    pub(super) fn mark(&mut self) {
        self.dirty = true;
    }

    fn tick_auto_page(&mut self) {
        if self.screen() != Screen::Reader || self.overlay != Overlay::None {
            return;
        }
        let ms = self.state.settings.auto_page_ms;
        if ms == 0 {
            return;
        }
        if self.last_auto.elapsed() >= Duration::from_millis(ms) {
            self.reader_page(1);
            self.last_auto = std::time::Instant::now();
        }
    }

    fn handle_event(&mut self, ev: Event) -> Result<()> {
        match ev {
            Event::Resize(_, _) => self.mark(),
            Event::Paste(text) => {
                if self.overlay == Overlay::Cookie {
                    self.cookie_buf.push_str(&text);
                    self.mark();
                } else if self.screen() == Screen::Search {
                    self.search_input.push_str(&text);
                    self.mark();
                }
            }
            Event::Mouse(m) => self.handle_mouse(m),
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                let folded = fold_ime(key.code);
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(folded, KeyCode::Char(c) if c.eq_ignore_ascii_case(&'c'))
                {
                    self.should_quit = true;
                    return Ok(());
                }
                let code = if self.typing() { folded } else { ascii_letter_lower(folded) };
                self.handle_key(code, key.modifiers);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        // 老板键在任意屏幕生效；只有自由文本输入框（Cookie、文件夹名）里反引号保留原义。
        if code == KeyCode::Char('`') && !matches!(self.overlay, Overlay::Cookie | Overlay::FolderInput) {
            self.toggle_cover();
            return;
        }
        if self.covered() {
            self.cover_key(code);
            return;
        }
        if self.overlay != Overlay::None {
            self.overlay_key(code);
            return;
        }
        match self.screen() {
            Screen::Home => self.home_key(code),
            Screen::Shelf => self.shelf_key(code, mods),
            Screen::Search => self.search_key(code),
            Screen::Rank => self.rank_key(code),
            Screen::Book => self.book_key(code),
            Screen::Reader => self.reader_key(code),
            Screen::Login => self.login_key(code),
        }
    }

    fn overlay_key(&mut self, code: KeyCode) {
        match self.overlay {
            Overlay::Help => {
                self.overlay = Overlay::None;
                self.mark();
            }
            Overlay::Toc => self.toc_key(code),
            Overlay::Jump => self.jump_key(code),
            Overlay::FolderPick => self.folder_pick_key(code),
            Overlay::FolderInput => self.folder_input_key(code),
            Overlay::Cookie => self.cookie_key(code),
            Overlay::Settings => self.settings_key(code),
            Overlay::Profile => self.profile_key(code),
            Overlay::Organize => self.organize_key(code),
            Overlay::None => {}
        }
    }

    pub(super) fn move_list(state: &mut ListState, len: usize, delta: i32) {
        state.select(wrap_index(state.selected(), len, delta));
    }

    fn typing(&self) -> bool {
        matches!(self.overlay, Overlay::Cookie | Overlay::FolderInput | Overlay::Jump | Overlay::Toc) || (self.overlay == Overlay::None && self.screen() == Screen::Search)
    }
}

/// 列表选择按 delta 循环移动；空列表清空选择。
fn wrap_index(current: Option<usize>, len: usize, delta: i32) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let cur = current.unwrap_or(0) as i32;
    Some((cur + delta).rem_euclid(len as i32) as usize)
}

fn fold_ime(code: KeyCode) -> KeyCode {
    let KeyCode::Char(c) = code else {
        return code;
    };
    let mapped = if ('\u{FF01}'..='\u{FF5E}').contains(&c) {
        char::from_u32(u32::from(c) - 0xFEE0).unwrap_or(c)
    } else {
        match c {
            '【' | '「' => '[',
            '】' | '」' => ']',
            // 中文标点模式下反引号键输出间隔号，老板键也要能触发。
            '·' => '`',
            _ => c,
        }
    };
    KeyCode::Char(mapped)
}

fn ascii_letter_lower(code: KeyCode) -> KeyCode {
    match code {
        KeyCode::Char(c) if c.is_ascii_alphabetic() => KeyCode::Char(c.to_ascii_lowercase()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullwidth_help_and_search_map_to_ascii() {
        assert_eq!(fold_ime(KeyCode::Char('？')), KeyCode::Char('?'));
        assert_eq!(fold_ime(KeyCode::Char('／')), KeyCode::Char('/'));
        assert_eq!(fold_ime(KeyCode::Char('ｑ')), KeyCode::Char('q'));
        assert_eq!(fold_ime(KeyCode::Char('【')), KeyCode::Char('['));
        assert_eq!(fold_ime(KeyCode::Char('·')), KeyCode::Char('`'));
        assert_eq!(fold_ime(KeyCode::Char('｀')), KeyCode::Char('`'));
    }

    #[test]
    fn letter_shortcuts_accept_uppercase() {
        assert_eq!(ascii_letter_lower(KeyCode::Char('Q')), KeyCode::Char('q'));
        assert_eq!(ascii_letter_lower(KeyCode::Char('S')), KeyCode::Char('s'));
        assert_eq!(ascii_letter_lower(KeyCode::Char('?')), KeyCode::Char('?'));
    }
}
