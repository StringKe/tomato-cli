use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};

use crate::store;

const BUNDLED: &str = include_str!("fontmap.json");
const UNICODE_START: u32 = 0xe3e8;
const GID_START: u32 = 58344;
pub(super) const MIN_ENTRIES: usize = 100;

static MAP: OnceLock<RwLock<HashMap<char, char>>> = OnceLock::new();
static VGID_DICT: OnceLock<HashMap<u32, char>> = OnceLock::new();

fn slot() -> &'static RwLock<HashMap<char, char>> {
    MAP.get_or_init(|| RwLock::new(load_initial()))
}

pub(super) fn lock_map() -> std::sync::RwLockReadGuard<'static, HashMap<char, char>> {
    slot().read().unwrap_or_else(|e| e.into_inner())
}

pub(super) fn apply(map: HashMap<char, char>) {
    *slot().write().unwrap_or_else(|e| e.into_inner()) = map;
}

pub(super) fn merge(base: HashMap<char, char>, overlay: HashMap<char, char>) -> HashMap<char, char> {
    let mut out = base;
    out.extend(overlay);
    out
}

pub(super) fn vgid_dict() -> &'static HashMap<u32, char> {
    VGID_DICT.get_or_init(|| {
        let data: HashMap<String, String> = serde_json::from_str(BUNDLED).unwrap_or_default();
        data.into_iter()
            .filter_map(|(k, v)| {
                let gid = k.parse::<u32>().ok()?;
                let ch = v.chars().next()?;
                Some((gid, ch))
            })
            .collect()
    })
}

pub(super) fn linear_vgid(cp: u32) -> u32 {
    GID_START.wrapping_add(cp.wrapping_sub(UNICODE_START))
}

pub(super) fn linear_from_bundled() -> HashMap<char, char> {
    vgid_dict()
        .iter()
        .filter_map(|(&vgid, &han)| {
            let cp = UNICODE_START.wrapping_add(vgid.wrapping_sub(GID_START));
            char::from_u32(cp).map(|c| (c, han))
        })
        .collect()
}

fn parse_pua_json(raw: &str) -> Result<HashMap<char, char>> {
    let data: HashMap<String, String> = serde_json::from_str(raw).context("字表 JSON 无效")?;
    let map: HashMap<char, char> = data
        .into_iter()
        .filter_map(|(k, v)| {
            let code = k.parse::<u32>().ok()?;
            let from = char::from_u32(code)?;
            let to = v.chars().next()?;
            Some((from, to))
        })
        .collect();
    if map.len() < MIN_ENTRIES {
        bail!("字表条目过少（{}）", map.len());
    }
    Ok(map)
}

pub(super) fn load_disk() -> Option<HashMap<char, char>> {
    let path = cache_path().ok()?;
    let raw = std::fs::read_to_string(path).ok()?;
    parse_pua_json(&raw).ok()
}

fn load_initial() -> HashMap<char, char> {
    match load_disk() {
        Some(disk) => merge(linear_from_bundled(), disk),
        None => linear_from_bundled(),
    }
}

pub fn cache_path() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("fontmap.json"))
}

fn font_url_path() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("font-url.txt"))
}

pub(super) fn load_font_url() -> Option<String> {
    let raw = std::fs::read_to_string(font_url_path().ok()?).ok()?;
    let url = raw.lines().next().unwrap_or("").trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

pub struct FontmapStatus {
    pub entries: usize,
    pub source: &'static str,
    pub path: PathBuf,
    pub font_url: Option<String>,
    pub age: Option<Duration>,
}

pub fn status() -> FontmapStatus {
    let path = cache_path().unwrap_or_else(|_| PathBuf::from("fontmap.json"));
    let on_disk = load_disk().is_some();
    let age = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok()).and_then(|t| SystemTime::now().duration_since(t).ok());
    FontmapStatus {
        entries: lock_map().len(),
        source: if on_disk { "官方生成缓存" } else { "内置兜底" },
        path,
        font_url: load_font_url(),
        age,
    }
}

fn map_to_json(map: &HashMap<char, char>) -> Result<String> {
    let ordered: BTreeMap<String, String> = map.iter().map(|(k, v)| (u32::from(*k).to_string(), v.to_string())).collect();
    serde_json::to_string_pretty(&ordered).context("序列化字表")
}

fn persist_file(dir: &std::path::Path, name: &str, bytes: &[u8]) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("创建 {}", dir.display()))?;
    let path = dir.join(name);
    let mut tmp = tempfile::NamedTempFile::new_in(dir).context("创建字表临时文件")?;
    tmp.write_all(bytes).context("写入字表")?;
    tmp.persist(&path).map_err(|e| e.error).with_context(|| format!("保存 {}", path.display()))?;
    Ok(path)
}

pub(super) fn write_cache(map: &HashMap<char, char>, font_url: &str) -> Result<()> {
    let dir = store::config_dir()?;
    persist_file(&dir, "fontmap.json", map_to_json(map)?.as_bytes())?;
    persist_file(&dir, "font-url.txt", format!("{font_url}\n").as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_pua() {
        let ch = char::from_u32(58611).unwrap();
        assert_eq!(crate::font::decode_pua(&ch.to_string()), "的");
    }

    #[test]
    fn bundled_parses() {
        assert!(parse_pua_json(BUNDLED).unwrap().len() >= 100);
        assert_eq!(vgid_dict().get(&58611).copied(), Some('的'));
    }
}
