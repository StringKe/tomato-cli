use crate::model::{Chapter, ChapterBody, LayoutId, Progress, ShelfItem, SortMode, UNGROUPED};
use crate::reader::{demo_book, WrapCache};
use crate::reflow::Reflow;
use crate::store;

use super::{filtered_chapters, App, OpenBook, Overlay, ReaderSession, Screen};

/// 到底后两次翻页之间至少要隔这么久才算「再按一次」，短于它视为连按。
const PAGE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(600);
/// 伪装态状态行上一次性提示的停留时间。
const COVER_NOTE_TTL: std::time::Duration = std::time::Duration::from_millis(2500);
/// 伪装态 spinner 的换帧间隔。
const ANIM_TICK: std::time::Duration = std::time::Duration::from_millis(120);

impl App {
    pub(super) fn activate_home(&mut self) {
        match self.home_list.selected().unwrap_or(0) {
            0 => self.spawn_login(),
            1 => self.open_cookie(),
            2 => self.push(Screen::Search),
            3 => self.open_rank(),
            4 => self.open_demo(),
            5 => self.open_settings(),
            6 => self.should_quit = true,
            _ => {}
        }
    }

    pub(super) fn activate_profile(&mut self) {
        match self.profile_list.selected().unwrap_or(0) {
            0 => self.spawn_login(),
            1 => self.logout(),
            2 => {
                self.spawn_update_check();
                self.status = "正在检查更新…".into();
                self.mark();
            }
            _ => {}
        }
    }

    pub(super) fn apply_chapter(&mut self, body: ChapterBody) {
        {
            let Some(r) = self.reader.as_mut() else { return };
            if let Some(idx) = r.chapters.iter().position(|c| c.item_id == body.item_id) {
                r.index = idx;
                // 目录页的选中项跟着当前章走，退回目录时光标就在这一章。
                if let Some(open) = self.open.as_mut() {
                    open.list.select(Some(idx));
                }
            }
            let (line, anchor) = self.state.progress.get(&r.book.book_id).filter(|p| p.item_id == body.item_id).map(|p| (p.line, p.anchor)).unwrap_or((0, None));
            r.body = body;
            r.cache.set_text(r.body.content.clone());
            r.offset = line;
            r.pending_anchor = anchor;
        }
        if self.screens.last() != Some(&Screen::Reader) {
            self.push(Screen::Reader);
            // 进入阅读页第一帧即伪装；切章时已在阅读页，用户揭开后保持揭开。
            self.cover = self.state.settings.layout != LayoutId::Native;
        }
        self.last_auto = std::time::Instant::now();
        self.status.clear();
        // 预读失败的章节每次切章允许再试一次，既不无限重试也不会一直缺着。
        self.prefetch_failed.clear();
        self.schedule_prefetch();
    }

    pub(super) fn merge_remote_shelf(&mut self, items: Vec<ShelfItem>) {
        let local = std::mem::take(&mut self.state.shelf);
        let mut merged = items;
        for remote in &mut merged {
            if let Some(old) = local.iter().find(|b| b.book_id == remote.book_id) {
                remote.fill_missing_from(old);
            }
        }
        for old in local {
            if !merged.iter().any(|b| b.book_id == old.book_id) {
                merged.push(old);
            }
        }
        self.state.shelf = merged;
    }

    fn chapters(&self) -> &[Chapter] {
        if let Some(r) = self.reader.as_ref() {
            return &r.chapters;
        }
        if let Some(o) = self.open.as_ref() {
            return &o.chapters;
        }
        &[]
    }

    pub(crate) fn toc_items(&self) -> Vec<(usize, &Chapter)> {
        if self.covered() {
            // 伪装态目录只显示序号文件名，标题不可见，过滤按序号前缀才有意义。
            return self.chapters().iter().enumerate().filter(|(i, _)| self.toc_filter.is_empty() || (i + 1).to_string().starts_with(&self.toc_filter)).collect();
        }
        filtered_chapters(self.chapters(), &self.toc_filter)
    }

    pub(super) fn toc_selected(&self) -> Option<usize> {
        let i = self.toc_list.selected()?;
        self.toc_items().get(i).map(|(idx, _)| *idx)
    }

    pub(super) fn jump_to_index(&mut self, index: usize) {
        let Some(item_id) = self.chapters().get(index).map(|c| c.item_id.clone()) else {
            self.status = "章节不存在".into();
            self.mark();
            return;
        };
        if let Some(open) = self.open.as_mut() {
            open.list.select(Some(index));
        }
        self.save_reader_progress();
        self.ensure_reader_shell();
        self.spawn_chapter(item_id);
    }

    pub(super) fn read_selected_chapter(&mut self) {
        let idx = {
            let Some(open) = self.open.as_ref() else { return };
            let vis = filtered_chapters(&open.chapters, &open.filter);
            let sel = open.list.selected().unwrap_or(0);
            let Some(&(idx, _)) = vis.get(sel) else { return };
            idx
        };
        self.jump_to_index(idx);
    }

