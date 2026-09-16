use serde::{Deserialize, Serialize};

use crate::reflow::Reflow;
use crate::theme::ThemeId;

pub const UNGROUPED: &str = "未分组";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Book {
    pub book_id: String,
    pub title: String,
    pub author: String,
    pub abstract_text: String,
    pub category: String,
    pub chapter_count: u32,
    pub last_chapter_title: String,
    #[serde(default)]
    pub thumb_url: String,
    #[serde(default)]
    pub word_count: u64,
    #[serde(default)]
    pub read_count: u64,
    /// 番茄的 creation_status：0 已完结，1 连载中（书籍页 SSR 与页面「已完结」标签比对确认）。None 表示接口没给。
    #[serde(default)]
    pub creation_status: Option<i64>,
}

impl Book {
    pub fn from_id(book_id: &str) -> Self {
        Self { book_id: book_id.to_string(), title: book_id.to_string(), ..Self::default() }
    }

    pub fn status_label(&self) -> &'static str {
        creation_label(self.creation_status)
    }

    pub fn word_label(&self) -> String {
        word_label(self.word_count)
    }
}

pub fn creation_label(status: Option<i64>) -> &'static str {
    match status {
        Some(0) => "完结",
        Some(_) => "连载",
        None => "",
    }
}

pub fn word_label(words: u64) -> String {
    if words == 0 {
        String::new()
    } else if words >= 10_000 {
        format!("{}万字", words / 10_000)
    } else {
        format!("{words}字")
    }
}

/// 榜单分类，来自 /rank 页面 SSR。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankCategory {
    pub id: String,
    pub name: String,
    /// 榜单接口的 gender 参数：实测男频分类只在 1 下有数据，女频分类只在 0 下有数据，与页面 male / female 分组一致。
    pub gender: u8,
}

pub const GENDER_MALE: u8 = 1;
pub const GENDER_FEMALE: u8 = 0;

