//! 榜单屏：不登录也能按分类发现书。分类来自 /rank 页面，书目按页拉取，选中项停在末尾再向下时续页。

use ratatui::crossterm::event::KeyCode;

use crate::model::{Book, GENDER_FEMALE, GENDER_MALE, RankCategory};

use super::{App, Screen};

impl App {
    pub(super) fn rank_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.back(),
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('s') => self.open_settings(),
            KeyCode::Char('p') => self.open_profile(),
            KeyCode::Char('[') | KeyCode::Left => self.rank_switch_category(-1),
            KeyCode::Char(']') | KeyCode::Right => self.rank_switch_category(1),
            KeyCode::Tab | KeyCode::BackTab => self.rank_switch_gender(),
            KeyCode::Char('j') | KeyCode::Down => self.rank_move(1),
            KeyCode::Char('k') | KeyCode::Up => self.rank_move(-1),
            KeyCode::Char('r') => self.rank_reload(),
            KeyCode::Enter => self.open_selected_rank(),
            _ => {}
        }
    }

    /// 进入榜单。分类只拉一次；已有书目时直接展示，不重复请求。
    pub(super) fn open_rank(&mut self) {
        self.push(Screen::Rank);
        if self.rank_cats.is_empty() {
            self.spawn_rank_categories();
        } else if self.rank_books.is_empty() && !self.busy {
            self.spawn_rank_page(0);
        }
    }

    /// 当前性别下的分类列表。
    pub(crate) fn rank_categories_of_gender(&self) -> Vec<&RankCategory> {
        self.rank_cats.iter().filter(|c| c.gender == self.rank_gender).collect()
    }

    pub(crate) fn rank_category(&self) -> Option<&RankCategory> {
        self.rank_categories_of_gender().get(self.rank_cat_idx).copied()
    }

    fn rank_switch_category(&mut self, dir: i32) {
        let n = self.rank_categories_of_gender().len();
        if n == 0 {
            return;
        }
        self.rank_cat_idx = (self.rank_cat_idx as i32 + dir).rem_euclid(n as i32) as usize;
        self.rank_reset_and_load();
    }

    fn rank_switch_gender(&mut self) {
        self.rank_gender = if self.rank_gender == GENDER_MALE { GENDER_FEMALE } else { GENDER_MALE };
        self.rank_cat_idx = 0;
        self.rank_reset_and_load();
    }

    fn rank_reset_and_load(&mut self) {
        self.rank_books.clear();
        self.rank_total = 0;
        self.rank_list = ratatui::widgets::ListState::default();
        self.spawn_rank_page(0);
    }

    fn rank_reload(&mut self) {
        if self.rank_cats.is_empty() {
            self.spawn_rank_categories();
        } else {
            self.rank_reset_and_load();
        }
    }

    /// 上下移动，到头停住不环绕；已停在最后一条再向下且榜单还没拉完时加载下一页。
    pub(super) fn rank_move(&mut self, dir: i32) {
        let n = self.rank_books.len();
        let Some(next) = clamp_index(self.rank_list.selected(), n, dir) else { return };
        if next == self.rank_list.selected().unwrap_or(0) && dir > 0 && (n as u32) < self.rank_total && !self.busy {
            self.spawn_rank_page(n as u32);
            return;
        }
        self.rank_list.select(Some(next));
        self.mark();
    }

    pub(super) fn open_selected_rank(&mut self) {
        let Some(idx) = self.rank_list.selected() else { return };
        let Some(book) = self.rank_books.get(idx).cloned() else { return };
        self.spawn_open(book);
    }

    /// 一页榜单到达。请求时的分类、性别或 offset 与当前状态不符说明用户已切换或刷新，直接丢弃。
    pub(super) fn apply_rank_page(&mut self, category_id: &str, gender: u8, offset: u32, result: Result<(Vec<Book>, u32), String>) {
        let current = self.rank_category().filter(|c| c.id == category_id && c.gender == gender).is_some();
        if !current || offset != self.rank_books.len() as u32 {
            return;
        }
        match result {
            Ok((books, total)) => {
                let added = books.len();
                self.rank_books.extend(books);
                self.rank_total = total.max(self.rank_books.len() as u32);
                if self.rank_list.selected().is_none() && !self.rank_books.is_empty() {
                    self.rank_list.select(Some(0));
                }
                self.status = if offset == 0 { format!("榜单 {} 本", self.rank_total) } else { format!("再加载 {added} 本，已到第 {} 名", self.rank_books.len()) };
            }
            Err(e) => self.status = e,
        }
    }
}

/// 榜单列表按 delta 移动并夹在两端，不像其他列表那样环绕；空列表返回 None。
fn clamp_index(current: Option<usize>, len: usize, delta: i32) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let cur = current.unwrap_or(0) as i32;
    Some((cur + delta).clamp(0, len as i32 - 1) as usize)
}

#[cfg(test)]
mod tests {
    use super::clamp_index;

    #[test]
    fn clamps_at_both_ends() {
        assert_eq!(clamp_index(None, 0, 1), None);
        assert_eq!(clamp_index(None, 5, -1), Some(0));
        assert_eq!(clamp_index(Some(0), 5, -1), Some(0));
        assert_eq!(clamp_index(Some(4), 5, 1), Some(4));
        assert_eq!(clamp_index(Some(2), 5, 1), Some(3));
        assert_eq!(clamp_index(Some(9), 5, 1), Some(4));
    }
}
