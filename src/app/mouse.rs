use std::time::{Duration, Instant};

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use crate::store;

use super::{App, HOME_LEN, LOGIN_LEN, Overlay, PROFILE_LEN, SETTINGS_LEN, Screen, card_index_at, click_hits, filtered_chapters, list_index_at, point_in};

impl App {
    pub(super) fn handle_mouse(&mut self, m: MouseEvent) {
        // 伪装态：非阅读屏幕全部吞掉；阅读页右键原本是返回，只能用来关内联目录 / 跳号。
        if self.covered() {
            if self.screen() != Screen::Reader {
                return;
            }
            if matches!(m.kind, MouseEventKind::Down(MouseButton::Right)) {
                if matches!(self.overlay, Overlay::Toc | Overlay::Jump) {
                    self.overlay = Overlay::None;
                    self.mark();
                }
                return;
            }
        }
        match m.kind {
            MouseEventKind::ScrollUp => self.mouse_scroll(-1),
            MouseEventKind::ScrollDown => self.mouse_scroll(1),
            MouseEventKind::Down(MouseButton::Left) => self.mouse_click(m.column, m.row),
            MouseEventKind::Down(MouseButton::Right) => self.mouse_right(),
            MouseEventKind::Down(MouseButton::Middle) if self.screen() == Screen::Reader && self.overlay == Overlay::None => self.reader_page(1),
            _ => {}
        }
    }

    fn mouse_right(&mut self) {
        if self.overlay == Overlay::Organize {
            self.cancel_organize();
        } else if self.overlay != Overlay::None {
            self.overlay = Overlay::None;
            self.mark();
        } else {
            self.back();
        }
    }

    fn mouse_scroll(&mut self, dir: i32) {
        match self.overlay {
            Overlay::Organize => {
                Self::move_list(&mut self.organize_list, self.organize_plan.len(), dir);
                self.mark();
                return;
            }
            Overlay::Toc => {
                let n = self.toc_items().len();
                Self::move_list(&mut self.toc_list, n, dir);
                self.mark();
                return;
            }
            Overlay::FolderPick => {
                let n = store::all_folders(&self.state).len();
                if n > 0 {
                    self.folder_pick_idx = (self.folder_pick_idx as i32 + dir).rem_euclid(n as i32) as usize;
                    self.mark();
                }
                return;
            }
            Overlay::Settings => {
                self.settings_idx = (self.settings_idx as i32 + dir).rem_euclid(SETTINGS_LEN as i32) as usize;
                self.settings_list.select(Some(self.settings_idx));
                self.mark();
                return;
            }
            Overlay::Profile => {
                Self::move_list(&mut self.profile_list, PROFILE_LEN, dir);
                self.mark();
                return;
            }
            Overlay::Help | Overlay::Jump | Overlay::FolderInput | Overlay::Cookie => return,
            Overlay::None => {}
        }
        match self.screen() {
            Screen::Home => {
                Self::move_list(&mut self.home_list, HOME_LEN, dir);
                self.mark();
            }
            Screen::Shelf => {
                let n = self.visible_shelf().len();
                Self::move_list(&mut self.shelf_list, n, dir);
                self.mark();
            }
            // 向下与 j / Down 同一入口：停在最后一条再滚就加载下一页。
            Screen::Search if dir > 0 => self.search_down(),
            Screen::Search => {
                Self::move_list(&mut self.search_list, self.search_results.len(), dir);
                self.mark();
            }
            Screen::Rank => self.rank_move(dir),
            Screen::Book => {
                if let Some(open) = self.open.as_mut() {
                    let n = filtered_chapters(&open.chapters, &open.filter).len();
                    Self::move_list(&mut open.list, n, dir);
                    self.mark();
                }
            }
            Screen::Reader => self.reader_scroll(dir),
            Screen::Login => {
                Self::move_list(&mut self.login_list, LOGIN_LEN, dir);
                self.mark();
            }
        }
    }

