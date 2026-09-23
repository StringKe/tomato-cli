use std::collections::{HashMap, HashSet};
use std::io::{self, Stdout, stdout};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::crossterm::event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
use ratatui::layout::Rect;
use ratatui::widgets::ListState;
use ratatui_image::picker::Picker;
use ratatui_image::picker::cap_parser::QueryStdioOptions;
use ratatui_image::protocol::StatefulProtocol;

use crate::api::Client;
use crate::model::{Book, Chapter, ChapterBody, GENDER_MALE, LayoutId, RankCategory};
use crate::reader::{Deco, WrapCache, WrapOpts};
use crate::store::{self, State};

mod actions;
mod backend;
mod hints;
mod keys;
mod keys_more;
mod mouse;
mod nav;
mod rank;
mod workers;

pub(crate) use hints::{HELP_SECTIONS, HOME_ITEMS, Hint, LOGIN_ITEMS, PROFILE_ITEMS};
#[cfg(test)]
pub(crate) use hints::hint;
use workers::WorkerMsg;

const HOME_LEN: usize = HOME_ITEMS.len();
const SETTINGS_LEN: usize = 14;
/// 书架和搜索结果每项占的行数：标题一行、两行信息，项与项之间空一行。鼠标命中按 `App::card_stride` 换算。
pub(crate) const CARD_H: u16 = 3;
pub(crate) const CARD_GAP: u16 = 1;
/// 终端支持真正的图片协议时卡片左侧放缩略封面：4 行高在 kitty / iTerm2 下约 64 px，3:4 的封面占 6 列。半块字符 4 行只有 8 px，看不出内容，不放。
pub(crate) const CARD_COVER_H: u16 = 4;
pub(crate) const CARD_COVER_W: u16 = 8;
const LOGIN_LEN: usize = LOGIN_ITEMS.len();
const PROFILE_LEN: usize = PROFILE_ITEMS.len();

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Home,
    Shelf,
    Search,
    Rank,
    Book,
    Reader,
    Login,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Overlay {
    None,
    Help,
    Toc,
    Jump,
    FolderPick,
    FolderInput,
    Cookie,
    Settings,
    Profile,
}

pub(crate) struct OpenBook {
    pub book: Book,
    pub chapters: Vec<Chapter>,
    pub list: ListState,
    pub filter: String,
    pub demo_bodies: Vec<ChapterBody>,
}

pub(crate) struct ReaderSession {
    pub book: Book,
    pub chapters: Vec<Chapter>,
    pub index: usize,
    pub body: ChapterBody,
    pub cache: WrapCache,
    pub offset: usize,
    /// 下一次折行后要回到的正文位置：来自阅读记录，或折行参数改变前记下的当前首行。
    pub pending_anchor: Option<usize>,
    /// 上一帧正文区的行数，绘制时回写；翻页步长和「到底了」判断都靠它。
    pub view_height: usize,
    pub demo_bodies: Vec<ChapterBody>,
}

impl ReaderSession {
    /// 最后一行已经在屏幕内。
    pub fn at_end(&self) -> bool {
        self.offset.saturating_add(self.view_height) >= self.cache.len()
    }

    /// 按当前宽度和参数折行，并把待恢复的正文位置换算成行偏移。终端改宽或设置变化导致重折时同样按正文位置回到原处。
    pub fn prepare_wrap(&mut self, width: u16, opts: WrapOpts) {
        let before = self.cache.anchor_of(self.offset);
        let rewrapped = self.cache.ensure(width, opts);
        if self.cache.len() == 0 {
            return;
        }
        if let Some(a) = self.pending_anchor.take() {
            self.offset = self.cache.line_of(a);
        } else if rewrapped && let Some(a) = before {
            self.offset = self.cache.line_of(a);
        }
    }
}

