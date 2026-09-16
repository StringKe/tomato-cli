//! 宽字符感知的终端后端。
//!
//! ratatui 自带的 crossterm 后端只在「下一格正好是上一格 x + 1」时省掉光标定位，而汉字占两列，
//! 所以整屏中文每个字前面都会多发一条 MoveTo，一帧输出约为必要量的三倍。本地终端感觉不到，
//! ssh / mosh / tmux 远端按住 j 滚动时字节量直接变成延迟。这里按字符显示宽度推算光标位置，
//! 只在真的不连续时才定位；其余行为与 CrosstermBackend 一致。

use std::io::{self, Write};

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::crossterm::cursor::MoveTo;
use ratatui::crossterm::queue;
use ratatui::crossterm::style::{Attribute, Color as CColor, Colors, Print, SetAttribute, SetBackgroundColor, SetColors, SetForegroundColor, SetUnderlineColor};
use ratatui::layout::{Position, Size};
use ratatui::prelude::{CrosstermBackend, IntoCrossterm};
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

pub struct WideBackend<W: Write> {
    inner: CrosstermBackend<W>,
}

impl<W: Write> WideBackend<W> {
    pub fn new(writer: W) -> Self {
        Self { inner: CrosstermBackend::new(writer) }
    }
}

/// 画完一个格子后光标应该在哪。带 VS16（U+FE0F）的符号各终端推进列数不一致，下一格重新定位。
fn cursor_after(x: u16, y: u16, symbol: &str) -> Option<Position> {
    if symbol.contains('\u{fe0f}') {
        return None;
    }
    let width = symbol.width().max(1) as u16;
    x.checked_add(width).map(|x| Position { x, y })
}

/// 只发从 `from` 到 `to` 的属性差异，与 ratatui 的 crossterm 后端相同的规则：
/// Bold 和 Dim 共用一个 NormalIntensity 复位，复位后要把留下的那个重新打开。
fn queue_modifier<W: Write>(w: &mut W, from: Modifier, to: Modifier) -> io::Result<()> {
    let removed = from - to;
    if removed.contains(Modifier::REVERSED) {
        queue!(w, SetAttribute(Attribute::NoReverse))?;
    }
    let reset_intensity = removed.contains(Modifier::BOLD) || removed.contains(Modifier::DIM);
    if reset_intensity {
        queue!(w, SetAttribute(Attribute::NormalIntensity))?;
        if to.contains(Modifier::DIM) {
            queue!(w, SetAttribute(Attribute::Dim))?;
        }
        if to.contains(Modifier::BOLD) {
            queue!(w, SetAttribute(Attribute::Bold))?;
        }
    }
    if removed.contains(Modifier::ITALIC) {
        queue!(w, SetAttribute(Attribute::NoItalic))?;
    }
    if removed.contains(Modifier::UNDERLINED) {
        queue!(w, SetAttribute(Attribute::NoUnderline))?;
    }
    if removed.contains(Modifier::CROSSED_OUT) {
        queue!(w, SetAttribute(Attribute::NotCrossedOut))?;
    }
    if removed.contains(Modifier::HIDDEN) {
        queue!(w, SetAttribute(Attribute::NoHidden))?;
    }
    if removed.contains(Modifier::SLOW_BLINK) || removed.contains(Modifier::RAPID_BLINK) {
        queue!(w, SetAttribute(Attribute::NoBlink))?;
    }
    let added = to - from;
    if added.contains(Modifier::REVERSED) {
        queue!(w, SetAttribute(Attribute::Reverse))?;
    }
    if added.contains(Modifier::BOLD) && !reset_intensity {
        queue!(w, SetAttribute(Attribute::Bold))?;
    }
    if added.contains(Modifier::ITALIC) {
        queue!(w, SetAttribute(Attribute::Italic))?;
    }
    if added.contains(Modifier::UNDERLINED) {
        queue!(w, SetAttribute(Attribute::Underlined))?;
    }
    if added.contains(Modifier::DIM) && !reset_intensity {
        queue!(w, SetAttribute(Attribute::Dim))?;
    }
    if added.contains(Modifier::CROSSED_OUT) {
        queue!(w, SetAttribute(Attribute::CrossedOut))?;
    }
    if added.contains(Modifier::HIDDEN) {
        queue!(w, SetAttribute(Attribute::Hidden))?;
    }
    if added.contains(Modifier::SLOW_BLINK) {
        queue!(w, SetAttribute(Attribute::SlowBlink))?;
    }
    if added.contains(Modifier::RAPID_BLINK) {
        queue!(w, SetAttribute(Attribute::RapidBlink))?;
    }
    Ok(())
}

impl<W: Write> Backend for WideBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut fg = Color::Reset;
        let mut bg = Color::Reset;
        let mut underline = Color::Reset;
        let mut modifier = Modifier::empty();
        let mut cursor: Option<Position> = None;
        for (x, y, cell) in content {
            if cursor != Some(Position { x, y }) {
                queue!(self.inner, MoveTo(x, y))?;
            }
            if cell.modifier != modifier {
                queue_modifier(&mut self.inner, modifier, cell.modifier)?;
                modifier = cell.modifier;
            }
            if cell.fg != fg || cell.bg != bg {
                queue!(self.inner, SetColors(Colors::new(cell.fg.into_crossterm(), cell.bg.into_crossterm())))?;
                fg = cell.fg;
                bg = cell.bg;
            }
            if cell.underline_color != underline {
                queue!(self.inner, SetUnderlineColor(cell.underline_color.into_crossterm()))?;
                underline = cell.underline_color;
            }
            let symbol = cell.symbol();
            queue!(self.inner, Print(symbol))?;
            cursor = cursor_after(x, y, symbol);
        }
        queue!(self.inner, SetForegroundColor(CColor::Reset), SetBackgroundColor(CColor::Reset), SetUnderlineColor(CColor::Reset), SetAttribute(Attribute::Reset))
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        self.inner.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(&mut self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只有 MoveTo 用到分号（颜色都是 Reset 时不发 SetColors），数分号就是数定位次数。
    fn moves(out: &[u8]) -> usize {
        out.iter().filter(|b| **b == b';').count()
    }

    fn render(cells: &[(u16, u16, Cell)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut backend = WideBackend::new(&mut out);
        backend.draw(cells.iter().map(|(x, y, c)| (*x, *y, c))).unwrap();
        backend.flush().unwrap();
        drop(backend);
        out
    }

    #[test]
    fn contiguous_wide_cells_move_once() {
        let cells: Vec<(u16, u16, Cell)> = (0..10).map(|i| (i * 2, 3, Cell::new("汉"))).collect();
        assert_eq!(moves(&render(&cells)), 1);
    }

    #[test]
    fn gap_or_new_row_moves_again() {
        let cells = vec![(0, 0, Cell::new("a")), (1, 0, Cell::new("b")), (5, 0, Cell::new("c")), (0, 1, Cell::new("d"))];
        assert_eq!(moves(&render(&cells)), 3);
    }

    #[test]
    fn vs16_symbol_forces_reposition() {
        let cells = vec![(0, 0, Cell::new("\u{2764}\u{fe0f}")), (2, 0, Cell::new("x"))];
        assert_eq!(moves(&render(&cells)), 2);
    }
}
