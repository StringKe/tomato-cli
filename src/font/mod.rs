//! PUA 解密：官方 WOFF2 的 cmap/CFF 给出「PUA -> 虚拟 gid」，内置 fontmap.json 只作「虚拟 gid -> 汉字」字典。
//! 运行时不请求 GitHub / jsDelivr；无本地缓存才拉官方字体。

mod cache;
mod cff;
mod cmap;
mod discover;
mod woff;

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};
use unicode_width::UnicodeWidthStr;

use cache::{apply, linear_from_bundled, load_disk, load_font_url, lock_map, merge, vgid_dict, write_cache, MIN_ENTRIES};
use discover::{discover_font_url, fetch_font_bytes, http_client};

pub use cache::{cache_path, status, FontmapStatus};

pub fn decode_pua(text: &str) -> String {
    let map = lock_map();
    text.chars().map(|c| *map.get(&c).unwrap_or(&c)).collect()
}

pub fn print_status() -> Result<()> {
    let s: FontmapStatus = status();
    let age = match s.age {
        Some(d) => format!("{} 小时前", d.as_secs() / 3600),
        None => "无".into(),
    };
    print_kv(&[
        ("字表", format!("{} 条", s.entries)),
        ("来源", s.source.to_string()),
        ("路径", s.path.display().to_string()),
        ("官方字体", s.font_url.unwrap_or_else(|| "无".into())),
        ("缓存", age),
        ("更新", "tomato fontmap update".into()),
    ]);
    Ok(())
}

pub fn print_update() -> Result<()> {
    let rt = tokio::runtime::Runtime::new().context("启动 tokio")?;
    let n = rt.block_on(regenerate())?;
    let mut rows = vec![("已写入", format!("{n} 条 -> {}", cache_path()?.display()))];
    if let Some(url) = load_font_url() {
        rows.push(("官方字体", url));
    }
    print_kv(&rows);
    Ok(())
}

/// 标签按显示宽度对齐后输出，CJK 占两格。
fn print_kv(rows: &[(&str, String)]) {
    let label_w = rows.iter().map(|(k, _)| UnicodeWidthStr::width(*k)).max().unwrap_or(0);
    for (k, v) in rows {
        let pad = label_w - UnicodeWidthStr::width(*k) + 2;
        println!("{k}{}{v}", " ".repeat(pad));
    }
}

/// 配置目录已有生成字表则立刻返回，不联网。
pub async fn init_if_needed() -> Result<usize> {
    if let Some(disk) = load_disk() {
        let map = merge(linear_from_bundled(), disk);
        let n = map.len();
        apply(map);
        return Ok(n);
    }
    regenerate().await
}

/// 强制拉官方 WOFF2、解析、写入缓存并替换内存表。
pub async fn regenerate() -> Result<usize> {
    let client = http_client()?;
    let font_url = discover_font_url(&client).await?;
    let bytes = fetch_font_bytes(&client, &font_url).await?;
    let map = build_pua_map(&bytes)?;
    if map.len() < MIN_ENTRIES {
        bail!("官方字体生成字表过少（{}）", map.len());
    }
    write_cache(&map, &font_url)?;
    let n = map.len();
    apply(map);
    Ok(n)
}

fn build_pua_map(font: &[u8]) -> Result<HashMap<char, char>> {
    let dict = vgid_dict();
    let mut map = linear_from_bundled();
    let vgid_of = woff::parse_font_vgid(font)?;
    for (cp, vgid) in vgid_of {
        if let (Some(from), Some(&han)) = (char::from_u32(cp), dict.get(&vgid)) {
            map.insert(from, han);
        }
    }
    Ok(map)
}

fn be_u16(b: &[u8], off: usize) -> Result<u16> {
    let s = b.get(off..off + 2).ok_or_else(|| anyhow!("字体数据截断"))?;
    Ok(u16::from_be_bytes([s[0], s[1]]))
}

fn be_u32(b: &[u8], off: usize) -> Result<u32> {
    let s = b.get(off..off + 4).ok_or_else(|| anyhow!("字体数据截断"))?;
    Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}