pub struct App {
    pub(crate) client: Client,
    pub(crate) state: State,
    pub(crate) screens: Vec<Screen>,
    pub(crate) overlay: Overlay,
    pub(crate) dirty: bool,
    pub(crate) should_quit: bool,
    pub(crate) status: String,
    pub(crate) update_hint: Option<String>,
    pub(crate) busy: bool,
    pub(crate) shelf_list: ListState,
    pub(crate) folder_idx: usize,
    pub(crate) search_input: String,
    pub(crate) search_results: Vec<Book>,
    pub(crate) search_list: ListState,
    /// 当前结果对应的关键词、下一页 offset 和是否还有下一页；输入框内容可能已被改动，翻页要按这里的关键词发。
    pub(crate) search_query: String,
    pub(crate) search_offset: u32,
    pub(crate) search_has_more: bool,
    /// 榜单分类（男女频合在一起，按 rank_gender 过滤），rank_cat_idx 是当前性别分类列表里的下标。
    pub(crate) rank_cats: Vec<RankCategory>,
    pub(crate) rank_cat_idx: usize,
    pub(crate) rank_gender: u8,
    pub(crate) rank_books: Vec<Book>,
    pub(crate) rank_list: ListState,
    /// 当前分类榜单的总条数，rank_books 不足它时到末尾自动加载下一页。
    pub(crate) rank_total: u32,
    pub(crate) open: Option<OpenBook>,
    /// 终端图片协议探测结果，进入 TUI 后由 from_query_stdio 覆盖。
    pub(crate) picker: Picker,
    /// 已解码的封面，按 book_id 缓存；StatefulProtocol 在渲染时按区域缩放。
    pub(crate) covers: HashMap<String, StatefulProtocol>,
    pub(crate) cover_requested: HashSet<String>,
    /// 下载或解码失败过的封面，本次会话不再自动重试，否则离线时卡片每帧都会重新发请求；刷新书架或重开封面设置时清空。
    pub(crate) cover_failed: HashSet<String>,
    /// 正在下载或解码的封面数，用来决定事件循环要不要保持短超时。
    pub(crate) cover_inflight: usize,
    /// 书架补全任务是否在跑。
    pub(crate) hydrating: bool,
    /// 绘制时发现还没加载的封面 (book_id, url)，帧结束后由 flush_cover_requests 发起。
    pub(crate) cover_wanted: Vec<(String, String)>,
    /// 从书架按「继续阅读」打开时置位：目录到手后直接进入上次读到的章节。
    pub(crate) resume_on_open: bool,
    /// 当前已发给终端的窗口标题。
    pub(crate) title_shown: String,
    /// 正在后台预读的章节 item_id，避免重复请求。
    pub(crate) prefetching: HashSet<String>,
    /// 预读拉取失败的章节：不再排进预读队列，否则失败章会被无限重试；每次切章、换书或登录后清空。
    pub(crate) prefetch_failed: HashSet<String>,
    /// 用户要打开的章节正好在预读中：记下来等预读落盘直接用，不重复发同一个请求。
    pub(crate) pending_open: Option<String>,
    /// 这一帧画了图片的区域，ui::draw 开头清空、画图片时追加。
    pub(crate) image_rects: Vec<Rect>,
    /// 上一帧的浮层区域。浮层压过图片后关闭或挪动时，图片那些格子不会被差分刷新重写，要整屏重画。
    pub(crate) shown_overlay: Rect,
    pub(crate) reader: Option<ReaderSession>,
    /// 是否处于伪装态。运行时状态不落盘：老板键切换，进入阅读页时按 settings.layout 置位。
    pub(crate) cover: bool,
    /// 伪装态状态行上的一次性英文提示和它出现的时间，到时由 tick_cover_note 撤掉。
    pub(crate) cover_note: Option<(String, Instant)>,
    /// 伪装态工作行的起点（busy 开始的时间），耗时和 spinner 都按它算；不 busy 时为 None。
    pub(crate) cover_work_since: Option<Instant>,
    /// 上一次为 spinner 动画置脏的时间。
    pub(crate) last_anim: Instant,
    pub(crate) settings_idx: usize,
    pub(crate) login_qr: String,
    pub(crate) login_msg: String,
    pub(crate) toc_filter: String,
    pub(crate) toc_list: ListState,
    pub(crate) jump_buf: String,
    pub(crate) folder_buf: String,
    pub(crate) folder_rename: bool,
    pub(crate) last_auto: Instant,
    /// 上一次翻页按键的时间，用来判断是不是连按。
    pub(crate) last_page: Instant,
    pub(crate) home_list: ListState,
    pub(crate) cookie_buf: String,
    pub(crate) list_area: Rect,
    pub(crate) folder_tabs: Vec<(Rect, usize)>,
    pub(crate) menu_hits: Vec<(Rect, usize)>,
    pub(crate) help_hit: Rect,
    pub(crate) profile_hit: Rect,
    pub(crate) overlay_area: Rect,
    pub(crate) settings_list: ListState,
    pub(crate) login_list: ListState,
    pub(crate) profile_list: ListState,
    last_click: Option<(u16, u16, Instant, usize)>,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
    rt: tokio::runtime::Runtime,
}

