//! 书架整理规则：按目录、已读列表和阅读时间判断一本书没开始、在读、读完、读完后又更新还是弃读，给出它该在的文件夹。
//! 这里只有纯函数，拉数据、移动和同步在 app 层。

use std::cmp::Ordering;
use std::collections::HashSet;

use crate::model::{ReadKind, ReadStats, ShelfItem, UNGROUPED};

/// 读到了当时的最新章、之后又新出了不少章的书。
pub const UPDATED: &str = "有更新";
/// 没读完且很久没读的书。
pub const ABANDONED: &str = "弃读";
/// 用户在手机上手动建的「读完」分组。整理时和年份文件夹一样归规则管，里面的书按读完年份拆开。
const LEGACY_FINISHED: &[&str] = &["完毕"];
const DAY_MS: u64 = 86_400_000;
/// 番茄是国内服务，读完年份按北京时间算。
const CN_OFFSET_MS: u64 = 8 * 3_600_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub kind: ReadKind,
    /// 目标文件夹，空串是「默认」。
    pub folder: String,
    /// 读完后新出的章数。
    pub new: u32,
}

/// 一次整理里要移动的一本书。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub book_id: String,
    pub title: String,
    /// 原分组名，空串是「默认」。
    pub from: String,
    pub to: String,
    pub new: u32,
}

/// 末尾允许没读、读完后允许再出的章数。完本感言和番外通常就这么几章。
pub fn tolerance(total: u32) -> u32 {
    (total / 100).max(3)
}

/// toc 是目录顺序的 (item_id, 发布时间秒)，read 是服务端已读列表，local 是本地阅读记录 (item_id, 毫秒)。
/// 已读章数不能判断读完：跳着读、换设备读都会让它偏少，所以看读到的最远一章。
/// 阅读时间取书架最后操作时间、本地记录时间和最远一章发布时间的最大值：最后操作时间有时比读过的章节发布得还早。
pub fn measure(toc: &[(String, u64)], read: &HashSet<String>, local: Option<(&str, u64)>, operated_ms: u64, now_ms: u64) -> ReadStats {
    let local_item = local.map(|(id, _)| id);
    let mut furthest = 0;
    let mut read_n = 0;
    for (i, (id, _)) in toc.iter().enumerate() {
        if read.contains(id) || local_item == Some(id.as_str()) {
            read_n += 1;
            furthest = i + 1;
        }
    }
    let furthest_ms = if furthest > 0 { toc[furthest - 1].1.saturating_mul(1000) } else { 0 };
    let read_ms = operated_ms.max(local.map_or(0, |(_, t)| t)).max(furthest_ms);
    let (mut skipped, mut new_after) = (0, 0);
    for (_, published) in &toc[furthest..] {
        if published.saturating_mul(1000) > read_ms {
            new_after += 1;
        } else {
            skipped += 1;
        }
    }
    ReadStats {
        total: toc.len() as u32,
        furthest: furthest as u32,
        read_n,
        read_ms,
        skipped,
        new_after,
        toc_last: toc.last().map(|(id, _)| id.clone()).unwrap_or_default(),
        last_item: String::new(),
        fetched_ms: now_ms,
    }
}

/// 按顺序判定，命中即停：没开始、读到最后一章、读完后又出了较多新章、最近读过、弃读。目录为空时不判定。
/// 读到了最新章的连载书算在读：书没完结，放进读完年份不合适。
pub fn classify(s: &ReadStats, creation_status: Option<i64>, now_ms: u64, abandon_days: u16) -> Option<Verdict> {
    if s.total == 0 {
        return None;
    }
    if s.read_n <= 1 {
        return Some(Verdict { kind: ReadKind::NotStarted, folder: String::new(), new: 0 });
    }
    let tol = tolerance(s.total);
    let caught_up = s.skipped <= tol;
    let new = if caught_up { s.new_after } else { 0 };
    let (kind, folder) = if caught_up && s.new_after <= tol {
        if creation_status == Some(1) { (ReadKind::Reading, String::new()) } else { (ReadKind::Finished, year_of(s.read_ms).to_string()) }
    } else if caught_up {
        (ReadKind::Updated, UPDATED.to_string())
    } else if now_ms.saturating_sub(s.read_ms) <= u64::from(abandon_days) * DAY_MS {
        (ReadKind::Reading, String::new())
    } else {
        (ReadKind::Abandoned, ABANDONED.to_string())
    };
    Some(Verdict { kind, folder, new })
}

