use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::collections::HashMap;

use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};

use super::layout::{block, center_h, chapter_item, dim, hl, inner, set_input_cursor, width_of, work_col};
use crate::app::{App, CARD_COVER_W, CARD_GAP, ReaderSession, filtered_chapters};
use crate::model::{Book, ShelfItem, gender_label};
use crate::reader::{WrapOpts, clamp_offset, visible_window};
use crate::store;
use crate::theme::{Palette, palette};

/// 封面区域：225×300 的封面按 3:4 放进 18 列 × 12 行（半块模式下 1 列 ≈ 1 px 宽、1 行 ≈ 2 px 高，同样是 3:4）。
const COVER_W: u16 = 18;
const COVER_H: u16 = 12;
/// 没有封面时书籍信息区的高度。
const INFO_H: u16 = 5;
/// 封面和信息列之间的间隔。
const COVER_GAP: u16 = 2;

/// 元数据行里相邻字段之间的间隔。字段之间只用空白分隔，不加符号，也不给字段单独上色：
/// 高亮行用 REVERSED 实现时，任何单独设置的前景色都会变成一块底色。
const FIELD_GAP: &str = "  ";

/// 元数据行：空字段不占位，非空字段带可选前缀（「读到」「最新」）。
fn meta_line(parts: &[(&str, &str)]) -> Line<'static> {
    let text = parts.iter().filter(|(_, v)| !v.is_empty()).map(|(label, value)| if label.is_empty() { value.to_string() } else { format!("{label} {value}") }).collect::<Vec<_>>().join(FIELD_GAP);
    Line::from(text)
}

/// 一张卡片：三行文字，外加可选的封面（book_id 和图片地址）。
struct Card {
    text: Text<'static>,
    cover: Option<(String, String)>,
}

fn card(title: &str, info1: Line<'static>, info2: Line<'static>, cover: Option<(String, String)>) -> Card {
    let head = Line::from(Span::styled(title.to_string(), Style::new().add_modifier(Modifier::BOLD)));
    Card { text: Text::from(vec![head, info1, info2]), cover: cover.filter(|(_, url)| !url.is_empty()) }
}

fn shelf_card(app: &App, b: &ShelfItem) -> Card {
    let read = app.state.progress.get(&b.book_id).map(|p| p.title.as_str()).filter(|t| !t.is_empty()).unwrap_or(b.last_read_title.as_str());
    let count = if b.chapter_count == 0 { String::new() } else { format!("{} 章", b.chapter_count) };
    let info1 = meta_line(&[("", b.author.as_str()), ("", b.status_label()), ("", count.as_str())]);
    let info2 = meta_line(&[("读到", read), ("最新", b.last_chapter_title.as_str())]);
    card(&b.title, info1, info2, Some((b.book_id.clone(), b.thumb_url.clone())))
}

fn search_card(b: &Book) -> Card {
    let words = b.word_label();
    let info1 = meta_line(&[("", b.author.as_str()), ("", b.category.as_str()), ("", b.status_label()), ("", words.as_str())]);
    let brief: String = b.abstract_text.split_whitespace().collect::<Vec<_>>().join(" ");
    card(&b.title, info1, Line::from(brief), Some((b.book_id.clone(), b.thumb_url.clone())))
}

/// 榜单卡片：标题前带名次，第二行作者、状态、字数，第三行简介压成一行。
fn rank_card(pos: usize, b: &Book) -> Card {
    let words = b.word_label();
    let info1 = meta_line(&[("", b.author.as_str()), ("", b.status_label()), ("", words.as_str())]);
    let brief: String = b.abstract_text.split_whitespace().collect::<Vec<_>>().join(" ");
    card(&format!("{pos}. {}", b.title), info1, Line::from(brief), Some((b.book_id.clone(), b.thumb_url.clone())))
}

/// 卡片列表的排版参数。covers 为 None 时不留封面列。
struct CardView<'a> {
    height: u16,
    covers: Option<&'a mut HashMap<String, StatefulProtocol>>,
}

