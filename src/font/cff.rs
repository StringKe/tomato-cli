use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};

use super::be_u16;

const CFF_STD_STRINGS: u32 = 391;

fn read_cff_index<'a>(data: &'a [u8], pos: &mut usize) -> Result<Vec<&'a [u8]>> {
    let count = be_u16(data, *pos)? as usize;
    *pos += 2;
    if count == 0 {
        *pos += 1;
        return Ok(Vec::new());
    }
    let off_size = *data.get(*pos).ok_or_else(|| anyhow!("CFF INDEX 截断"))? as usize;
    *pos += 1;
    if !(1..=4).contains(&off_size) {
        bail!("CFF INDEX offSize 无效");
    }
    let mut offsets = Vec::with_capacity(count + 1);
    for _ in 0..=count {
        let mut v = 0usize;
        for _ in 0..off_size {
            v = (v << 8) | usize::from(*data.get(*pos).ok_or_else(|| anyhow!("CFF INDEX offset 截断"))?);
            *pos += 1;
        }
        offsets.push(v);
    }
    let data_start = *pos;
    let mut items = Vec::with_capacity(count);
    for i in 0..count {
        let a = data_start + offsets[i].saturating_sub(1);
        let b = data_start + offsets[i + 1].saturating_sub(1);
        items.push(data.get(a..b).ok_or_else(|| anyhow!("CFF INDEX 数据截断"))?);
    }
    *pos = data_start + offsets[count].saturating_sub(1);
    Ok(items)
}

fn parse_cff_dict(dict: &[u8]) -> Vec<(u16, Vec<i32>)> {
    let mut out = Vec::new();
    let mut operands: Vec<i32> = Vec::new();
    let mut i = 0;
    while i < dict.len() {
        let b0 = dict[i];
        if b0 == 12 {
            if i + 1 >= dict.len() {
                break;
            }
            out.push((1200 + u16::from(dict[i + 1]), std::mem::take(&mut operands)));
            i += 2;
        } else if b0 <= 21 {
            out.push((u16::from(b0), std::mem::take(&mut operands)));
            i += 1;
        } else if let Some((v, n)) = read_cff_operand(dict, i) {
            operands.push(v);
            i += n;
        } else {
            break;
        }
    }
    out
}

fn read_cff_operand(dict: &[u8], i: usize) -> Option<(i32, usize)> {
    let b0 = *dict.get(i)?;
    if b0 == 28 {
        let v = i16::from_be_bytes([*dict.get(i + 1)?, *dict.get(i + 2)?]) as i32;
        return Some((v, 3));
    }
    if b0 == 29 {
        let v = i32::from_be_bytes([*dict.get(i + 1)?, *dict.get(i + 2)?, *dict.get(i + 3)?, *dict.get(i + 4)?]);
        return Some((v, 5));
    }
    if b0 == 30 {
        let mut s = String::new();
        let mut j = i + 1;
        loop {
            let b = *dict.get(j)?;
            j += 1;
            for nib in [b >> 4, b & 0x0f] {
                match nib {
                    0x0a => s.push('.'),
                    0x0b => s.push('E'),
                    0x0c => s.push_str("E-"),
                    0x0e => s.push('-'),
                    0x0f => return Some((s.parse::<f64>().ok()? as i32, j - i)),
                    n if n <= 9 => s.push(char::from(b'0' + n)),
                    _ => return None,
                }
            }
        }
    }
    if (32..=246).contains(&b0) {
        return Some((i32::from(b0) - 139, 1));
    }
    if (247..=250).contains(&b0) {
        let b1 = *dict.get(i + 1)?;
        return Some(((i32::from(b0) - 247) * 256 + i32::from(b1) + 108, 2));
    }
    if (251..=254).contains(&b0) {
        let b1 = *dict.get(i + 1)?;
        return Some((-(i32::from(b0) - 251) * 256 - i32::from(b1) - 108, 2));
    }
    if b0 == 255 {
        let hi = i16::from_be_bytes([*dict.get(i + 1)?, *dict.get(i + 2)?]) as i32;
        let lo = u16::from_be_bytes([*dict.get(i + 3)?, *dict.get(i + 4)?]) as i32;
        return Some((hi.saturating_mul(65536).saturating_add(lo), 5));
    }
    None
}