/// 卡片上「新 N 章」的 N：读到了当时的最新章之后又发布的章数。
pub fn new_chapters(s: &ReadStats) -> u32 {
    if s.read_n > 1 && s.skipped <= tolerance(s.total) { s.new_after } else { 0 }
}

pub fn is_year(name: &str) -> bool {
    name.len() == 4 && name.bytes().all(|b| b.is_ascii_digit()) && (name.starts_with("19") || name.starts_with("20"))
}

/// 归整理规则管的文件夹。用户自建的其他文件夹里的书整理时不动。
pub fn is_managed(folder: &str) -> bool {
    let folder = folder.trim();
    folder.is_empty() || folder == UNGROUPED || folder == UPDATED || folder == ABANDONED || is_year(folder) || LEGACY_FINISHED.contains(&folder)
}

/// 书架标签顺序：默认、有更新、年份从新到旧、弃读，然后是其他文件夹按名字排。
pub fn folder_order(a: &str, b: &str) -> Ordering {
    fn rank(name: &str) -> u8 {
        match name {
            UNGROUPED => 0,
            UPDATED => 1,
            _ if is_year(name) => 2,
            ABANDONED => 3,
            _ => 4,
        }
    }
    rank(a).cmp(&rank(b)).then_with(|| if rank(a) == 2 { b.cmp(a) } else { a.cmp(b) })
}

/// 整理清单。只看受管文件夹里、有阅读数据的书，演示书不动。
/// 手动移动过的书在阅读状态变化之前不动；状态变了就解除手动标记，交回规则。手动标记没记下状态的（移动时还没整理过），以这次的状态为准。
pub fn plan(shelf: &mut [ShelfItem], now_ms: u64, abandon_days: u16) -> Vec<Move> {
    let mut moves = Vec::new();
    for item in shelf.iter_mut() {
        if item.book_id == "demo" || !is_managed(&item.group_name) {
            continue;
        }
        let Some(v) = item.read_stats.as_ref().and_then(|s| classify(s, item.creation_status, now_ms, abandon_days)) else { continue };
        if item.pinned {
            match item.pinned_kind {
                None => {
                    item.pinned_kind = Some(v.kind);
                    continue;
                }
                Some(k) if k == v.kind => continue,
                Some(_) => {
                    item.pinned = false;
                    item.pinned_kind = None;
                }
            }
        }
        let from = item.group_name.trim();
        let from = if from == UNGROUPED { "" } else { from };
        if from != v.folder {
            moves.push(Move { book_id: item.book_id.clone(), title: item.title.clone(), from: from.to_string(), to: v.folder, new: v.new });
        }
    }
    moves
}

/// 手动移动时记下当前的阅读状态，自动整理据此判断状态有没有变。
pub fn pin(item: &mut ShelfItem, now_ms: u64, abandon_days: u16) {
    item.pinned = true;
    item.pinned_kind = item.read_stats.as_ref().and_then(|s| classify(s, item.creation_status, now_ms, abandon_days)).map(|v| v.kind);
}