/// 书架和搜索结果共用的卡片列表。自己排版而不用 List，因为项与项之间要空一行而高亮只盖住项本身。
/// 滚动偏移存在 ListState.offset 里，保证选中项完整可见。返回鼠标命中用的内容区和还没加载的封面。
fn draw_cards(frame: &mut ratatui::Frame, area: Rect, cards: Vec<Card>, empty: &str, state: &mut ListState, p: Palette, mut view: CardView) -> (Rect, Vec<(String, String)>) {
    frame.render_widget(block("", p), area);
    let content = inner(area);
    let mut wanted = Vec::new();
    if cards.is_empty() {
        frame.render_widget(Paragraph::new(empty).style(dim(p)), content);
        return (Rect { height: 0, ..content }, wanted);
    }
    let stride = view.height + CARD_GAP;
    let visible = (content.height.saturating_add(CARD_GAP) / stride).max(1) as usize;
    let selected = state.selected().map(|s| s.min(cards.len() - 1));
    let mut offset = state.offset().min(cards.len() - 1);
    if let Some(s) = selected {
        if s < offset {
            offset = s;
        } else if s >= offset + visible {
            offset = s + 1 - visible;
        }
    }
    *state.offset_mut() = offset;
    for (i, card) in cards.into_iter().enumerate().skip(offset) {
        let y = content.y.saturating_add(((i - offset) as u16).saturating_mul(stride));
        if y >= content.bottom() {
            break;
        }
        let rect = Rect { x: content.x, y, width: content.width, height: view.height.min(content.bottom() - y) };
        let text_area = match view.covers.as_deref_mut() {
            Some(covers) => {
                let [cover, text] = Layout::horizontal([Constraint::Length(CARD_COVER_W), Constraint::Fill(1)]).spacing(1).areas(rect);
                if let Some((id, url)) = card.cover {
                    match covers.get_mut(&id) {
                        Some(protocol) => frame.render_stateful_widget(StatefulImage::new().resize(Resize::Fit(None)), cover, protocol),
                        None => wanted.push((id, url)),
                    }
                }
                text
            }
            None => rect,
        };
        let style = if selected == Some(i) { hl(p) } else { Style::new() };
        frame.render_widget(Paragraph::new(card.text).style(style), text_area);
    }
    (content, wanted)
}

pub(super) fn draw_shelf(app: &mut App, frame: &mut ratatui::Frame, area: Rect) {
    let p = palette(app.state.settings.theme);
    app.menu_hits.clear();
    let col = work_col(area);
    let folders = store::all_folders(&app.state);
    let [tabs, list_area] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(col);
    app.folder_tabs.clear();
    let cells = Layout::horizontal(folders.iter().map(|f| Constraint::Length(width_of(f).max(1)))).spacing(1).flex(Flex::Start).split(tabs);
    for (i, (f, cell)) in folders.iter().zip(cells.iter()).enumerate() {
        let style = if i == app.folder_idx { hl(p) } else { Style::new() };
        frame.render_widget(Paragraph::new(f.as_str()).style(style), *cell);
        app.folder_tabs.push((*cell, i));
    }
    let items: Vec<Card> = app.visible_shelf().iter().map(|b| shelf_card(app, b)).collect();
    let empty = if app.state.user.is_none() && app.state.shelf.is_empty() { "登录后同步书架，或用 / 搜索" } else { "空文件夹" };
    let mut state = std::mem::take(&mut app.shelf_list);
    let (hit, wanted) = draw_cards(frame, list_area, items, empty, &mut state, p, card_view(app));
    app.shelf_list = state;
    app.list_area = hit;
    app.cover_wanted.extend(wanted);
}

fn card_view(app: &mut App) -> CardView<'_> {
    let height = app.card_height();
    CardView { height, covers: if app.card_covers() { Some(&mut app.covers) } else { None } }
}

pub(super) fn draw_search(app: &mut App, frame: &mut ratatui::Frame, area: Rect) {
    let p = palette(app.state.settings.theme);
    app.menu_hits.clear();
    app.folder_tabs.clear();
    let col = work_col(area);
    let [input, list_area] = Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(col);
    frame.render_widget(Paragraph::new(app.search_input.as_str()).block(block("搜索：书名、作者、book_id 或书籍页链接", p)), input);
    if app.overlay == crate::app::Overlay::None {
        set_input_cursor(frame, input, &app.search_input);
    }
    let items: Vec<Card> = app.search_results.iter().map(search_card).collect();
    let mut state = std::mem::take(&mut app.search_list);
    let (hit, wanted) = draw_cards(frame, list_area, items, "输入关键词后回车", &mut state, p, card_view(app));
    app.search_list = state;
    app.list_area = hit;
    app.cover_wanted.extend(wanted);
}

