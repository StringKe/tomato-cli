use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::{Progress, Settings, ShelfItem, User, UNGROUPED};

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
    state.shelf.insert(0, item);
}

pub fn remove_shelf(state: &mut State, book_id: &str) {
    state.shelf.retain(|b| b.book_id != book_id);
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
        set.insert(0, UNGROUPED.to_string());
    }
    set.sort();
    if let Some(i) = set.iter().position(|f| f == UNGROUPED) {
        let g = set.remove(i);
        set.insert(0, g);
    }
    set
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

pub fn rename_folder(state: &mut State, from: &str, to: &str) {
    let to = to.trim();
    if from == UNGROUPED || to.is_empty() {
        return;
    }
    for item in &mut state.shelf {
        if item.folder() == from {
            item.group_name = to.to_string();
        }
    }
    state.folders.retain(|f| f != from);
    ensure_folder(state, to);
}

pub fn delete_folder(state: &mut State, name: &str) {
    if name == UNGROUPED {
        return;
    }
    for item in &mut state.shelf {
        if item.folder() == name {
            item.group_name.clear();
        }
    }
    state.folders.retain(|f| f != name);
}

pub fn move_to_folder(state: &mut State, book_id: &str, folder: &str) {
    if let Some(item) = state.shelf.iter_mut().find(|b| b.book_id == book_id) {
        item.group_name = if folder == UNGROUPED { String::new() } else { folder.to_string() };
    }
    ensure_folder(state, folder);
}
