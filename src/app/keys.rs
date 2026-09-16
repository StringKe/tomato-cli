use ratatui::crossterm::event::{KeyCode, KeyModifiers};

use crate::model::UNGROUPED;
use crate::store;

use super::workers::WorkerMsg;
use super::{App, Overlay, Screen, HOME_LEN, LOGIN_LEN};

impl App {
    pub(super) fn home_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Enter => self.activate_home(),
            KeyCode::Char('l') => self.spawn_login(),
            KeyCode::Char('c') => self.open_cookie(),
            KeyCode::Char('/') => self.push(Screen::Search),
            KeyCode::Char('b') => self.open_rank(),
            KeyCode::Char('d') => self.open_demo(),
            KeyCode::Char('s') => self.open_settings(),
            KeyCode::Char('p') => self.open_profile(),
            KeyCode::Char('j') | KeyCode::Down => {
                Self::move_list(&mut self.home_list, HOME_LEN, 1);
                self.mark();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                Self::move_list(&mut self.home_list, HOME_LEN, -1);
                self.mark();
            }
            _ => {}
        }
    }

    pub(super) fn login_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.back(),
            KeyCode::Char('1') | KeyCode::Char('l') => {
                self.login_list.select(Some(0));
                self.spawn_login();
            }
            KeyCode::Char('2') | KeyCode::Char('c') => {
                self.login_list.select(Some(1));
                self.open_cookie();
            }
            KeyCode::Char('3') | KeyCode::Char('r') => {
                self.login_list.select(Some(2));
                self.spawn_login();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                Self::move_list(&mut self.login_list, LOGIN_LEN, 1);
                self.mark();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                Self::move_list(&mut self.login_list, LOGIN_LEN, -1);
                self.mark();
            }
            KeyCode::Enter => match self.login_list.selected().unwrap_or(0) {
                0 | 2 => self.spawn_login(),
                1 => self.open_cookie(),
                _ => {}
            },
            _ => {}
        }
    }

    pub(super) fn cookie_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.mark();
            }
            KeyCode::Backspace => {
                self.cookie_buf.pop();
                self.mark();
            }
            KeyCode::Enter => self.submit_cookie(),
            KeyCode::Char(c) if !c.is_control() => {
                self.cookie_buf.push(c);
                self.mark();
            }
            _ => {}
        }
    }

    pub(super) fn open_cookie(&mut self) {
        self.cookie_buf.clear();
        self.overlay = Overlay::Cookie;
        self.status.clear();
        self.mark();
    }

    fn submit_cookie(&mut self) {
        let raw = self.cookie_buf.trim().to_string();
        if raw.is_empty() {
            self.status = "Cookie 为空".into();
            self.mark();
            return;
        }
        self.overlay = Overlay::None;
        self.busy = true;
        self.status = "正在用 Cookie 登录…".into();
        self.mark();
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = client.login_with_cookie(&raw).await.map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::LoginDone(result));
        });
    }

    pub(super) fn shelf_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        match code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => {}
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('/') => self.push(Screen::Search),
            KeyCode::Char('b') => self.open_rank(),
            KeyCode::Char('l') => self.spawn_login(),
            KeyCode::Char('p') => self.open_profile(),
            KeyCode::Char('s') => self.open_settings(),
            KeyCode::Char('r') => self.spawn_shelf_refresh(),
            KeyCode::Char('d') if mods.contains(KeyModifiers::SHIFT) => {
                let folders = store::all_folders(&self.state);
                if let Some(name) = folders.get(self.folder_idx)
                    && name != UNGROUPED
                {
                    let name = name.clone();
                    store::delete_folder(&mut self.state, &name);
                    self.folder_idx = 0;
                    self.sync_shelf_select();
                    self.persist();
                    self.status = format!("已删除文件夹 {name}");
                    self.mark();
                }
            }
            KeyCode::Char('d') => self.open_demo(),
            KeyCode::Char('o') => {
                self.state.settings.sort = self.state.settings.sort.next();
                self.status = format!("排序：{}", self.state.settings.sort.name());
                self.sync_shelf_select();
                self.mark();
            }
            KeyCode::Char('n') => {
                self.folder_rename = false;
                self.folder_buf.clear();
                self.overlay = Overlay::FolderInput;
                self.status.clear();
                self.mark();
            }
            KeyCode::Char('e') => {
                let folders = store::all_folders(&self.state);
                if let Some(name) = folders.get(self.folder_idx)
                    && name != UNGROUPED
                {
                    self.folder_rename = true;
                    self.folder_buf.clone_from(name);
                    self.overlay = Overlay::FolderInput;
                    self.status.clear();
                    self.mark();
                }
            }
            KeyCode::Char('m') => {
                if self.selected_shelf_item().is_some() {
                    self.overlay = Overlay::FolderPick;
                    self.status.clear();
                    self.mark();
                }
            }
            KeyCode::Char('x') => self.remove_selected_shelf(),
            KeyCode::Char('[') | KeyCode::BackTab => {
                let n = store::all_folders(&self.state).len().max(1);
                self.folder_idx = (self.folder_idx + n - 1) % n;
                self.sync_shelf_select();
                self.mark();
            }
            KeyCode::Char(']') | KeyCode::Tab => {
                let n = store::all_folders(&self.state).len().max(1);
                self.folder_idx = (self.folder_idx + 1) % n;
                self.sync_shelf_select();
                self.mark();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let n = self.visible_shelf().len();
                Self::move_list(&mut self.shelf_list, n, 1);
                self.mark();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let n = self.visible_shelf().len();
                Self::move_list(&mut self.shelf_list, n, -1);
                self.mark();
            }
            KeyCode::Enter | KeyCode::Char('c') => self.open_selected_shelf(true),
            KeyCode::Char('i') => self.open_selected_shelf(false),
            _ => {}
        }
    }

    pub(super) fn search_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.back(),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Backspace => {
                self.search_input.pop();
                self.mark();
            }
            KeyCode::Enter => {
                if !self.search_results.is_empty() && self.search_list.selected().is_some() {
                    self.open_selected_search();
                } else if !self.search_input.is_empty() {
                    self.spawn_search();
                }
            }
            KeyCode::Down => self.search_down(),
            KeyCode::Up => {
                Self::move_list(&mut self.search_list, self.search_results.len(), -1);
                self.mark();
            }
            KeyCode::Char('j') if self.search_input.is_empty() => self.search_down(),
            KeyCode::Char('k') if self.search_input.is_empty() => {
                Self::move_list(&mut self.search_list, self.search_results.len(), -1);
                self.mark();
            }
            KeyCode::Char(c) if !c.is_control() => {
                self.search_input.push(c);
                self.mark();
            }
            _ => {}
        }
    }
}