fn list_at(i: usize) -> ListState {
    let mut s = ListState::default();
    s.select(Some(i));
    s
}

pub fn run() -> Result<()> {
    crate::theme::probe_terminal();
    let state = store::load().unwrap_or_default();
    let client = Client::new(state.cookies.clone())?;
    let rt = tokio::runtime::Runtime::new().context("启动 tokio")?;
    let (tx, rx) = mpsc::channel();
    let logged_in = state.user.is_some();
    let mut app = App {
        client,
        state,
        screens: vec![if logged_in { Screen::Shelf } else { Screen::Home }],
        overlay: Overlay::None,
        dirty: true,
        should_quit: false,
        status: String::new(),
        update_hint: None,
        busy: false,
        shelf_list: ListState::default(),
        folder_idx: 0,
        search_input: String::new(),
        search_results: Vec::new(),
        search_list: ListState::default(),
        search_query: String::new(),
        search_offset: 0,
        search_has_more: false,
        rank_cats: Vec::new(),
        rank_cat_idx: 0,
        rank_gender: GENDER_MALE,
        rank_books: Vec::new(),
        rank_list: ListState::default(),
        rank_total: 0,
        open: None,
        picker: Picker::halfblocks(),
        covers: HashMap::new(),
        cover_requested: HashSet::new(),
        cover_failed: HashSet::new(),
        cover_inflight: 0,
        hydrating: false,
        cover_wanted: Vec::new(),
        resume_on_open: false,
        title_shown: String::new(),
        prefetching: HashSet::new(),
        prefetch_failed: HashSet::new(),
        pending_open: None,
        image_rects: Vec::new(),
        shown_overlay: Rect::default(),
        reader: None,
        cover: false,
        cover_note: None,
        cover_work_since: None,
        last_anim: Instant::now(),
        settings_idx: 0,
        login_qr: String::new(),
        login_msg: String::new(),
        toc_filter: String::new(),
        toc_list: ListState::default(),
        jump_buf: String::new(),
        folder_buf: String::new(),
        folder_rename: false,
        last_auto: Instant::now(),
        last_page: Instant::now(),
        home_list: list_at(0),
        cookie_buf: String::new(),
        list_area: Rect::default(),
        folder_tabs: Vec::new(),
        menu_hits: Vec::new(),
        help_hit: Rect::default(),
        profile_hit: Rect::default(),
        overlay_area: Rect::default(),
        settings_list: list_at(0),
        login_list: list_at(0),
        profile_list: list_at(0),
        last_click: None,
        tx,
        rx,
        rt,
    };
    app.sync_shelf_select();
    let _ = store::save(&app.state);
    if app.state.settings.check_update {
        app.spawn_update_check();
    }
    app.spawn_fontmap_refresh();
    if app.state.user.is_some() {
        app.spawn_shelf_refresh();
    } else if !app.state.cookies.is_empty() {
        app.spawn_restore_session();
    }

    let mut terminal = init_terminal()?;
    app.picker = probe_picker(app.picker);
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste)?;
    let result = app.loop_ui(&mut terminal);
    execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste).ok();
    ratatui::restore();
    result
}

impl App {
    pub(crate) fn screen(&self) -> Screen {
        *self.screens.last().unwrap_or(&Screen::Shelf)
    }

    /// 伪装态生效条件：cover 置位且选了皮肤。任何屏幕都可能为真。
    pub(crate) fn covered(&self) -> bool {
        self.cover && self.state.settings.layout != LayoutId::Native
    }

    /// 书架和搜索结果卡片是否带缩略封面。
    pub(crate) fn card_covers(&self) -> bool {
        self.state.settings.show_covers && self.picker.protocol_type() != ratatui_image::picker::ProtocolType::Halfblocks
    }

    /// 原生阅读页的折行参数，全部来自设置。
    pub(crate) fn wrap_opts(&self) -> WrapOpts {
        let s = &self.state.settings;
        WrapOpts { line_gap: s.line_gap, reflow: s.reflow, para_indent: s.para_indent, para_blank: s.para_blank, deco: Deco::None }
    }

    pub(crate) fn card_height(&self) -> u16 {
        if self.card_covers() { CARD_COVER_H } else { CARD_H }
    }

    pub(crate) fn card_stride(&self) -> u16 {
        self.card_height() + CARD_GAP
    }
}

