use ratatui::crossterm::event::KeyCode;

use crate::model::LayoutId;
use crate::reflow::Reflow;
use crate::store;

use super::{filtered_chapters, App, Overlay, PROFILE_LEN, SETTINGS_LEN, Screen};

impl App {
    pub(super) fn book_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.back(),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('s') => self.open_settings(),
            KeyCode::Char('p') => self.open_profile(),
            KeyCode::Char('t') => self.open_toc(),
            KeyCode::Char('g') => {
                self.jump_buf.clear();
                self.overlay = Overlay::Jump;
                self.status.clear();
                self.mark();
            }
            KeyCode::Char('/') => {
                if let Some(open) = self.open.as_mut() {
                    open.filter.clear();
                }
                self.open_toc();
            }
            KeyCode::Char('a') => self.add_open_to_shelf(),
            KeyCode::Char('c') | KeyCode::Enter => self.read_selected_chapter(),
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(open) = self.open.as_mut() {
                    let n = filtered_chapters(&open.chapters, &open.filter).len();
                    Self::move_list(&mut open.list, n, 1);
                    self.mark();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(open) = self.open.as_mut() {
                    let n = filtered_chapters(&open.chapters, &open.filter).len();
                    Self::move_list(&mut open.list, n, -1);
                    self.mark();
                }
            }
            _ => {}
        }
    }

    /// 阅读页按键和其他屏幕保持同一套习惯：j/k 上下、h/l 或 [ ] 或左右方向键切「上一个 / 下一个」、p 资料、/ 目录过滤。
    pub(super) fn reader_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.back(),
            KeyCode::Char('q') => self.back(),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('s') => self.open_settings(),
            KeyCode::Char('p') => self.open_profile(),
            KeyCode::Char('t') | KeyCode::Char('/') => self.open_toc(),
            KeyCode::Char('g') => {
                self.jump_buf.clear();
                self.overlay = Overlay::Jump;
                self.mark();
            }
            KeyCode::Char('j') | KeyCode::Down => self.reader_scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.reader_scroll(-1),
            KeyCode::Char(' ') | KeyCode::PageDown => self.reader_page(1),
            KeyCode::Backspace | KeyCode::PageUp => self.reader_page(-1),
            KeyCode::Char('l') | KeyCode::Char(']') | KeyCode::Right | KeyCode::Tab => self.reader_chapter(1),
            KeyCode::Char('h') | KeyCode::Char('[') | KeyCode::Left | KeyCode::BackTab => self.reader_chapter(-1),
            KeyCode::Char('r') => self.reader_cycle_reflow(),
            KeyCode::Home => {
                if let Some(r) = self.reader.as_mut() {
                    r.offset = 0;
                    self.mark();
                }
            }
            KeyCode::End => {
                if let Some(r) = self.reader.as_mut() {
                    r.offset = usize::MAX;
                    self.mark();
                }
            }
            _ => {}
        }
    }

    /// 老板键。没选皮肤时只提示；否则翻转伪装态，关掉伪装画不出来的浮层（Toc / Jump 由伪装帧内联绘制，保留）。
    pub(super) fn toggle_cover(&mut self) {
        if self.state.settings.layout == LayoutId::Native {
            self.status = "先在设置里选择伪装布局".into();
            self.mark();
            return;
        }
        if !matches!(self.overlay, Overlay::None | Overlay::Toc | Overlay::Jump) {
            self.overlay = Overlay::None;
        }
        // 伪装态目录按序号前缀过滤、原生目录按标题子串过滤，同一个过滤词在两边含义不同，切换时清掉。
        if self.overlay == Overlay::Toc {
            self.toc_filter.clear();
        }
        self.cover = !self.cover;
        self.cover_note = None;
        self.mark();
    }

    /// 伪装态按键白名单。阅读页放行翻页、切章、目录、跳号、整理；其他屏幕全部吞掉，Esc / q / ? / s / p 都不响应。
    pub(super) fn cover_key(&mut self, code: KeyCode) {
        match self.overlay {
            Overlay::Toc => {
                if cover_toc_accepts(code) {
                    self.toc_key(code);
                }
                return;
            }
            Overlay::Jump => {
                self.jump_key(code);
                return;
            }
            Overlay::None => {}
            _ => {
                self.overlay = Overlay::None;
                self.mark();
                return;
            }
        }
        if self.screen() != Screen::Reader {
            return;
        }
        match code {
            KeyCode::Char('j') | KeyCode::Down => self.reader_scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.reader_scroll(-1),
            KeyCode::Char(' ') | KeyCode::PageDown => self.reader_page(1),
            KeyCode::Backspace | KeyCode::PageUp => self.reader_page(-1),
            KeyCode::Char('l') | KeyCode::Char(']') | KeyCode::Right | KeyCode::Tab => self.reader_chapter(1),
            KeyCode::Char('h') | KeyCode::Char('[') | KeyCode::Left | KeyCode::BackTab => self.reader_chapter(-1),
            KeyCode::Char('r') => self.reader_cycle_reflow(),
            KeyCode::Home | KeyCode::End | KeyCode::Char('t') | KeyCode::Char('g') => self.reader_key(code),
            _ => {}
        }
    }

    pub(super) fn settings_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.persist();
                self.back();
            }
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('j') | KeyCode::Down => {
                self.settings_idx = (self.settings_idx + 1) % SETTINGS_LEN;
                self.settings_list.select(Some(self.settings_idx));
                self.mark();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.settings_idx = (self.settings_idx + SETTINGS_LEN - 1) % SETTINGS_LEN;
                self.settings_list.select(Some(self.settings_idx));
                self.mark();
            }
            KeyCode::Char('h') | KeyCode::Left => self.nudge_setting(-1),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => self.nudge_setting(1),
            _ => {}
        }
    }

    pub(super) fn profile_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.back(),
            KeyCode::Char('l') => {
                self.profile_list.select(Some(0));
                self.spawn_login();
            }
            KeyCode::Char('o') => {
                self.profile_list.select(Some(1));
                self.logout();
            }
            KeyCode::Char('u') => {
                self.profile_list.select(Some(2));
                self.spawn_update_check();
                self.status = "正在检查更新…".into();
                self.mark();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                Self::move_list(&mut self.profile_list, PROFILE_LEN, 1);
                self.mark();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                Self::move_list(&mut self.profile_list, PROFILE_LEN, -1);
                self.mark();
            }
            KeyCode::Enter => self.activate_profile(),
            _ => {}
        }
    }

    pub(super) fn toc_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.mark();
            }
            KeyCode::Enter => {
                if let Some(idx) = self.toc_selected() {
                    self.overlay = Overlay::None;
                    self.jump_to_index(idx);
                }
            }
            KeyCode::Backspace => {
                self.toc_filter.pop();
                self.mark();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let n = self.toc_items().len();
                Self::move_list(&mut self.toc_list, n, 1);
                self.mark();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let n = self.toc_items().len();
                Self::move_list(&mut self.toc_list, n, -1);
                self.mark();
            }
            KeyCode::Char(c) if !c.is_control() => {
                self.toc_filter.push(c);
                self.mark();
            }
            _ => {}
        }
    }

    pub(super) fn jump_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.mark();
            }
            KeyCode::Backspace => {
                self.jump_buf.pop();
                self.mark();
            }
            KeyCode::Enter => {
                if let Ok(n) = self.jump_buf.parse::<usize>()
                    && n > 0
                {
                    self.overlay = Overlay::None;
                    self.jump_to_index(n - 1);
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                self.jump_buf.push(c);
                self.mark();
            }
            _ => {}
        }
    }

    pub(super) fn folder_pick_key(&mut self, code: KeyCode) {
        let folders = store::all_folders(&self.state);
        match code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.mark();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !folders.is_empty() {
                    self.folder_pick_idx = (self.folder_pick_idx + 1) % folders.len();
                    self.mark();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !folders.is_empty() {
                    self.folder_pick_idx = (self.folder_pick_idx + folders.len() - 1) % folders.len();
                    self.mark();
                }
            }
            KeyCode::Enter => {
                if let Some(folder) = folders.get(self.folder_pick_idx).cloned() {
                    self.move_selected_to(&folder);
                }
            }
            _ => {}
        }
    }

    pub(super) fn organize_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.cancel_organize(),
            KeyCode::Enter => self.apply_organize(),
            KeyCode::Char('j') | KeyCode::Down => {
                Self::move_list(&mut self.organize_list, self.organize_plan.len(), 1);
                self.mark();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                Self::move_list(&mut self.organize_list, self.organize_plan.len(), -1);
                self.mark();
            }
            _ => {}
        }
    }

    pub(super) fn folder_input_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.mark();
            }
            KeyCode::Backspace => {
                self.folder_buf.pop();
                self.mark();
            }
            KeyCode::Enter => {
                let name = self.folder_buf.trim().to_string();
                if !name.is_empty() {
                    if self.folder_rename {
                        if let Some(from) = self.current_folder() {
                            let moved = store::rename_folder(&mut self.state, &from, &name);
                            self.push_groups(moved.into_iter().map(|id| (id, name.clone())).collect());
                        }
                    } else {
                        store::ensure_folder(&mut self.state, &name);
                    }
                    self.persist();
                    self.status = format!("文件夹 {name}");
                }
                self.overlay = Overlay::None;
                self.mark();
            }
            KeyCode::Char(c) if !c.is_control() => {
                self.folder_buf.push(c);
                self.mark();
            }
            _ => {}
        }
    }

    pub(super) fn nudge_setting(&mut self, dir: i32) {
        match self.settings_idx {
            0 => self.state.settings.theme = if dir >= 0 { self.state.settings.theme.next() } else { self.state.settings.theme.prev() },
            1 => {
                self.state.settings.layout = if dir >= 0 { self.state.settings.layout.next() } else { self.state.settings.layout.prev() };
                if self.state.settings.layout == LayoutId::Native {
                    self.cover = false;
                }
            }
            2 => self.state.settings.cycle_width(dir),
            3 => self.state.settings.cycle_margin(dir),
            4 => self.state.settings.cycle_gap(dir),
            5 => self.state.settings.cycle_reflow(dir),
            6 => self.state.settings.para_indent = !self.state.settings.para_indent,
            7 => self.state.settings.para_blank = !self.state.settings.para_blank,
            8 => self.state.settings.cycle_auto(dir),
            9 => self.state.settings.check_update = !self.state.settings.check_update,
            10 => self.state.settings.sort = self.state.settings.sort.next(),
            11 => self.state.settings.auto_organize = !self.state.settings.auto_organize,
            12 => self.state.settings.cycle_abandon(dir),
            13 => {
                self.state.settings.show_covers = !self.state.settings.show_covers;
                if self.state.settings.show_covers {
                    if let Some((id, url)) = self.open.as_ref().map(|o| (o.book.book_id.clone(), o.book.thumb_url.clone())) {
                        self.spawn_cover(&id, &url);
                    }
                } else {
                    self.covers.clear();
                    self.cover_requested.clear();
                    self.cover_failed.clear();
                }
            }
            14 => {
                self.state.settings.cycle_prefetch(dir);
                self.schedule_prefetch();
            }
            15 => {
                self.spawn_fontmap_regenerate();
                return;
            }
            _ => {}
        }
        // 折行相关的设置都在 WrapOpts 里，下一帧 prepare_wrap 比较参数后自己重折并按正文位置回到原处；这里不强制重折，主题、排序这类改动就不用整章重算。
        self.persist();
        self.mark();
    }

    pub(super) fn open_help(&mut self) {
        self.overlay = Overlay::Help;
        self.mark();
    }

    pub(super) fn open_settings(&mut self) {
        if self.settings_idx >= SETTINGS_LEN {
            self.settings_idx = 0;
        }
        self.settings_list.select(Some(self.settings_idx));
        self.overlay = Overlay::Settings;
        self.status.clear();
        self.mark();
    }

    pub(super) fn open_profile(&mut self) {
        self.overlay = Overlay::Profile;
        self.status.clear();
        self.mark();
    }

    pub(super) fn open_toc(&mut self) {
        self.toc_filter.clear();
        let idx = self.reader.as_ref().map(|r| r.index).or_else(|| self.open.as_ref().and_then(|o| o.list.selected())).unwrap_or(0);
        self.toc_list.select(Some(idx));
        self.overlay = Overlay::Toc;
        self.status.clear();
        self.mark();
    }

    /// 「换行整理」的设置行值。Auto 时带上本章实际落到的模式，读者不用猜自动判定的结果。
    fn reflow_label(&self) -> String {
        let reflow = self.state.settings.reflow;
        if reflow != Reflow::Auto {
            return reflow.name().into();
        }
        match self.reader.as_ref().and_then(|r| r.cache.resolved()) {
            Some(Reflow::Merge) => "自动（本章 已合并）".into(),
            Some(_) => "自动（本章 原样）".into(),
            None => "自动".into(),
        }
    }

    pub(crate) fn settings_pairs(&self) -> Vec<(&'static str, String)> {
        let s = &self.state.settings;
        vec![
            ("主题", s.theme.name()),
            ("伪装布局", s.layout.name().into()),
            ("正文宽度", s.width_label()),
            ("边距", s.margin.to_string()),
            ("行距", s.line_gap.to_string()),
            ("换行整理", self.reflow_label()),
            ("段首缩进", on_off(s.para_indent)),
            ("段间空行", on_off(s.para_blank)),
            ("自动翻页", s.auto_label()),
            ("启动检查更新", on_off(s.check_update)),
            ("书架排序", s.sort.name().to_string()),
            ("自动整理", on_off(s.auto_organize)),
            ("弃读判定", format!("{} 天", s.abandon_days)),
            ("书籍封面", on_off(s.show_covers)),
            ("提前缓存", s.prefetch_label()),
            ("字表", "重新生成".into()),
        ]
    }
}

