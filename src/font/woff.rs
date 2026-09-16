use std::collections::HashMap;
use std::io::Read;

use anyhow::{anyhow, bail, Context, Result};

use super::cache::linear_vgid;
use super::cff::parse_cff_gid_names;
use super::cmap::parse_cmap;
use super::{be_u16, be_u32};

const WOFF2_TAGS: [&str; 63] = [
    "cmap", "head", "hhea", "hmtx", "maxp", "name", "OS/2", "post", "cvt ", "fpgm",
    "glyf", "loca", "prep", "CFF ", "VORG", "EBDT", "EBLC", "EBSC", "CBDT", "CBLC",
    "COLR", "CPAL", "SVG ", "sbix", "acnt", "avar", "bdat", "bloc", "bsln", "cvar",
    "fdsc", "feat", "fmtx", "fvar", "gasp", "gcid", "glyf", "gvar", "hdmx", "hsty",
    "just", "kern", "lcar", "loca", "ltag", "MATH", "maxp", "merge", "meta", "mort",
    "morx", "opbd", "prop", "sbix", "seac", "sfnt", "shm", "trak", "vhea", "vmtx",
    "DSIG", "vvar", "",
];

struct FontTables {
    cmap: Option<Vec<u8>>,
    cff: Option<Vec<u8>>,
}

pub(super) fn parse_font_vgid(buf: &[u8]) -> Result<HashMap<u32, u32>> {
    if buf.len() < 4 {
        bail!("字体过短");
    }
    let tables = if buf.starts_with(b"wOF2") {
        parse_woff2(buf)?
    } else if buf.starts_with(b"wOFF") {
        parse_woff1(buf)?
    } else {
        parse_ttf(buf)?
    };
    let cmap = tables.cmap.as_deref().ok_or_else(|| anyhow!("字体缺少 cmap"))?;
    let char_to_gid = parse_cmap(cmap)?;
    if char_to_gid.is_empty() {
        bail!("cmap 为空");
    }
    let gid_to_vgid = tables.cff.as_deref().and_then(|cff| parse_cff_gid_names(cff).ok()).unwrap_or_default();
    let mut out = HashMap::new();
    for (cp, font_gid) in char_to_gid {
        if (0xe000..=0xf8ff).contains(&cp) {
            let vgid = gid_to_vgid.get(&font_gid).copied().unwrap_or_else(|| linear_vgid(cp));
            out.insert(cp, vgid);
        }
    }
    if out.is_empty() {
        bail!("字体不含 PUA 映射");
    }
    Ok(out)
}

fn read_base128(buf: &[u8], p: &mut usize) -> Result<u32> {
    let mut result: u32 = 0;
    for i in 0..5 {
        if *p >= buf.len() {
            bail!("woff2 truncated");
        }
        let b = buf[*p];
        *p += 1;
        if i == 0 && b == 0x80 {
            bail!("invalid base128");
        }
        if result & 0xfe00_0000 != 0 {
            bail!("base128 overflow");
        }
        result = (result << 7) | u32::from(b & 0x7f);
        if b & 0x80 == 0 {
            return Ok(result);
        }
    }
    bail!("base128 too long")
}

fn brotli_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut input = data;
    let mut out = Vec::new();
    brotli::BrotliDecompress(&mut input, &mut out).map_err(|e| anyhow!("brotli 解压失败: {e}"))?;
    Ok(out)
}

fn zlib_inflate(raw: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::ZlibDecoder::new(raw);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).context("zlib 解压失败")?;
    Ok(out)
}