pub fn gender_label(gender: u8) -> &'static str {
    if gender == GENDER_MALE { "男频" } else { "女频" }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub item_id: String,
    pub title: String,
    pub need_pay: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterBody {
    pub item_id: String,
    pub book_id: String,
    pub book_name: String,
    pub title: String,
    pub content: String,
    pub pre_item_id: String,
    pub next_item_id: String,
    pub need_pay: bool,
    /// 阅读页 SSR 的 isChapterLock：未登录时为 true 且 content 只是截断预览，登录后服务端给全文并置 false。这是登录墙，不是付费墙。
    #[serde(default)]
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Progress {
    pub book_id: String,
    pub item_id: String,
    pub title: String,
    /// 折行后的行偏移，只在 anchor 缺失（旧记录、远端记录）时用。
    pub line: usize,
    /// 首行对应的正文位置（见 reader::WrapCache::anchors），不随宽度、整理模式、段落样式漂移。
    #[serde(default)]
    pub anchor: Option<usize>,
    pub updated_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct User {
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub desc: String,
    pub is_vip: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShelfItem {
    pub book_id: String,
    pub title: String,
    pub author: String,
    pub last_chapter_title: String,
    pub last_read_item_id: String,
    #[serde(default)]
    pub group_name: String,
    #[serde(default)]
    pub thumb_url: String,
    #[serde(default)]
    pub chapter_count: u32,
    #[serde(default)]
    pub creation_status: Option<i64>,
    /// 远端最新章节的 item_id，用于补全作者和最新章标题。
    #[serde(default)]
    pub last_chapter_item_id: String,
    /// 远端记录的最近阅读章节标题。
    #[serde(default)]
    pub last_read_title: String,
}

impl ShelfItem {
    pub fn folder(&self) -> &str {
        if self.group_name.trim().is_empty() {
            UNGROUPED
        } else {
            self.group_name.as_str()
        }
    }

    pub fn status_label(&self) -> &'static str {
        creation_label(self.creation_status)
    }

    /// 作者或最新章缺失时需要用阅读页补全。
    pub fn needs_meta(&self) -> bool {
        self.book_id != "demo" && !self.last_chapter_item_id.is_empty() && (self.author.is_empty() || self.last_chapter_title.is_empty())
    }

    pub fn to_book(&self) -> Book {
        Book {
            book_id: self.book_id.clone(),
            title: self.title.clone(),
            author: self.author.clone(),
            chapter_count: self.chapter_count,
            last_chapter_title: self.last_chapter_title.clone(),
            thumb_url: self.thumb_url.clone(),
            creation_status: self.creation_status,
            ..Book::default()
        }
    }

    /// 书籍元数据部分来自远端书架、部分来自阅读页，来源不同时只用 other 补空位，不覆盖已有值。
    pub fn fill_missing_from(&mut self, other: &ShelfItem) {
        for (mine, theirs) in [
            (&mut self.title, &other.title),
            (&mut self.author, &other.author),
            (&mut self.last_chapter_title, &other.last_chapter_title),
            (&mut self.last_read_item_id, &other.last_read_item_id),
            (&mut self.last_read_title, &other.last_read_title),
            (&mut self.group_name, &other.group_name),
            (&mut self.thumb_url, &other.thumb_url),
            (&mut self.last_chapter_item_id, &other.last_chapter_item_id),
        ] {
            if mine.is_empty() {
                mine.clone_from(theirs);
            }
        }
        if self.chapter_count == 0 {
            self.chapter_count = other.chapter_count;
        }
        if self.creation_status.is_none() {
            self.creation_status = other.creation_status;
        }
    }

    /// 从书籍元数据生成书架条目，阅读进度字段留空由调用方补。
    pub fn from_book(book: &Book) -> Self {
        Self {
            book_id: book.book_id.clone(),
            title: book.title.clone(),
            author: book.author.clone(),
            last_chapter_title: book.last_chapter_title.clone(),
            thumb_url: book.thumb_url.clone(),
            chapter_count: book.chapter_count,
            creation_status: book.creation_status,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
    #[default]
    Recent,
    Title,
    Author,
}

impl SortMode {
    pub fn name(self) -> &'static str {
        match self {
            SortMode::Recent => "最近阅读",
            SortMode::Title => "书名",
            SortMode::Author => "作者",
        }
    }

    pub fn next(self) -> Self {
        match self {
            SortMode::Recent => SortMode::Title,
            SortMode::Title => SortMode::Author,
            SortMode::Author => SortMode::Recent,
        }
    }
}

/// 阅读页伪装皮肤。只影响外观，与 theme 无关。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutId {
    #[default]
    Native,
    Claude,
    Codex,
    GitLog,
    Man,
    Grok,
}

impl LayoutId {
    pub const ALL: [LayoutId; 6] = [LayoutId::Native, LayoutId::Claude, LayoutId::Codex, LayoutId::GitLog, LayoutId::Man, LayoutId::Grok];

    pub fn name(self) -> &'static str {
        match self {
            LayoutId::Native => "关",
            LayoutId::Claude => "Claude Code",
            LayoutId::Codex => "Codex",
            LayoutId::GitLog => "git log",
            LayoutId::Man => "man",
            LayoutId::Grok => "Grok Build",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub theme: ThemeId,
    /// 阅读页伪装皮肤，Native 表示关闭。只影响外观，与 theme 无关。
    #[serde(default)]
    pub layout: LayoutId,
    #[serde(default)]
    pub text_width: u16,
    #[serde(default = "default_margin")]
    pub margin: u16,
    #[serde(default)]
    pub line_gap: u8,
    /// 碎行章节的换行整理策略。
    #[serde(default)]
    pub reflow: Reflow,
    /// 段首缩进两个全角空格。与段间空行是两个独立开关。
    #[serde(default = "default_true")]
    pub para_indent: bool,
    /// 段与段之间空一行。默认关：用户把「行距 0」理解成段落之间也没有空行。
    #[serde(default)]
    pub para_blank: bool,
    #[serde(default)]
    pub auto_page_ms: u64,
    #[serde(default = "default_true")]
    pub check_update: bool,
    #[serde(default)]
    pub sort: SortMode,
    /// 书籍页是否下载并显示封面。
    #[serde(default = "default_true")]
    pub show_covers: bool,
    /// 打开一章后提前拉取并缓存后面几章，0 关闭。
    #[serde(default = "default_prefetch")]
    pub prefetch: u8,
}

fn default_margin() -> u16 {
    1
}

fn default_true() -> bool {
    true
}

fn default_prefetch() -> u8 {
    5
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeId::Auto,
            layout: LayoutId::Native,
            text_width: 0,
            margin: 1,
            line_gap: 0,
            reflow: Reflow::Auto,
            para_indent: true,
            para_blank: false,
            auto_page_ms: 0,
            check_update: true,
            sort: SortMode::Recent,
            show_covers: true,
            prefetch: 5,
        }
    }
}

impl Settings {
    pub fn cycle_width(&mut self, dir: i32) {
        const VALS: [u16; 5] = [0, 40, 56, 72, 88];
        cycle_u16(&mut self.text_width, &VALS, dir);
    }

    pub fn cycle_margin(&mut self, dir: i32) {
        const VALS: [u16; 4] = [0, 1, 2, 4];
        cycle_u16(&mut self.margin, &VALS, dir);
    }

    pub fn cycle_gap(&mut self, dir: i32) {
        self.line_gap = match (self.line_gap, dir >= 0) {
            (0, true) => 1,
            (1, true) => 2,
            (2, true) => 0,
            (0, false) => 2,
            (1, false) => 0,
            _ => 1,
        };
    }

    pub fn cycle_reflow(&mut self, dir: i32) {
        self.reflow = if dir >= 0 { self.reflow.next() } else { self.reflow.prev() };
    }

    pub fn cycle_prefetch(&mut self, dir: i32) {
        const VALS: [u8; 4] = [0, 3, 5, 10];
        let i = VALS.iter().position(|v| *v == self.prefetch).unwrap_or(2) as i32;
        let next = (i + dir).rem_euclid(VALS.len() as i32) as usize;
        self.prefetch = VALS[next];
    }

    pub fn prefetch_label(&self) -> String {
        if self.prefetch == 0 { "关".into() } else { format!("{} 章", self.prefetch) }
    }

    pub fn cycle_auto(&mut self, dir: i32) {
        const VALS: [u64; 4] = [0, 2000, 4000, 8000];
        let i = VALS.iter().position(|v| *v == self.auto_page_ms).unwrap_or(0) as i32;
        let next = (i + dir).rem_euclid(VALS.len() as i32) as usize;
        self.auto_page_ms = VALS[next];
    }

    pub fn width_label(&self) -> String {
        if self.text_width == 0 {
            "自适应".into()
        } else {
            format!("{}", self.text_width)
        }
    }

    pub fn auto_label(&self) -> String {
        if self.auto_page_ms == 0 {
            "关".into()
        } else {
            format!("{}s", self.auto_page_ms / 1000)
        }
    }
}

fn cycle_u16(slot: &mut u16, vals: &[u16], dir: i32) {
    let i = vals.iter().position(|v| *v == *slot).unwrap_or(0) as i32;
    let next = (i + dir).rem_euclid(vals.len() as i32) as usize;
    *slot = vals[next];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_cycle_wraps() {
        let mut id = LayoutId::Native;
        for _ in 0..LayoutId::ALL.len() {
            id = id.next();
        }
        assert_eq!(id, LayoutId::Native);
        assert_eq!(LayoutId::ALL.len(), 6);
        let mut id = LayoutId::Native;
        for _ in 0..LayoutId::ALL.len() {
            id = id.prev();
        }
        assert_eq!(id, LayoutId::Native);
        assert_eq!(LayoutId::Native.prev(), LayoutId::Grok);
        assert_eq!(LayoutId::Grok.next(), LayoutId::Native);
    }

    #[test]
    fn settings_without_layout_field_defaults_to_native() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.layout, LayoutId::Native);
    }

    #[test]
    fn settings_without_reflow_fields_default_to_auto_indent_no_blank() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.reflow, Reflow::Auto);
        assert!(s.para_indent);
        assert!(!s.para_blank);
        assert_eq!((Settings::default().para_indent, Settings::default().para_blank), (true, false));
        // 旧 state.json 里的 para_style 字段被忽略，不影响读取。
        let old: Settings = serde_json::from_str(r#"{"para_style":"Indent"}"#).unwrap();
        assert!(old.para_indent);
    }
}