/// 榜单：顶行是性别、分类和当前起始名次，下面是卡片列表。先画列表再画顶行，因为起始名次取的是 draw_cards 修正后的滚动偏移。
pub(super) fn draw_rank(app: &mut App, frame: &mut ratatui::Frame, area: Rect) {
    let p = palette(app.state.settings.theme);
    app.menu_hits.clear();
    app.folder_tabs.clear();
    let col = work_col(area);
    let [head, list_area] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(col);
    let items: Vec<Card> = app.rank_books.iter().enumerate().map(|(i, b)| rank_card(i + 1, b)).collect();
    let empty = if app.busy { "加载中…" } else { "榜单为空" };
    let mut state = std::mem::take(&mut app.rank_list);
    let (hit, wanted) = draw_cards(frame, list_area, items, empty, &mut state, p, card_view(app));
    app.rank_list = state;
    app.list_area = hit;
    app.cover_wanted.extend(wanted);
    let gender = gender_label(app.rank_gender);
    let category = app.rank_category().map(|c| c.name.clone()).unwrap_or_default();
    let from = if app.rank_books.is_empty() { String::new() } else { format!("第 {} 名起", app.rank_list.offset() + 1) };
    let [gender_a, category_a, from_a] = Layout::horizontal([Constraint::Length(width_of(gender)), Constraint::Length(width_of(&category)), Constraint::Length(width_of(&from))]).spacing(2).areas(head);
    frame.render_widget(Paragraph::new(gender).style(hl(p)), gender_a);
    frame.render_widget(Paragraph::new(category).style(Style::new().add_modifier(Modifier::BOLD)), category_a);
    frame.render_widget(Paragraph::new(from).style(dim(p)), from_a);
}

/// 书籍页元数据行：状态、字数、章数、最新章。
fn book_meta_line(book: &Book) -> Line<'static> {
    let words = book.word_label();
    let count = if book.chapter_count == 0 { String::new() } else { format!("{} 章", book.chapter_count) };
    meta_line(&[("", book.status_label()), ("", words.as_str()), ("", count.as_str()), ("最新", book.last_chapter_title.as_str())])
}

/// 书籍页第三行右边补上阅读进度，让「继续阅读」有依据。
fn book_progress_line(app: &App, book_id: &str) -> Option<Line<'static>> {
    let title = app.state.progress.get(book_id).map(|p| p.title.as_str()).filter(|t| !t.is_empty())?;
    Some(meta_line(&[("读到", title)]))
}

pub(super) fn draw_book(app: &mut App, frame: &mut ratatui::Frame, area: Rect) {
    let p = palette(app.state.settings.theme);
    app.menu_hits.clear();
    app.folder_tabs.clear();
    let show_cover = app.state.settings.show_covers;
    let progress_line = app.open.as_ref().and_then(|o| book_progress_line(app, &o.book.book_id));
    let Some(open) = app.open.as_mut() else { return };
    let col = work_col(area);
    let has_cover_slot = show_cover && !open.book.thumb_url.is_empty() && col.width >= COVER_W * 3;
    let info_h = if has_cover_slot { COVER_H } else { INFO_H };
    let [top, list_area] = Layout::vertical([Constraint::Length(info_h), Constraint::Fill(1)]).areas(col);
    let info = if has_cover_slot {
        let [cover, info] = Layout::horizontal([Constraint::Length(COVER_W), Constraint::Fill(1)]).spacing(COVER_GAP).areas(top);
        match app.covers.get_mut(&open.book.book_id) {
            Some(protocol) => frame.render_stateful_widget(StatefulImage::new().resize(Resize::Fit(None)), cover, protocol),
            None => frame.render_widget(Paragraph::new("封面加载中").style(dim(p)), cover),
        }
        info
    } else {
        top
    };
    let author_line = meta_line(&[("", open.book.author.as_str()), ("", open.book.category.as_str())]);
    let title_line = Line::from(Span::styled(open.book.title.clone(), Style::new().add_modifier(Modifier::BOLD)));
    let mut info_lines = vec![title_line, author_line, book_meta_line(&open.book)];
    info_lines.extend(progress_line);
    info_lines.extend([Line::default(), Line::from(open.book.abstract_text.clone())]);
    frame.render_widget(Paragraph::new(info_lines).wrap(Wrap { trim: true }).style(Style::new().fg(p.fg)), info);
    let items: Vec<ListItem> = filtered_chapters(&open.chapters, &open.filter).into_iter().map(|(i, c)| chapter_item(i, c, p)).collect();
    let list = List::new(items).block(block("", p)).highlight_style(hl(p));
    app.list_area = inner(list_area);
    frame.render_stateful_widget(list, list_area, &mut open.list);
}