    pub(super) fn ensure_reader_shell(&mut self) {
        if self.reader.is_some() {
            return;
        }
        let Some(open) = self.open.as_ref() else { return };
        let mut cache = WrapCache::default();
        cache.set_text("正在拉取章节…");
        self.reader = Some(ReaderSession {
            book: open.book.clone(),
            chapters: open.chapters.clone(),
            index: open.list.selected().unwrap_or(0),
            body: ChapterBody {
                item_id: String::new(),
                book_id: open.book.book_id.clone(),
                book_name: open.book.title.clone(),
                title: "加载中".into(),
                content: "正在拉取章节…".into(),
                pre_item_id: String::new(),
                next_item_id: String::new(),
                need_pay: false,
                locked: false,
            },
            cache,
            offset: 0,
            pending_anchor: None,
            view_height: 0,
            demo_bodies: open.demo_bodies.clone(),
        });
    }

    pub(super) fn reader_scroll(&mut self, delta: i32) {
        if let Some(r) = self.reader.as_mut() {
            r.offset = (r.offset as i32 + delta).max(0) as usize;
            self.last_auto = std::time::Instant::now();
            self.mark();
        }
    }

    /// 整页翻动。到底之后再按一次向下翻页才进入下一章；连按（间隔小于 PAGE_DEBOUNCE）会停在本章末尾，避免快速翻页冲进下一章。
    pub(super) fn reader_page(&mut self, dir: i32) {
        let Some(r) = self.reader.as_ref() else { return };
        let now = std::time::Instant::now();
        let rapid = now.duration_since(self.last_page) < PAGE_DEBOUNCE;
        self.last_page = now;
        if dir > 0 && r.at_end() && r.view_height > 0 {
            if !rapid {
                self.reader_chapter(1);
            }
            return;
        }
        let step = r.view_height.saturating_sub(1).max(1) as i32;
        self.reader_scroll(dir * step);
    }

    /// 阅读页按 r 循环换行整理模式。状态行报出 Auto 在本章的判定结果，读者能看出是自动合并还是自己固定的。
    /// 伪装态不画 status，改在皮肤状态行放一条英文短提示，几秒后自动撤掉。
    pub(super) fn reader_cycle_reflow(&mut self) {
        self.state.settings.cycle_reflow(1);
        let Some(r) = self.reader.as_ref() else { return };
        let verdict = crate::reflow::detect(&r.body.content);
        self.status = match self.state.settings.reflow {
            Reflow::Auto if verdict.fragmented() => format!("整理：自动（碎行 {}%，已合并）", verdict.nonterm_percent()),
            Reflow::Auto => "整理：自动（原样）".into(),
            Reflow::Raw => "整理：原样".into(),
            Reflow::Merge => "整理：合并".into(),
        };
        if self.covered() {
            self.cover_note = Some((crate::ui::reflow_note(self.state.settings.reflow, verdict.fragmented()), std::time::Instant::now()));
        }
        self.persist();
        self.mark();
    }

    /// 伪装态的工作行要转 spinner、走秒：busy 期间每隔一段时间主动置脏重绘，并记住 busy 的起点。
    pub(super) fn tick_cover_anim(&mut self) {
        if !self.covered() || !self.busy {
            self.cover_work_since = None;
            return;
        }
        let now = std::time::Instant::now();
        if self.cover_work_since.is_none() {
            self.cover_work_since = Some(now);
        }
        if now.duration_since(self.last_anim) >= ANIM_TICK {
            self.last_anim = now;
            self.mark();
        }
    }

    /// 伪装态的一次性提示到时撤掉。没有输入时不会重绘，所以要在循环里主动置脏。
    pub(super) fn tick_cover_note(&mut self) {
        if let Some((_, since)) = &self.cover_note
            && since.elapsed() >= COVER_NOTE_TTL
        {
            self.cover_note = None;
            self.mark();
        }
    }

    pub(super) fn reader_chapter(&mut self, dir: i32) {
        let item_id = {
            let Some(r) = self.reader.as_ref() else { return };
            let next = r.index as i32 + dir;
            if next < 0 || next as usize >= r.chapters.len() {
                None
            } else {
                Some(r.chapters[next as usize].item_id.clone())
            }
        };
        let Some(item_id) = item_id else {
            self.status = "没有更多章节".into();
            self.mark();
            return;
        };
        self.save_reader_progress();
        self.spawn_chapter(item_id);
    }