fn parse_woff2(buf: &[u8]) -> Result<FontTables> {
    if buf.len() < 48 {
        bail!("woff2 header 过短");
    }
    if &buf[4..8] == b"ttcf" {
        bail!("不支持 WOFF2 字体集合");
    }
    let num_tables = be_u16(buf, 12)? as usize;
    let total_compressed = be_u32(buf, 20)? as usize;
    let mut p = 48;
    let mut entries: Vec<([u8; 4], usize)> = Vec::with_capacity(num_tables);
    for _ in 0..num_tables {
        if p >= buf.len() {
            bail!("woff2 table directory 截断");
        }
        let flags = buf[p];
        p += 1;
        let tag_index = flags & 0x3f;
        let tag = if tag_index == 63 {
            let s = buf.get(p..p + 4).ok_or_else(|| anyhow!("woff2 自定义 tag 截断"))?;
            p += 4;
            [s[0], s[1], s[2], s[3]]
        } else {
            let name = WOFF2_TAGS.get(tag_index as usize).copied().unwrap_or("");
            let bytes = name.as_bytes();
            if bytes.len() == 4 {
                [bytes[0], bytes[1], bytes[2], bytes[3]]
            } else {
                *b"    "
            }
        };
        let orig_len = read_base128(buf, &mut p)? as usize;
        let stream_len = if flags & 0x40 != 0 { read_base128(buf, &mut p)? as usize } else { orig_len };
        entries.push((tag, stream_len));
    }
    let avail = buf.len().saturating_sub(p);
    let take = if total_compressed > 0 && total_compressed <= avail { total_compressed } else { avail };
    let sfnt = brotli_decompress(&buf[p..p + take])?;
    let mut off: usize = 0;
    let mut out = FontTables { cmap: None, cff: None };
    for (tag, len) in entries {
        let end = off.saturating_add(len);
        if end > sfnt.len() {
            break;
        }
        let data = sfnt[off..end].to_vec();
        if &tag == b"cmap" {
            out.cmap = Some(data);
        } else if &tag == b"CFF " {
            out.cff = Some(data);
        }
        off = end;
    }
    Ok(out)
}

fn parse_woff1(buf: &[u8]) -> Result<FontTables> {
    if buf.len() < 44 {
        bail!("woff header 过短");
    }
    let num_tables = be_u16(buf, 12)? as usize;
    let mut p = 44;
    let mut out = FontTables { cmap: None, cff: None };
    for _ in 0..num_tables {
        let tag = buf.get(p..p + 4).ok_or_else(|| anyhow!("woff 表目录截断"))?;
        let tag = [tag[0], tag[1], tag[2], tag[3]];
        let t_offset = be_u32(buf, p + 4)? as usize;
        let comp_len = be_u32(buf, p + 8)? as usize;
        let orig_len = be_u32(buf, p + 12)? as usize;
        let raw = buf.get(t_offset..t_offset.saturating_add(comp_len)).ok_or_else(|| anyhow!("woff 表数据截断"))?;
        let data = if comp_len < orig_len { zlib_inflate(raw)? } else { raw.to_vec() };
        if &tag == b"cmap" {
            out.cmap = Some(data);
        } else if &tag == b"CFF " {
            out.cff = Some(data);
        }
        p += 20;
    }
    Ok(out)
}

fn parse_ttf(buf: &[u8]) -> Result<FontTables> {
    if buf.len() < 12 {
        bail!("ttf header 过短");
    }
    let num_tables = be_u16(buf, 4)? as usize;
    let mut p = 12;
    let mut out = FontTables { cmap: None, cff: None };
    for _ in 0..num_tables {
        let tag = buf.get(p..p + 4).ok_or_else(|| anyhow!("ttf 表目录截断"))?;
        let tag = [tag[0], tag[1], tag[2], tag[3]];
        let t_offset = be_u32(buf, p + 8)? as usize;
        let t_len = be_u32(buf, p + 12)? as usize;
        if let Some(data) = buf.get(t_offset..t_offset.saturating_add(t_len)) {
            if &tag == b"cmap" {
                out.cmap = Some(data.to_vec());
            } else if &tag == b"CFF " {
                out.cff = Some(data.to_vec());
            }
        }
        p += 16;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_official_woff2_fixture() {
        let path = std::path::Path::new("/tmp/fanqie.woff2");
        if !path.is_file() {
            return;
        }
        let bytes = std::fs::read(path).unwrap();
        let vgid = parse_font_vgid(&bytes).unwrap();
        assert_eq!(vgid.get(&58611).copied(), Some(58611));
        let map = super::super::build_pua_map(&bytes).unwrap();
        assert_eq!(map.get(&char::from_u32(58611).unwrap()).copied(), Some('的'));
        assert!(map.len() >= 100);
    }
}