fn on_off(v: bool) -> String {
    if v { "开".into() } else { "关".into() }
}

/// 伪装态目录浮层放行的键。伪装目录只按序号前缀过滤（见 `toc_items`），字母和符号进不了过滤词，q / ? / s 这类在伪装态被吞的键也不会在揭开后变成原生目录的过滤词。
fn cover_toc_accepts(code: KeyCode) -> bool {
    match code {
        KeyCode::Char(c) => c.is_ascii_digit() || matches!(c, 'j' | 'k'),
        KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace | KeyCode::Up | KeyCode::Down => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_toc_only_accepts_digits_and_navigation() {
        for c in ['0', '7', 'j', 'k'] {
            assert!(cover_toc_accepts(KeyCode::Char(c)), "{c}");
        }
        for c in ['q', '?', 's', 'p', 't', '/', ' ', 'a', '章'] {
            assert!(!cover_toc_accepts(KeyCode::Char(c)), "{c}");
        }
        for code in [KeyCode::Esc, KeyCode::Enter, KeyCode::Backspace, KeyCode::Up, KeyCode::Down] {
            assert!(cover_toc_accepts(code));
        }
        assert!(!cover_toc_accepts(KeyCode::Tab));
        assert!(!cover_toc_accepts(KeyCode::Home));
    }
}