    fn mouse_click(&mut self, col: u16, row: u16) {
        if self.overlay == Overlay::Help {
            self.overlay = Overlay::None;
            self.mark();
            return;
        }
        if self.overlay != Overlay::None {
            if self.overlay_area.width > 0 && !point_in(self.overlay_area, col, row) {
                if self.overlay == Overlay::Organize {
                    self.cancel_organize();
                } else {
                    self.overlay = Overlay::None;
                    self.mark();
                }
                return;
            }
            match self.overlay {
                Overlay::Toc => {
                    let n = self.toc_items().len();
                    if let Some(i) = list_index_at(self.list_area, col, row, n, self.toc_list.offset()) {
                        self.toc_list.select(Some(i));
                        if let Some(idx) = self.toc_items().get(i).map(|(idx, _)| *idx) {
                            self.overlay = Overlay::None;
                            self.jump_to_index(idx);
                        }
                    }
                }
                Overlay::FolderPick => self.click_folder_pick(col, row),
                Overlay::Organize => {
                    if let Some(i) = list_index_at(self.list_area, col, row, self.organize_plan.len(), self.organize_list.offset()) {
                        self.organize_list.select(Some(i));
                        self.mark();
                    }
                }
                Overlay::Settings => {
                    if let Some(i) = list_index_at(self.list_area, col, row, SETTINGS_LEN, self.settings_list.offset()) {
                        self.settings_idx = i;
                        self.settings_list.select(Some(i));
                        let dir = if col < self.list_area.x.saturating_add(self.list_area.width / 2) { -1 } else { 1 };
                        self.nudge_setting(dir);
                    }
                }
                Overlay::Profile => {
                    if let Some(i) = list_index_at(self.list_area, col, row, PROFILE_LEN, self.profile_list.offset()) {
                        self.profile_list.select(Some(i));
                        self.activate_profile();
                    }
                }
                _ => {}
            }
            return;
        }
        if point_in(self.help_hit, col, row) {
            self.open_help();
            return;
        }
        if point_in(self.profile_hit, col, row) {
            self.open_profile();
            return;
        }
        for (rect, idx) in self.folder_tabs.iter().copied() {
            if point_in(rect, col, row) {
                self.folder_idx = idx;
                self.sync_shelf_select();
                self.mark();
                return;
            }
        }
        match self.screen() {
            Screen::Home => {
                if let Some(i) = click_hits(&self.menu_hits, col, row) {
                    self.home_list.select(Some(i));
                    self.activate_home();
                }
            }
            Screen::Shelf => {
                let n = self.visible_shelf().len();
                if let Some(i) = card_index_at(self.list_area, col, row, n, self.shelf_list.offset(), self.card_stride()) {
                    self.shelf_list.select(Some(i));
                    if self.is_double_click(col, row, i) {
                        self.open_selected_shelf(true);
                    } else {
                        self.mark();
                    }
                }
            }
            Screen::Search => {
                let n = self.search_results.len();
                if let Some(i) = card_index_at(self.list_area, col, row, n, self.search_list.offset(), self.card_stride()) {
                    self.search_list.select(Some(i));
                    if self.is_double_click(col, row, i) {
                        self.open_selected_search();
                    } else {
                        self.mark();
                    }
                }
            }
            Screen::Rank => {
                let n = self.rank_books.len();
                if let Some(i) = card_index_at(self.list_area, col, row, n, self.rank_list.offset(), self.card_stride()) {
                    self.rank_list.select(Some(i));
                    if self.is_double_click(col, row, i) {
                        self.open_selected_rank();
                    } else {
                        self.mark();
                    }
                }
            }
            Screen::Book => {
                let n = self.open.as_ref().map(|o| filtered_chapters(&o.chapters, &o.filter).len()).unwrap_or(0);
                let offset = self.open.as_ref().map(|o| o.list.offset()).unwrap_or(0);
                if let Some(i) = list_index_at(self.list_area, col, row, n, offset) {
                    if let Some(open) = self.open.as_mut() {
                        open.list.select(Some(i));
                    }
                    if self.is_double_click(col, row, i) {
                        self.read_selected_chapter();
                    } else {
                        self.mark();
                    }
                }
            }
            Screen::Reader => {
                let area = self.list_area;
                if point_in(area, col, row) {
                    if row == area.y {
                        self.open_toc();
                    } else if col < area.x.saturating_add(area.width / 3) {
                        self.reader_page(-1);
                    } else if col > area.x.saturating_add(area.width * 2 / 3) {
                        self.reader_page(1);
                    }
                }
            }
            Screen::Login => {
                if let Some(i) = click_hits(&self.menu_hits, col, row) {
                    self.login_list.select(Some(i));
                    match i {
                        0 | 2 => self.spawn_login(),
                        1 => self.open_cookie(),
                        _ => {}
                    }
                }
            }
        }
    }

    fn click_folder_pick(&mut self, col: u16, row: u16) {
        let folders = store::all_folders(&self.state);
        if let Some(i) = list_index_at(self.list_area, col, row, folders.len(), 0) {
            self.folder_pick_idx = i;
            if let Some(folder) = folders.get(i).cloned() {
                self.move_selected_to(&folder);
            }
        }
    }

    fn is_double_click(&mut self, col: u16, row: u16, idx: usize) -> bool {
        let now = Instant::now();
        let dbl = self.last_click.map(|(c, r, t, i)| i == idx && c.abs_diff(col) < 3 && r.abs_diff(row) < 2 && now.duration_since(t) < Duration::from_millis(400)).unwrap_or(false);
        self.last_click = Some((col, row, now, idx));
        dbl
    }
}