fn parse_cff_charset(cs: &[u8], num_glyphs: usize) -> Vec<u32> {
    let mut sid_for_glyph = vec![0u32; num_glyphs];
    if cs.is_empty() || num_glyphs <= 1 {
        return sid_for_glyph;
    }
    let fmt = cs[0];
    if fmt == 0 {
        let mut q = 1;
        for slot in sid_for_glyph.iter_mut().skip(1) {
            if q + 1 >= cs.len() {
                break;
            }
            *slot = u16::from_be_bytes([cs[q], cs[q + 1]]) as u32;
            q += 2;
        }
    } else if fmt == 1 {
        let mut q = 1;
        let mut g = 1;
        while q + 2 < cs.len() && g < num_glyphs {
            let first = u16::from_be_bytes([cs[q], cs[q + 1]]) as u32;
            let n_left = cs[q + 2] as u32;
            q += 3;
            for i in 0..=n_left {
                if g >= num_glyphs {
                    break;
                }
                sid_for_glyph[g] = first.wrapping_add(i);
                g += 1;
            }
        }
    } else if fmt == 2 {
        let mut q = 1;
        let mut g = 1;
        while q + 3 < cs.len() && g < num_glyphs {
            let first = u16::from_be_bytes([cs[q], cs[q + 1]]) as u32;
            let n_left = u16::from_be_bytes([cs[q + 2], cs[q + 3]]) as u32;
            q += 4;
            for i in 0..=n_left {
                if g >= num_glyphs {
                    break;
                }
                sid_for_glyph[g] = first.wrapping_add(i);
                g += 1;
            }
        }
    }
    sid_for_glyph
}

pub(super) fn parse_cff_gid_names(cff: &[u8]) -> Result<HashMap<u32, u32>> {
    if cff.len() < 4 {
        bail!("CFF 过短");
    }
    let mut pos = usize::from(cff[2]);
    let _name_idx = read_cff_index(cff, &mut pos)?;
    let top_idx = read_cff_index(cff, &mut pos)?;
    let string_idx = read_cff_index(cff, &mut pos)?;
    let top = parse_cff_dict(top_idx.first().copied().unwrap_or(&[]));
    let charset_off = top.iter().find(|(op, _)| *op == 15).and_then(|(_, ops)| ops.first()).copied();
    let charstrings_off = top.iter().find(|(op, _)| *op == 17).and_then(|(_, ops)| ops.first()).copied();
    let (Some(charset_off), Some(charstrings_off)) = (charset_off, charstrings_off) else {
        bail!("CFF 缺少 charset/CharStrings");
    };
    let charset_off = charset_off as usize;
    let charstrings_off = charstrings_off as usize;
    let num_glyphs = be_u16(cff, charstrings_off)? as usize;
    let charset = cff.get(charset_off..).ok_or_else(|| anyhow!("CFF charset 截断"))?;
    let sid_for_glyph = parse_cff_charset(charset, num_glyphs);
    let mut out = HashMap::new();
    for (g, sid) in sid_for_glyph.iter().enumerate().skip(1) {
        let idx = sid.saturating_sub(CFF_STD_STRINGS) as usize;
        if let Some(raw) = string_idx.get(idx) {
            let name: String = raw.iter().copied().map(char::from).collect();
            if let Some(digits) = name.strip_prefix("gid")
                && let Ok(vgid) = digits.parse::<u32>()
            {
                out.insert(g as u32, vgid);
            }
        }
    }
    if out.is_empty() {
        bail!("CFF 无 gidXXXXX 字形名");
    }
    Ok(out)
}
