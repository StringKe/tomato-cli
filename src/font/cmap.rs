use std::collections::HashMap;

use anyhow::{bail, Result};

use super::{be_u16, be_u32};

pub(super) fn parse_cmap(cmap: &[u8]) -> Result<HashMap<u32, u32>> {
    if cmap.len() < 4 {
        bail!("cmap 过短");
    }
    let num_tables = be_u16(cmap, 2)? as usize;
    let mut records = Vec::with_capacity(num_tables);
    let mut p = 4;
    for _ in 0..num_tables {
        let platform = be_u16(cmap, p)?;
        let encoding = be_u16(cmap, p + 2)?;
        let offset = be_u32(cmap, p + 4)? as usize;
        records.push((platform, encoding, offset));
        p += 8;
    }
    records.sort_by_key(|(platform, _, _)| match *platform {
        3 => 0u8,
        0 => 1,
        _ => 2,
    });
    for (_, _, offset) in records {
        if offset + 2 > cmap.len() {
            continue;
        }
        let format = be_u16(cmap, offset)?;
        let map = if format == 4 {
            parse_cmap_fmt4(cmap, offset)
        } else if format == 12 {
            parse_cmap_fmt12(cmap, offset)
        } else {
            continue;
        };
        if let Ok(map) = map
            && !map.is_empty()
        {
            return Ok(map);
        }
    }
    Ok(HashMap::new())
}

fn parse_cmap_fmt4(cmap: &[u8], base: usize) -> Result<HashMap<u32, u32>> {
    let seg_count = (be_u16(cmap, base + 6)? as usize) / 2;
    let mut q = base + 14;
    let mut end_codes = Vec::with_capacity(seg_count);
    for _ in 0..seg_count {
        end_codes.push(be_u16(cmap, q)?);
        q += 2;
    }
    q += 2;
    let mut start_codes = Vec::with_capacity(seg_count);
    for _ in 0..seg_count {
        start_codes.push(be_u16(cmap, q)?);
        q += 2;
    }
    let mut id_deltas = Vec::with_capacity(seg_count);
    for _ in 0..seg_count {
        id_deltas.push(be_u16(cmap, q)?);
        q += 2;
    }
    let id_range_pos = q;
    let mut id_range_offsets = Vec::with_capacity(seg_count);
    for _ in 0..seg_count {
        id_range_offsets.push(be_u16(cmap, q)?);
        q += 2;
    }
    let mut map = HashMap::new();
    for i in 0..seg_count {
        let start = start_codes[i] as u32;
        let end = end_codes[i] as u32;
        for c in start..=end {
            if c == 0xffff {
                continue;
            }
            let gid = if id_range_offsets[i] == 0 {
                (c.wrapping_add(u32::from(id_deltas[i]))) & 0xffff
            } else {
                let addr = id_range_pos + i * 2 + id_range_offsets[i] as usize + (c as usize - start_codes[i] as usize) * 2;
                let mut gid = be_u16(cmap, addr).unwrap_or(0) as u32;
                if gid != 0 {
                    gid = (gid.wrapping_add(u32::from(id_deltas[i]))) & 0xffff;
                }
                gid
            };
            if gid != 0 {
                map.entry(c).or_insert(gid);
            }
        }
    }
    Ok(map)
}

fn parse_cmap_fmt12(cmap: &[u8], base: usize) -> Result<HashMap<u32, u32>> {
    let n_groups = be_u32(cmap, base + 12)? as usize;
    let mut q = base + 16;
    let mut map = HashMap::new();
    for _ in 0..n_groups {
        let start_c = be_u32(cmap, q)?;
        let end_c = be_u32(cmap, q + 4)?;
        let start_gid = be_u32(cmap, q + 8)?;
        q += 12;
        for (i, c) in (start_c..=end_c).enumerate() {
            map.entry(c).or_insert(start_gid.wrapping_add(i as u32));
        }
    }
    Ok(map)
}
