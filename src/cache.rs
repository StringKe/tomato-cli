//! 章节正文、封面和整理书架用的目录的本地缓存。预读和已读章节、下载过的封面都写在这里，下次不再走网络。
//! 一个 item_id / book_id 一个文件，按写入时间淘汰，目录总量超过上限时删最旧的。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::ChapterBody;
use crate::store;

/// 章节缓存上限。一章约 10 KB，64 MB 够放几千章。
const CHAPTER_CAP: u64 = 64 * 1024 * 1024;
/// 封面缓存上限。一张约 30 KB。
const COVER_CAP: u64 = 32 * 1024 * 1024;
/// 目录缓存上限。一千章约 35 KB。
const TOC_CAP: u64 = 16 * 1024 * 1024;

/// 整理书架用的精简目录：目录顺序的 (item_id, 发布时间秒)。书有更新后最后一章变了，调用方据此判断缓存过期。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Toc {
    pub items: Vec<(String, u64)>,
}

impl Toc {
    pub fn last_item(&self) -> &str {
        self.items.last().map(|(id, _)| id.as_str()).unwrap_or("")
    }
}

pub fn dir() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("chapters"))
}

pub fn cover_dir() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("covers"))
}

pub fn toc_dir() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("tocs"))
}

fn toc_path(book_id: &str) -> Result<PathBuf> {
    Ok(toc_dir()?.join(format!("{}.json", safe_name(book_id))))
}

pub fn toc_get(book_id: &str) -> Option<Toc> {
    let raw = fs::read_to_string(toc_path(book_id).ok()?).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn toc_put(book_id: &str, toc: &Toc) -> Result<()> {
    if book_id.is_empty() || toc.items.is_empty() {
        return Ok(());
    }
    write_atomic(&toc_path(book_id)?, serde_json::to_string(toc)?.as_bytes())?;
    prune(&toc_dir()?, TOC_CAP)
}

/// id 只有数字，不会带路径分隔符；保险起见仍过滤一遍。
fn safe_name(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect()
}

fn path_for(item_id: &str) -> Result<PathBuf> {
    Ok(dir()?.join(format!("{}.json", safe_name(item_id))))
}

fn cover_path(book_id: &str) -> Result<PathBuf> {
    Ok(cover_dir()?.join(format!("{}.img", safe_name(book_id))))
}

pub fn get(item_id: &str) -> Option<ChapterBody> {
    let raw = fs::read_to_string(path_for(item_id).ok()?).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn has(item_id: &str) -> bool {
    path_for(item_id).map(|p| p.exists()).unwrap_or(false)
}

/// 原子写入，然后按上限淘汰。演示书和空正文不缓存。
pub fn put(body: &ChapterBody) -> Result<()> {
    if body.book_id == "demo" || body.item_id.is_empty() || body.content.is_empty() {
        return Ok(());
    }
    write_atomic(&path_for(&body.item_id)?, serde_json::to_string(body)?.as_bytes())?;
    prune(&dir()?, CHAPTER_CAP)
}

pub fn cover_get(book_id: &str) -> Option<Vec<u8>> {
    fs::read(cover_path(book_id).ok()?).ok().filter(|b| !b.is_empty())
}

pub fn cover_put(book_id: &str, bytes: &[u8]) -> Result<()> {
    if book_id.is_empty() || bytes.is_empty() {
        return Ok(());
    }
    write_atomic(&cover_path(book_id)?, bytes)?;
    prune(&cover_dir()?, COVER_CAP)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("缓存路径没有父目录")?;
    fs::create_dir_all(dir).with_context(|| format!("创建 {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).context("创建临时文件")?;
    tmp.write_all(bytes)?;
    tmp.persist(path).map_err(|e| e.error).with_context(|| format!("写入 {}", path.display()))?;
    Ok(())
}

/// 某个缓存目录的文件数和总字节数。
pub fn stats(dir: &Path) -> (usize, u64) {
    let Ok(entries) = fs::read_dir(dir) else { return (0, 0) };
    entries.flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).fold((0, 0), |(n, bytes), m| (n + 1, bytes + m.len()))
}

pub fn clear() -> Result<()> {
    for dir in [dir()?, cover_dir()?, toc_dir()?] {
        if dir.exists() {
            fs::remove_dir_all(&dir).with_context(|| format!("删除 {}", dir.display()))?;
        }
    }
    Ok(())
}

fn prune(dir: &Path, cap: u64) -> Result<()> {
    let mut files: Vec<(SystemTime, u64, PathBuf)> = fs::read_dir(dir)?
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            if !m.is_file() {
                return None;
            }
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= cap {
        return Ok(());
    }
    files.sort_by_key(|f| f.0);
    for (_, len, path) in files {
        if total <= cap {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
    Ok(())
}

pub fn size_label(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::size_label;

    #[test]
    fn size_label_picks_unit() {
        assert_eq!(size_label(512), "512 B");
        assert_eq!(size_label(20 * 1024), "20 KB");
        assert_eq!(size_label(3 * 1024 * 1024 + 512 * 1024), "3.5 MB");
    }
}
