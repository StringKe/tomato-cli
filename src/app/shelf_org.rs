//! 书架整理与分组同步：一键整理、刷新后自动整理、读完最后一章归档、手动移动和文件夹改动同步到番茄。
//! 判定规则在 crate::organize，这里只管拉数据、改 App 状态和发请求。

use std::collections::HashSet;

use crate::api::Client;
use crate::cache::{self, Toc};
use crate::model::{ReadStats, ShelfItem, UNGROUPED};
use crate::organize::{self, Move};
use crate::store;

use super::workers::{HYDRATE_GAP, WorkerMsg};
use super::{App, Overlay, list_at};

/// 其余信号都没变时阅读数据的有效期。服务端已读列表可能在书架字段不变的情况下变化，过期就重拉。
const STATS_TTL_MS: u64 = 7 * 86_400_000;

/// 整理任务里一本书的输入，取自书架条目和本地阅读记录。
struct Target {
    book_id: String,
    /// 书架上的最新章。目录缓存的最后一章和它一致才可用，否则书更新过。
    last_chapter: String,
    operated_ms: u64,
    local: Option<(String, u64)>,
    last_read: String,
}

impl App {
    /// 书架按 z。
    pub(super) fn start_organize(&mut self) {
        if self.state.user.is_none() {
            self.status = "登录后才能整理书架".into();
        } else if self.organizing {
            self.status = "正在整理…".into();
        } else {
            let targets = self.organize_targets(|_| true);
            if targets.is_empty() {
                self.status = "没有需要整理的书".into();
            } else {
                self.busy = true;
                self.status = format!("整理中 0/{}", targets.len());
                self.spawn_organize(targets, false);
            }
        }
        self.mark();
    }

    /// 整理预览里按 Enter。
    pub(super) fn apply_organize(&mut self) {
        let plan = std::mem::take(&mut self.organize_plan);
        if self.state.organized_ms == 0 {
            self.state.organized_ms = store::now_ms();
        }
        self.land_moves(&plan);
        self.overlay = Overlay::None;
        self.status = format!("已整理 {} 本", plan.len());
        self.mark();
    }

    /// 整理预览里按 Esc，或用鼠标关掉预览。
    pub(super) fn cancel_organize(&mut self) {
        self.organize_plan.clear();
        self.overlay = Overlay::None;
        self.status = "已取消整理".into();
        self.mark();
    }

    /// 「移动到」浮层确认。folder 是 `store::all_folders` 里的名字，UNGROUPED 表示默认。
    pub(super) fn move_selected_to(&mut self, folder: &str) {
        let Some((book_id, from)) = self.selected_shelf_item().map(|item| (item.book_id.clone(), item.group_name.clone())) else { return };
        let tab = self.shelf_tab_name();
        store::move_to_folder(&mut self.state, &book_id, folder);
        let now = store::now_ms();
        let abandon_days = self.state.settings.abandon_days;
        if let Some(item) = self.state.shelf.iter_mut().find(|b| b.book_id == book_id) {
            organize::pin(item, now, abandon_days);
        }
        self.persist();
        let group = if folder == UNGROUPED { String::new() } else { folder.to_string() };
        // 服务端把原地移动记为失败，不发。
        if group != from {
            self.push_groups(vec![(book_id, group)]);
        }
        self.overlay = Overlay::None;
        self.status = format!("已移到 {folder}");
        self.restore_shelf_tab(tab);
        self.sync_shelf_select();
        self.mark();
    }