pub(super) fn read_percent(shown_through: usize, total: usize) -> usize {
    if total == 0 || shown_through >= total { 100 } else { shown_through.saturating_mul(100) / total }
}

/// 正文窗口计算：折行、回写 view_height、夹住偏移。返回 (start, end, percent)，调用方按 start..end 切 cache.lines()。
/// 进度按屏幕内最后一行算，最后一行可见即 100%。原生阅读页和伪装态共用，保证翻页步长和进度口径一致。
pub(super) fn reader_window(session: &mut ReaderSession, text_area: Rect, opts: WrapOpts) -> (usize, usize, usize) {
    session.prepare_wrap(text_area.width, opts);
    session.view_height = text_area.height as usize;
    session.offset = clamp_offset(session.offset, session.cache.len(), session.view_height);
    let (start, window) = visible_window(session.cache.lines(), session.offset, session.view_height);
    let end = start + window.len();
    (start, end, read_percent(end, session.cache.len()))
}

pub(super) fn draw_reader(app: &mut App, frame: &mut ratatui::Frame, area: Rect) {
    let p = palette(app.state.settings.theme);
    app.menu_hits.clear();
    app.folder_tabs.clear();
    let pad = app.state.settings.margin;
    let text_width = app.state.settings.text_width;
    let opts = app.wrap_opts();
    let Some(session) = app.reader.as_mut() else { return };
    let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
    let usable = Rect { x: body.x.saturating_add(pad), y: body.y, width: body.width.saturating_sub(pad.saturating_mul(2)).max(1), height: body.height };
    let text_w = if text_width == 0 { usable.width } else { text_width.min(usable.width) }.max(4);
    let text_area = center_h(usable, text_w);
    let (start, end, percent) = reader_window(session, text_area, opts);
    let window = &session.cache.lines()[start..end];
    let position = format!("{}/{}", session.index + 1, session.chapters.len().max(1));
    let percent = format!("{percent}%");
    let [title_a, pos_a, pct_a] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(width_of(&position)), Constraint::Length(width_of(&percent))]).spacing(2).areas(head);
    frame.render_widget(Paragraph::new(session.body.title.as_str()).style(dim(p)), title_a);
    frame.render_widget(Paragraph::new(position).style(dim(p)), pos_a);
    frame.render_widget(Paragraph::new(percent).style(dim(p)), pct_a);
    let text = window.join("\n");
    let para = if p.inherit { Paragraph::new(text) } else { Paragraph::new(text).style(Style::new().fg(p.fg).bg(p.bg)) };
    app.list_area = area;
    frame.render_widget(para, text_area);
    // 正文下方还有空行时标出章末，读者一眼知道已到底、下一次空格是切章。
    let shown = window.len() as u16;
    if session.at_end() && shown + 1 < text_area.height {
        let mark = Rect { x: text_area.x, y: text_area.y + shown + 1, width: text_area.width, height: 1 };
        frame.render_widget(Paragraph::new(format!("{} 完", session.body.title)).style(dim(p)).centered(), mark);
    }
}

#[cfg(test)]
mod tests {
    use super::read_percent;

    #[test]
    fn percent_hits_100_when_last_line_visible() {
        assert_eq!(read_percent(20, 100), 20);
        assert_eq!(read_percent(100, 100), 100);
        assert_eq!(read_percent(5, 5), 100);
        assert_eq!(read_percent(0, 0), 100);
    }
}