/// 与 ratatui::init 相同的步骤（panic 时先恢复终端、raw mode、备用屏），只是换成宽字符感知的后端。
fn init_terminal() -> Result<Terminal<backend::WideBackend<Stdout>>> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        hook(info);
    }));
    enable_raw_mode().context("进入 raw mode")?;
    execute!(stdout(), EnterAlternateScreen).context("进入备用屏")?;
    Terminal::new(backend::WideBackend::new(stdout())).context("创建终端")
}

/// 终端不回应查询时等多久。本地终端不到 1ms 就回应，SSH 一个往返也远小于它；库默认的 2 秒会让 ConPTY 这类不回应的终端首帧空等 2 秒。
const PICKER_QUERY_TIMEOUT: Duration = Duration::from_millis(500);

/// 图片协议探测。要在进入备用屏之后、读取事件之前做，因为它向终端发查询并读 stdin。
/// tmux / screen 默认不转发透传查询，探测线程会一直阻塞在 stdin 上并吞掉用户的第一个按键，所以这两种环境直接用半块字符。
/// `fallback` 是已构造好的半块 Picker：构造它在 tmux 里要 spawn 一次 `tmux set`，不重复造。
fn probe_picker(fallback: Picker) -> Picker {
    let term = std::env::var("TERM").unwrap_or_default();
    let multiplexed = std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some() || term.starts_with("tmux") || term.starts_with("screen");
    if multiplexed {
        return fallback;
    }
    Picker::from_query_stdio_with_options(QueryStdioOptions { timeout: PICKER_QUERY_TIMEOUT, ..Default::default() }).unwrap_or(fallback)
}

fn point_in(area: Rect, col: u16, row: u16) -> bool {
    col >= area.x && col < area.x.saturating_add(area.width) && row >= area.y && row < area.y.saturating_add(area.height)
}

fn click_hits(hits: &[(Rect, usize)], col: u16, row: u16) -> Option<usize> {
    hits.iter().find(|(rect, _)| point_in(*rect, col, row)).map(|(_, i)| *i)
}

fn list_index_at(area: Rect, col: u16, row: u16, len: usize, offset: usize) -> Option<usize> {
    card_index_at(area, col, row, len, offset, 1)
}

/// 每项占 item_h 行的列表命中。ui 传入 inner 内容区，第一行就是 index offset。
fn card_index_at(area: Rect, col: u16, row: u16, len: usize, offset: usize, item_h: u16) -> Option<usize> {
    if !point_in(area, col, row) || len == 0 {
        return None;
    }
    let idx = offset.saturating_add(((row - area.y) / item_h.max(1)) as usize);
    if idx < len { Some(idx) } else { None }
}

/// 按标题过滤后的章节，附带原始序号。展示格式由 ui 决定。
pub(crate) fn filtered_chapters<'a>(chapters: &'a [Chapter], filter: &str) -> Vec<(usize, &'a Chapter)> {
    chapters.iter().enumerate().filter(|(_, c)| filter.is_empty() || c.title.contains(filter)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_index_uses_content_area_and_offset() {
        let area = Rect { x: 2, y: 3, width: 20, height: 8 };
        assert_eq!(list_index_at(area, 3, 3, 20, 0), Some(0));
        assert_eq!(list_index_at(area, 3, 4, 20, 0), Some(1));
        assert_eq!(list_index_at(area, 3, 4, 20, 6), Some(7));
        assert_eq!(list_index_at(area, 1, 4, 20, 0), None);
        assert_eq!(list_index_at(area, 3, 10, 20, 0), Some(7));
        assert_eq!(list_index_at(area, 3, 11, 20, 0), None);
    }

    #[test]
    fn card_index_groups_rows_by_item_height() {
        let area = Rect { x: 2, y: 3, width: 20, height: 12 };
        assert_eq!(card_index_at(area, 3, 3, 5, 0, 4), Some(0));
        assert_eq!(card_index_at(area, 3, 6, 5, 0, 4), Some(0));
        assert_eq!(card_index_at(area, 3, 7, 5, 0, 4), Some(1));
        assert_eq!(card_index_at(area, 3, 7, 5, 2, 4), Some(3));
        assert_eq!(card_index_at(area, 3, 14, 2, 0, 4), None);
    }

    #[test]
    fn click_hits_returns_menu_index() {
        let hits = vec![(Rect { x: 2, y: 4, width: 10, height: 1 }, 0), (Rect { x: 2, y: 5, width: 10, height: 1 }, 1)];
        assert_eq!(click_hits(&hits, 3, 4), Some(0));
        assert_eq!(click_hits(&hits, 3, 5), Some(1));
        assert_eq!(click_hits(&hits, 1, 4), None);
    }
}