/// 毫秒时间戳在北京时间下的年份。
pub fn year_of(ms: u64) -> i64 {
    let days = ((ms + CN_OFFSET_MS) / DAY_MS) as i64;
    // 公历日期换算（Howard Hinnant 的 civil_from_days），只取年份。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    yoe + era * 400 + i64::from(mp >= 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = DAY_MS;
    /// 2025-06-01 00:00 北京时间。
    const JUNE_2025: u64 = 1_748_707_200_000;

    fn toc(n: usize, published_s: impl Fn(usize) -> u64) -> Vec<(String, u64)> {
        (0..n).map(|i| (format!("c{i}"), published_s(i))).collect()
    }

    fn read(ids: impl IntoIterator<Item = usize>) -> HashSet<String> {
        ids.into_iter().map(|i| format!("c{i}")).collect()
    }

    fn stats(total: u32, read_n: u32, skipped: u32, new_after: u32, read_ms: u64) -> ReadStats {
        ReadStats { total, furthest: total - skipped - new_after, read_n, read_ms, skipped, new_after, ..ReadStats::default() }
    }

    #[test]
    fn year_uses_beijing_time() {
        assert_eq!(year_of(0), 1970);
        // 2025-01-01 00:00 北京时间 = 2024-12-31 16:00 UTC。
        assert_eq!(year_of(1_735_660_800_000), 2025);
        assert_eq!(year_of(1_735_660_799_000), 2024);
        assert_eq!(year_of(JUNE_2025), 2025);
        assert_eq!(year_of(951_753_600_000), 2000);
    }

    #[test]
    fn measure_uses_furthest_chapter_not_read_count() {
        let base = JUNE_2025 / 1000;
        let t = toc(10, |i| base + i as u64 * 86_400);
        let s = measure(&t, &read([0, 1, 7]), None, 0, JUNE_2025);
        assert_eq!((s.total, s.furthest, s.read_n), (10, 8, 3));
        // 阅读时间至少是最远一章的发布时间；之后两章都在那之后才发布。
        assert_eq!(s.read_ms, (base + 7 * 86_400) * 1000);
        assert_eq!((s.skipped, s.new_after), (0, 2));
        assert_eq!(s.toc_last, "c9");
    }

    #[test]
    fn measure_counts_local_progress_and_later_operate_time() {
        let base = JUNE_2025 / 1000;
        let t = toc(10, |_| base);
        let late = JUNE_2025 + 30 * DAY;
        let s = measure(&t, &read([0]), Some(("c9", late)), 0, late);
        assert_eq!((s.furthest, s.read_n, s.read_ms), (10, 2, late));
        let s = measure(&t, &read([0, 4]), None, late, late);
        assert_eq!((s.furthest, s.read_ms, s.skipped, s.new_after), (5, late, 5, 0));
    }

    #[test]
    fn classify_follows_rule_order() {
        let now = JUNE_2025 + 10 * DAY;
        let recent = JUNE_2025;
        let old = JUNE_2025 - 200 * DAY;
        let kind = |s: &ReadStats, cs| classify(s, cs, now, 90).map(|v| (v.kind, v.folder, v.new));
        assert_eq!(kind(&stats(500, 1, 400, 99, recent), Some(0)), Some((ReadKind::NotStarted, String::new(), 0)));
        assert_eq!(kind(&stats(500, 480, 0, 0, old), Some(0)), Some((ReadKind::Finished, "2024".into(), 0)));
        // 500 章的容差是 5 章：末尾差几章、读完后新出几章都算读完，新出的章数照样报出来。
        assert_eq!(kind(&stats(500, 400, 3, 1, recent), Some(0)), Some((ReadKind::Finished, "2025".into(), 1)));
        assert_eq!(kind(&stats(500, 400, 0, 37, old), Some(0)), Some((ReadKind::Updated, UPDATED.into(), 37)));
        assert_eq!(kind(&stats(500, 300, 100, 0, recent), Some(0)), Some((ReadKind::Reading, String::new(), 0)));
        assert_eq!(kind(&stats(500, 300, 100, 0, old), Some(0)), Some((ReadKind::Abandoned, ABANDONED.into(), 0)));
        // 追到最新的连载书不进年份文件夹。
        assert_eq!(kind(&stats(500, 480, 0, 0, old), Some(1)), Some((ReadKind::Reading, String::new(), 0)));
        assert_eq!(classify(&ReadStats::default(), Some(0), now, 90), None);
    }

    #[test]
    fn tolerance_scales_with_length() {
        assert_eq!(tolerance(200), 3);
        assert_eq!(tolerance(1501), 15);
        let now = JUNE_2025;
        let old = JUNE_2025 - 400 * DAY;
        // 1501 章差 5 章按 1% 算读完，486 章差 25 章不算。
        assert_eq!(classify(&stats(1501, 689, 5, 0, old), Some(0), now, 90).unwrap().kind, ReadKind::Finished);
        assert_eq!(classify(&stats(486, 374, 25, 0, old), Some(0), now, 90).unwrap().kind, ReadKind::Abandoned);
    }

    #[test]
    fn abandon_threshold_is_a_setting() {
        let s = stats(500, 300, 100, 0, JUNE_2025);
        assert_eq!(classify(&s, Some(0), JUNE_2025 + 60 * DAY, 90).unwrap().kind, ReadKind::Reading);
        assert_eq!(classify(&s, Some(0), JUNE_2025 + 60 * DAY, 30).unwrap().kind, ReadKind::Abandoned);
    }

    #[test]
    fn managed_folders_and_tab_order() {
        for f in ["", UNGROUPED, UPDATED, ABANDONED, "2025", "完毕"] {
            assert!(is_managed(f), "{f}");
        }
        for f in ["演示", "收藏", "20250", "3025"] {
            assert!(!is_managed(f), "{f}");
        }
        let mut names = vec!["收藏", ABANDONED, "2024", UNGROUPED, "2026", UPDATED, "演示"];
        names.sort_by(|a, b| folder_order(a, b));
        assert_eq!(names, vec![UNGROUPED, UPDATED, "2026", "2024", ABANDONED, "收藏", "演示"]);
    }

    fn item(id: &str, group: &str, s: ReadStats) -> ShelfItem {
        ShelfItem { book_id: id.into(), title: id.into(), group_name: group.into(), creation_status: Some(0), read_stats: Some(s), ..ShelfItem::default() }
    }

    #[test]
    fn plan_moves_managed_books_and_leaves_custom_folders() {
        let now = JUNE_2025 + 10 * DAY;
        let finished = stats(500, 480, 0, 0, JUNE_2025);
        let mut shelf = vec![item("a", "完毕", finished.clone()), item("b", "", finished.clone()), item("c", "收藏", finished.clone()), item("d", "2025", finished.clone()), ShelfItem { book_id: "e".into(), ..ShelfItem::default() }, item("demo", "", finished)];
        let moves = plan(&mut shelf, now, 90);
        let got: Vec<(&str, &str, &str)> = moves.iter().map(|m| (m.book_id.as_str(), m.from.as_str(), m.to.as_str())).collect();
        assert_eq!(got, vec![("a", "完毕", "2025"), ("b", "", "2025")]);
    }

    #[test]
    fn pinned_book_stays_until_its_state_changes() {
        let now = JUNE_2025 + 10 * DAY;
        let reading = stats(500, 300, 100, 0, JUNE_2025);
        let mut shelf = vec![item("a", ABANDONED, reading)];
        pin(&mut shelf[0], now, 90);
        assert_eq!(shelf[0].pinned_kind, Some(ReadKind::Reading));
        assert!(plan(&mut shelf, now, 90).is_empty());
        // 读完了：状态变了，手动标记解除，交回规则。
        shelf[0].read_stats = Some(stats(500, 480, 0, 0, JUNE_2025));
        let moves = plan(&mut shelf, now, 90);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].to, "2025");
        assert!(!shelf[0].pinned);
    }

    #[test]
    fn pin_without_stats_takes_next_state_as_baseline() {
        let now = JUNE_2025 + 10 * DAY;
        let mut shelf = vec![ShelfItem { book_id: "a".into(), group_name: ABANDONED.into(), ..ShelfItem::default() }];
        pin(&mut shelf[0], now, 90);
        assert_eq!(shelf[0].pinned_kind, None);
        shelf[0].read_stats = Some(stats(500, 300, 100, 0, JUNE_2025));
        assert!(plan(&mut shelf, now, 90).is_empty());
        assert_eq!(shelf[0].pinned_kind, Some(ReadKind::Reading));
    }

    #[test]
    fn new_chapter_badge_only_when_caught_up() {
        assert_eq!(new_chapters(&stats(500, 400, 0, 37, JUNE_2025)), 37);
        assert_eq!(new_chapters(&stats(500, 300, 100, 37, JUNE_2025)), 0);
        assert_eq!(new_chapters(&stats(500, 1, 0, 37, JUNE_2025)), 0);
    }
}