    pub(super) fn save_reader_progress(&mut self) {
        let Some(r) = self.reader.as_ref() else { return };
        if r.body.item_id.is_empty() {
            return;
        }
        let progress = Progress {
            book_id: r.book.book_id.clone(),
            item_id: r.body.item_id.clone(),
            title: r.body.title.clone(),
            line: r.offset,
            anchor: r.cache.anchor_of(r.offset).or(r.pending_anchor),
            updated_ms: store::now_ms(),
        };
        store::upsert_progress(&mut self.state, progress.clone());
        let mut item = ShelfItem::from_book(&r.book);
        item.last_read_item_id = r.body.item_id.clone();
        item.last_read_title = r.body.title.clone();
        store::upsert_shelf(&mut self.state, item);
        let _ = store::save(&self.state);
        if self.state.user.is_some() {
            let client = self.client.clone();
            let order = r.index as u32 + 1;
            self.rt.spawn(async move {
                let _ = client.push_progress(&progress, order).await;
            });
        }
    }

    pub(crate) fn visible_shelf(&self) -> Vec<&ShelfItem> {
        let mut items: Vec<&ShelfItem> = self.state.shelf.iter().collect();
        match self.state.settings.sort {
            SortMode::Recent => {
                items.sort_by_key(|b| std::cmp::Reverse(self.state.progress.get(&b.book_id).map(|p| p.updated_ms).unwrap_or(0)));
            }
            SortMode::Title => items.sort_by(|a, b| a.title.cmp(&b.title)),
            SortMode::Author => items.sort_by(|a, b| a.author.cmp(&b.author)),
        }
        let folders = store::all_folders(&self.state);
        if let Some(folder) = folders.get(self.folder_idx)
            && folder != UNGROUPED
        {
            items.retain(|b| b.folder() == folder);
        }
        items
    }

    pub(super) fn selected_shelf_item(&self) -> Option<&ShelfItem> {
        let vis = self.visible_shelf();
        vis.get(self.shelf_list.selected()?).copied()
    }

    pub(super) fn sync_shelf_select(&mut self) {
        let n = self.visible_shelf().len();
        if n == 0 {
            self.shelf_list.select(None);
        } else if self.shelf_list.selected().map(|i| i >= n).unwrap_or(true) {
            self.shelf_list.select(Some(0));
        }
    }

    /// resume 为真时目录到手后直接进入上次读到的章节；没有阅读记录则停在目录页。
    pub(super) fn open_selected_shelf(&mut self, resume: bool) {
        let book = {
            let Some(item) = self.selected_shelf_item() else { return };
            if item.book_id == "demo" { None } else { Some(item.to_book()) }
        };
        match book {
            None => self.open_demo(),
            Some(book) => {
                self.resume_on_open = resume;
                self.spawn_open(book);
            }
        }
    }

    pub(super) fn open_selected_search(&mut self) {
        let Some(idx) = self.search_list.selected() else { return };
        let Some(book) = self.search_results.get(idx).cloned() else { return };
        self.spawn_open(book);
    }

    pub(super) fn remove_selected_shelf(&mut self) {
        let Some(item) = self.selected_shelf_item() else { return };
        let id = item.book_id.clone();
        let title = item.title.clone();
        store::remove_shelf(&mut self.state, &id);
        self.persist();
        if self.state.user.is_some() && id != "demo" {
            let client = self.client.clone();
            self.rt.spawn(async move {
                let _ = client.remove_bookshelf(&id).await;
            });
        }
        self.sync_shelf_select();
        self.status = format!("已移出 {title}");
        self.mark();
    }

    pub(super) fn add_open_to_shelf(&mut self) {
        let Some(open) = self.open.as_ref() else { return };
        let item = ShelfItem::from_book(&open.book);
        let id = item.book_id.clone();
        let title = item.title.clone();
        let remote = self.state.user.is_some() && id != "demo";
        store::upsert_shelf(&mut self.state, item);
        self.persist();
        if remote {
            let client = self.client.clone();
            self.rt.spawn(async move {
                let _ = client.add_bookshelf(&id).await;
            });
        }
        self.status = format!("已加入书架：{title}");
        self.mark();
    }

    pub(super) fn open_demo(&mut self) {
        let (book, chapters, bodies) = demo_book();
        let mut list = ratatui::widgets::ListState::default();
        list.select(Some(0));
        let mut item = ShelfItem::from_book(&book);
        item.last_read_item_id = "demo-1".into();
        item.group_name = "演示".into();
        store::upsert_shelf(&mut self.state, item);
        self.open = Some(OpenBook { book, chapters, list, filter: String::new(), demo_bodies: bodies });
        store::ensure_folder(&mut self.state, "演示");
        self.persist();
        self.push(Screen::Book);
    }

    pub(super) fn logout(&mut self) {
        self.client.logout();
        self.state.user = None;
        self.state.cookies.clear();
        self.persist();
        self.overlay = Overlay::None;
        self.screens = vec![Screen::Home];
        self.home_list.select(Some(0));
        self.status = "已退出登录".into();
        self.mark();
    }
}