    /// 把 (book_id, 分组名) 同步到番茄，分组名空串表示默认。
    pub(super) fn push_groups(&mut self, moves: Vec<(String, String)>) {
        if self.state.user.is_none() {
            return;
        }
        let moves: Vec<(String, String)> = moves.into_iter().filter(|(id, _)| id != "demo" && (self.remote_ids.is_empty() || self.remote_ids.contains(id))).collect();
        if moves.is_empty() {
            return;
        }
        let client = self.client.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = client.move_groups(&moves).await.map(|rejected| rejected.len()).map_err(|e| e.to_string());
            let _ = tx.send(WorkerMsg::GroupsSynced(result));
        });
    }

    pub(super) fn groups_synced(&mut self, result: Result<usize, String>) {
        match result {
            Ok(0) => {}
            Ok(n) => self.status = format!("{n} 本分组没同步到番茄"),
            Err(e) => self.status = format!("分组同步失败：{e}"),
        }
    }

    pub(super) fn organize_stat(&mut self, book_id: String, stats: Option<ReadStats>, auto: bool) {
        if let Some(stats) = stats
            && let Some(item) = self.state.shelf.iter_mut().find(|b| b.book_id == book_id)
        {
            item.read_stats = Some(stats);
        }
        self.organize_progress.0 += 1;
        if !auto {
            self.status = format!("整理中 {}/{}", self.organize_progress.0, self.organize_progress.1);
        }
    }

    pub(super) fn organize_done(&mut self, auto: bool, failed: usize) {
        self.organizing = false;
        let plan = organize::plan(&mut self.state.shelf, store::now_ms(), self.state.settings.abandon_days);
        self.persist();
        if auto {
            self.land_auto(&plan);
            return;
        }
        let total = self.organize_progress.1;
        if plan.is_empty() {
            self.status = if failed == 0 {
                "书架已经整理好了".into()
            } else if failed >= total {
                format!("{failed} 本都没拉到数据")
            } else {
                format!("书架已经整理好了，{failed} 本没拉到数据")
            };
            return;
        }
        self.status = if failed > 0 { format!("{failed} 本没拉到数据") } else { String::new() };
        self.organize_plan = plan;
        self.organize_list = list_at(0);
        self.overlay = Overlay::Organize;
    }

    /// 刷新书架后只重拉阅读情况可能变了的书；都没变也按已有数据判定一次，弃读是随时间变的。
    pub(super) fn auto_organize_after_refresh(&mut self) {
        if !self.auto_organize_ready() {
            return;
        }
        let now = store::now_ms();
        let targets = self.organize_targets(|b| self.stats_stale(b, now));
        if targets.is_empty() {
            let plan = organize::plan(&mut self.state.shelf, now, self.state.settings.abandon_days);
            self.land_auto(&plan);
        } else {
            self.spawn_organize(targets, true);
        }
    }

    /// 阅读页在最后一章按下一章：当场整理这一本。
    /// 自动翻页停在全书末尾时每个周期都会走到这里，已按这一章算过阅读情况的就不再重拉。
    pub(super) fn archive_finished(&mut self) {
        let Some((book_id, item_id)) = self.reader.as_ref().map(|r| (r.book.book_id.clone(), r.body.item_id.clone())) else { return };
        if book_id == "demo" || item_id.is_empty() || !self.auto_organize_ready() {
            return;
        }
        let eligible = self.state.shelf.iter().any(|b| b.book_id == book_id && organize::is_managed(&b.group_name) && b.read_stats.as_ref().is_none_or(|s| s.last_item != item_id));
        if !eligible {
            return;
        }
        // 判定看本地进度读到哪一章，先把最后一章记下来。
        self.save_reader_progress();
        let targets = self.organize_targets(|b| b.book_id == book_id);
        self.spawn_organize(targets, true);
    }

    fn auto_organize_ready(&self) -> bool {
        self.state.settings.auto_organize && self.state.organized_ms > 0 && self.state.user.is_some() && !self.organizing && self.overlay != Overlay::Organize
    }

    fn stats_stale(&self, item: &ShelfItem, now: u64) -> bool {
        let Some(s) = item.read_stats.as_ref() else { return true };
        let local_ms = self.state.progress.get(&item.book_id).map_or(0, |p| p.updated_ms);
        (!item.last_chapter_item_id.is_empty() && item.last_chapter_item_id != s.toc_last) || s.last_item != item.last_read_item_id || item.operated_ms > s.fetched_ms || local_ms > s.fetched_ms || now.saturating_sub(s.fetched_ms) > STATS_TTL_MS
    }

    /// 受管文件夹里的非演示书中满足 keep 的。
    fn organize_targets(&self, keep: impl Fn(&ShelfItem) -> bool) -> Vec<Target> {
        self.state
            .shelf
            .iter()
            .filter(|&b| b.book_id != "demo" && organize::is_managed(&b.group_name) && keep(b))
            .map(|b| Target {
                book_id: b.book_id.clone(),
                last_chapter: b.last_chapter_item_id.clone(),
                operated_ms: b.operated_ms,
                local: self.state.progress.get(&b.book_id).map(|p| (p.item_id.clone(), p.updated_ms)),
                last_read: b.last_read_item_id.clone(),
            })
            .collect()
    }

    /// 逐本拉已读列表和目录。手动整理用户在等，走前台；自动整理让路给前台请求，书与书之间留间隔。
    fn spawn_organize(&mut self, targets: Vec<Target>, auto: bool) {
        self.organizing = true;
        self.organize_progress = (0, targets.len());
        let client = if auto { self.client.background() } else { self.client.clone() };
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let mut failed = 0;
            for (i, target) in targets.into_iter().enumerate() {
                if auto && i > 0 {
                    tokio::time::sleep(HYDRATE_GAP).await;
                }
                let stats = measure_one(&client, &target).await;
                if stats.is_none() {
                    failed += 1;
                }
                if tx.send(WorkerMsg::OrganizeStat { book_id: target.book_id, stats, auto }).is_err() {
                    return;
                }
            }
            let _ = tx.send(WorkerMsg::OrganizeDone { auto, failed });
        });
    }

    fn land_auto(&mut self, plan: &[Move]) {
        if plan.is_empty() {
            return;
        }
        self.land_moves(plan);
        self.status = format!("自动整理：移动 {} 本", plan.len());
    }

    /// 移动落到本地、落盘并同步到番茄。
    fn land_moves(&mut self, moves: &[Move]) {
        let tab = self.shelf_tab_name();
        store::apply_moves(&mut self.state, moves);
        self.persist();
        self.push_groups(moves.iter().map(|m| (m.book_id.clone(), m.to.clone())).collect());
        self.restore_shelf_tab(tab);
        self.sync_shelf_select();
    }

    fn shelf_tab_name(&self) -> Option<String> {
        store::shelf_tabs(&self.state).get(self.folder_idx).cloned()
    }

    /// 文件夹增减后标签下标会错位：原标签还在就回到它，不在了就停在原下标并夹到范围内。
    fn restore_shelf_tab(&mut self, tab: Option<String>) {
        let tabs = store::shelf_tabs(&self.state);
        let idx = tab.and_then(|t| tabs.iter().position(|x| *x == t)).unwrap_or(self.folder_idx);
        self.folder_idx = idx.min(tabs.len().saturating_sub(1));
    }
}

/// 一本书的阅读情况。任何一步失败或目录为空都返回 None。
async fn measure_one(client: &Client, t: &Target) -> Option<ReadStats> {
    let read: HashSet<String> = client.read_items(&t.book_id).await.ok()?.into_iter().collect();
    let cached = cache::toc_get(&t.book_id).filter(|toc| !t.last_chapter.is_empty() && toc.last_item() == t.last_chapter);
    let toc = match cached {
        Some(toc) => toc,
        None => {
            let toc = Toc { items: client.directory(&t.book_id).await.ok()?.into_iter().map(|c| (c.item_id, c.published)).collect() };
            let _ = cache::toc_put(&t.book_id, &toc);
            toc
        }
    };
    if toc.items.is_empty() {
        return None;
    }
    let local = t.local.as_ref().map(|(id, ms)| (id.as_str(), *ms));
    let mut stats = organize::measure(&toc.items, &read, local, t.operated_ms, store::now_ms());
    stats.last_item.clone_from(&t.last_read);
    Some(stats)
}
