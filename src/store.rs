use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::{ALL_BOOKS, Progress, Settings, ShelfItem, UNGROUPED, User};
use crate::organize::{self, Move};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct State {
    pub cookies: BTreeMap<String, String>,
    pub user: Option<User>,
    pub shelf: Vec<ShelfItem>,
    pub progress: BTreeMap<String, Progress>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub config_rev: u32,
    /// 第一次确认一键整理的时间（毫秒），0 表示还没整理过。自动整理在这之后才生效，避免升级后一刷新就大批移动。
    #[serde(default)]
    pub organized_ms: u64,
}

pub fn config_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("com", "stringke", "tomato-cli").context("无法解析配置目录")?;
    Ok(dirs.config_dir().to_path_buf())
}

pub fn load() -> Result<State> {
    let path = config_dir()?.join("state.json");
    if !path.exists() {
        return Ok(State::default());
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("读取 {}", path.display()))?;
    let mut state: State = serde_json::from_str(&raw).with_context(|| format!("解析 {}", path.display()))?;
    if state.config_rev < 1 {
        state.settings.theme = crate::theme::ThemeId::Auto;
        state.config_rev = 1;
    }
    Ok(state)
}

pub fn save(state: &State) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("创建 {}", dir.display()))?;
    let path = dir.join("state.json");
    let mut tmp = tempfile::NamedTempFile::new_in(&dir).context("创建临时文件")?;
    tmp.write_all(serde_json::to_string_pretty(state)?.as_bytes())?;
    tmp.persist(&path).map_err(|e| e.error).with_context(|| format!("写入 {}", path.display()))?;
    Ok(())
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn cookie_header(cookies: &BTreeMap<String, String>) -> Option<String> {
    if cookies.is_empty() {
        return None;
    }
    Some(cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; "))
}

pub fn merge_set_cookie(cookies: &mut BTreeMap<String, String>, header: &str) {
    if let Ok(c) = cookie::Cookie::parse(header.to_string()) {
        cookies.insert(c.name().to_string(), c.value().to_string());
    }
}

pub fn upsert_progress(state: &mut State, progress: Progress) {
    state.progress.insert(progress.book_id.clone(), progress);
}

/// 新值优先，已有条目里的其余字段（分组、封面、章数等）保留。
pub fn upsert_shelf(state: &mut State, mut item: ShelfItem) {
    if let Some(existing) = state.shelf.iter_mut().find(|b| b.book_id == item.book_id) {
        item.fill_missing_from(existing);
        *existing = item;
        return;
    }
    if item.added_ms == 0 {
        item.added_ms = now_ms();
    }
    state.shelf.insert(0, item);
}

pub fn remove_shelf(state: &mut State, book_id: &str) {
    state.shelf.retain(|b| b.book_id != book_id);
}

/// 用远端书架替换本地书架，返回远端新出现的本数。元数据只补空位；分组以远端为准，否则在手机上移回默认的书在本地还留在原文件夹。
/// 远端没有的书保留为本地条目。
pub fn merge_remote_shelf(state: &mut State, items: Vec<ShelfItem>) -> usize {
    let local = std::mem::take(&mut state.shelf);
    let mut merged = items;
    let mut added = 0;
    for remote in &mut merged {
        match local.iter().find(|b| b.book_id == remote.book_id) {
            Some(old) => {
                let group = std::mem::take(&mut remote.group_name);
                remote.fill_missing_from(old);
                remote.group_name = group;
            }
            None => added += 1,
        }
    }
    for old in local {
        if !merged.iter().any(|b| b.book_id == old.book_id) {
            merged.push(old);
        }
    }
    state.shelf = merged;
    added
}

pub fn all_folders(state: &State) -> Vec<String> {
    let mut set: Vec<String> = state.folders.clone();
    for item in &state.shelf {
        let name = item.folder().to_string();
        if !set.iter().any(|f| f == &name) {
            set.push(name);
        }
    }
    if !set.iter().any(|f| f == UNGROUPED) {
        set.push(UNGROUPED.to_string());
    }
    set.sort_by(|a, b| organize::folder_order(a, b));
    set
}

/// 整理结果落到本地。不写进 `folders`：整理出来的文件夹空了就该消失，不像用户新建的文件夹那样常驻。
pub fn apply_moves(state: &mut State, moves: &[Move]) {
    for m in moves {
        if let Some(item) = state.shelf.iter_mut().find(|b| b.book_id == m.book_id) {
            item.group_name.clone_from(&m.to);
        }
    }
}

fn books_in(state: &State, folder: &str) -> Vec<String> {
    state.shelf.iter().filter(|b| b.folder() == folder).map(|b| b.book_id.clone()).collect()
}

/// 书架顶部的标签：默认（没进文件夹的书）、各文件夹、全部。`App::folder_idx` 是这个列表的下标。
pub fn shelf_tabs(state: &State) -> Vec<String> {
    let mut tabs = all_folders(state);
    tabs.push(ALL_BOOKS.to_string());
    tabs
}

pub fn ensure_folder(state: &mut State, name: &str) {
    let name = name.trim();
    if name.is_empty() || name == UNGROUPED {
        return;
    }
    if !state.folders.iter().any(|f| f == name) {
        state.folders.push(name.to_string());
    }
}

/// 返回被改名的书，调用方据此同步到远端。
pub fn rename_folder(state: &mut State, from: &str, to: &str) -> Vec<String> {
    let to = to.trim();
    if from == UNGROUPED || to.is_empty() {
        return Vec::new();
    }
    let moved = books_in(state, from);
    for item in &mut state.shelf {
        if item.folder() == from {
            item.group_name = to.to_string();
        }
    }
    state.folders.retain(|f| f != from);
    ensure_folder(state, to);
    moved
}

/// 文件夹里的书移回默认，返回这些书。
pub fn delete_folder(state: &mut State, name: &str) -> Vec<String> {
    if name == UNGROUPED {
        return Vec::new();
    }
    let moved = books_in(state, name);
    for item in &mut state.shelf {
        if item.folder() == name {
            item.group_name.clear();
        }
    }
    state.folders.retain(|f| f != name);
    moved
}

pub fn move_to_folder(state: &mut State, book_id: &str, folder: &str) {
    if let Some(item) = state.shelf.iter_mut().find(|b| b.book_id == book_id) {
        item.group_name = if folder == UNGROUPED { String::new() } else { folder.to_string() };
    }
    ensure_folder(state, folder);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(id: &str, group: &str) -> ShelfItem {
        ShelfItem { book_id: id.into(), group_name: group.into(), ..ShelfItem::default() }
    }

    #[test]
    fn tabs_are_default_folders_then_all() {
        let state = State { shelf: vec![book("1", ""), book("2", "完毕"), book("3", "追更")], ..State::default() };
        assert_eq!(shelf_tabs(&state), vec![UNGROUPED, "完毕", "追更", ALL_BOOKS]);
        assert_eq!(all_folders(&state), vec![UNGROUPED, "完毕", "追更"]);
    }

    #[test]
    fn tabs_put_organize_folders_first_and_years_newest_first() {
        let state = State { shelf: vec![book("1", "弃读"), book("2", "2024"), book("3", "收藏"), book("4", "2026"), book("5", "有更新"), book("6", "")], ..State::default() };
        assert_eq!(shelf_tabs(&state), vec![UNGROUPED, "有更新", "2026", "2024", "弃读", "收藏", ALL_BOOKS]);
    }

    #[test]
    fn remote_group_wins_but_local_metadata_fills_gaps() {
        let mut state = State { shelf: vec![ShelfItem { title: "书".into(), pinned: true, ..book("1", "2025") }, book("local", "收藏")], ..State::default() };
        let added = merge_remote_shelf(&mut state, vec![book("1", ""), book("2", "完毕")]);
        assert_eq!(added, 1);
        let first = &state.shelf[0];
        assert_eq!((first.group_name.as_str(), first.title.as_str(), first.pinned), ("", "书", true));
        assert_eq!(state.shelf.iter().map(|b| b.book_id.as_str()).collect::<Vec<_>>(), vec!["1", "2", "local"]);
        assert_eq!(state.shelf[2].group_name, "收藏");
    }

    #[test]
    fn rename_and_delete_report_moved_books() {
        let mut state = State { shelf: vec![book("1", "旧"), book("2", "旧"), book("3", "")], ..State::default() };
        assert_eq!(rename_folder(&mut state, "旧", "新"), vec!["1", "2"]);
        assert_eq!(delete_folder(&mut state, "新"), vec!["1", "2"]);
        assert!(state.shelf.iter().all(|b| b.group_name.is_empty()));
        assert!(rename_folder(&mut state, UNGROUPED, "x").is_empty());
    }

    #[test]
    fn new_shelf_item_gets_added_time_and_keeps_it_on_update() {
        let mut state = State::default();
        upsert_shelf(&mut state, book("1", ""));
        let added = state.shelf[0].added_ms;
        assert!(added > 0);
        upsert_shelf(&mut state, ShelfItem { title: "书".into(), ..book("1", "") });
        assert_eq!(state.shelf[0].added_ms, added);
    }
}
