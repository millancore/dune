//! Drawing the terminal grid into an ARGB8888 pixel buffer.

use alacritty_terminal::{
    event::EventListener,
    grid::Dimensions,
    term::{Term, cell::Flags},
    vte::ansi::{CursorShape, Rgb},
};

use dune_common::picker::{HINT, Picker};

use crate::{
    font::{Font, Style},
    palette,
};

// The @ picker's colors.
const INPUT_BG: Rgb = Rgb { r: 0x2c, g: 0x31, b: 0x3c };
const PANEL: Rgb = Rgb { r: 0x1f, g: 0x23, b: 0x2b };
const ACCENT: Rgb = Rgb { r: 0x61, g: 0xaf, b: 0xef };
const ACCENT_TEXT: Rgb = Rgb { r: 0x14, g: 0x14, b: 0x1a };
const DIM: Rgb = Rgb { r: 0x7f, g: 0x84, b: 0x8e };

/// Background of selected text.
const SELECTION: Rgb = Rgb { r: 0x3e, g: 0x44, b: 0x51 };

/// Space around the grid, in pixels.
pub const PADDING: u32 = 4;

pub struct Canvas<'a> {
    pub pixels: &'a mut [u8],
    pub width: u32,
    pub height: u32,
}

impl Canvas<'_> {
    fn fill(&mut self, color: Rgb) {
        let px = argb(color);
        for chunk in self.pixels.chunks_exact_mut(4) {
            chunk.copy_from_slice(&px);
        }
    }

    fn rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Rgb) {
        let px = argb(color);
        let (x0, y0) = (x.max(0) as u32, y.max(0) as u32);
        let x1 = ((x + w as i32).max(0) as u32).min(self.width);
        let y1 = ((y + h as i32).max(0) as u32).min(self.height);
        for row in y0..y1 {
            let start = ((row * self.width + x0) * 4) as usize;
            let end = ((row * self.width + x1) * 4) as usize;
            for chunk in self.pixels[start..end].chunks_exact_mut(4) {
                chunk.copy_from_slice(&px);
            }
        }
    }

    /// Blend a coverage mask in `color` (or, with `color == None`, an RGBA image)
    /// with its top-left corner at (x, y).
    fn mask(&mut self, x: i32, y: i32, w: u32, h: u32, data: &[u8], color: Option<Rgb>) {
        for my in 0..h as i32 {
            let py = y + my;
            if py < 0 || py >= self.height as i32 {
                continue;
            }
            for mx in 0..w as i32 {
                let px = x + mx;
                if px < 0 || px >= self.width as i32 {
                    continue;
                }
                let j = (my * w as i32 + mx) as usize;
                let (src, a) = match color {
                    Some(c) => (c, data[j] as u32),
                    None => (Rgb { r: data[4 * j], g: data[4 * j + 1], b: data[4 * j + 2] }, data[4 * j + 3] as u32),
                };
                if a == 0 {
                    continue;
                }
                let i = ((py as u32 * self.width + px as u32) * 4) as usize;
                let blend = |dst: u8, src: u8| ((src as u32 * a + dst as u32 * (255 - a)) / 255) as u8;
                // Little-endian ARGB8888: B, G, R, A.
                self.pixels[i] = blend(self.pixels[i], src.b);
                self.pixels[i + 1] = blend(self.pixels[i + 1], src.g);
                self.pixels[i + 2] = blend(self.pixels[i + 2], src.r);
                self.pixels[i + 3] = 0xff;
            }
        }
    }
}

fn argb(c: Rgb) -> [u8; 4] {
    [c.b, c.g, c.r, 0xff]
}

/// Columns and lines that fit in a window of this size.
pub fn grid_size(font: &Font, width: u32, height: u32) -> (usize, usize) {
    let cols = width.saturating_sub(2 * PADDING) / font.cell_width;
    let lines = height.saturating_sub(2 * PADDING) / font.cell_height;
    (cols.max(2) as usize, lines.max(1) as usize)
}

/// Draw the terminal, and the `@` picker over it when one is open.
pub fn draw<T: EventListener>(canvas: &mut Canvas, term: &Term<T>, font: &mut Font, focused: bool, picker: Option<&Picker>) {
    let grid = (term.columns(), term.screen_lines());
    let content = term.renderable_content();
    let colors = content.colors;
    let background = palette::background(colors);
    let cursor_color = palette::resolve(
        alacritty_terminal::vte::ansi::Color::Named(alacritty_terminal::vte::ansi::NamedColor::Cursor),
        colors,
    );
    canvas.fill(background);

    let (cw, ch, baseline) = (font.cell_width, font.cell_height, font.baseline);
    let offset = content.display_offset as i32;
    let cursor = content.cursor;
    let selection = content.selection;

    for indexed in content.display_iter {
        let cell = indexed.cell;
        let row = indexed.point.line.0 + offset;
        if row < 0 || cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let x = (PADDING + indexed.point.column.0 as u32 * cw) as i32;
        let y = (PADDING + row as u32 * ch) as i32;
        let width = if cell.flags.contains(Flags::WIDE_CHAR) { 2 * cw } else { cw };

        let (mut fg, mut bg) = palette::cell_colors(cell.fg, cell.bg, cell.flags, colors);
        if selection.is_some_and(|s| s.contains(indexed.point)) {
            bg = SELECTION;
        }
        // While the picker is open it draws its own cursor.
        let is_cursor = indexed.point == cursor.point && cursor.shape != CursorShape::Hidden && picker.is_none();
        let solid_cursor = is_cursor && focused && cursor.shape == CursorShape::Block;
        if solid_cursor {
            (fg, bg) = (bg, cursor_color);
        }

        if bg != background {
            canvas.rect(x, y, width, ch, bg);
        }
        if cell.c != ' ' && !cell.flags.contains(Flags::HIDDEN) {
            let style = Style::new(cell.flags.contains(Flags::BOLD), cell.flags.contains(Flags::ITALIC));
            if let Some(glyph) = font.glyph(cell.c, style) {
                let gx = x + glyph.left;
                let gy = y + baseline - glyph.top;
                let color = (!glyph.color).then_some(fg);
                canvas.mask(gx, gy, glyph.width, glyph.height, &glyph.data, color);
            }
        }
        if cell.flags.intersects(Flags::ALL_UNDERLINES) {
            canvas.rect(x, y + baseline + 1, width, 1, fg);
        }
        if cell.flags.contains(Flags::STRIKEOUT) {
            canvas.rect(x, y + baseline * 2 / 3, width, 1, fg);
        }

        if is_cursor && !solid_cursor {
            match cursor.shape {
                CursorShape::Beam => canvas.rect(x, y, 2, ch, cursor_color),
                CursorShape::Underline => canvas.rect(x, y + ch as i32 - 2, width, 2, cursor_color),
                // Unfocused (or hollow block): an outline.
                _ => {
                    canvas.rect(x, y, width, 1, cursor_color);
                    canvas.rect(x, y + ch as i32 - 1, width, 1, cursor_color);
                    canvas.rect(x, y, 1, ch, cursor_color);
                    canvas.rect(x + width as i32 - 1, y, 1, ch, cursor_color);
                }
            }
        }
    }

    if let Some(picker) = picker {
        let row = (cursor.point.line.0 + offset).max(0) as usize;
        draw_picker(canvas, font, picker, (cursor.point.column.0, row), grid, palette::foreground(colors));
    }
}

/// Text on the grid at a cell, one character per cell; returns the cells used.
fn put_text(canvas: &mut Canvas, font: &mut Font, (col, row): (usize, usize), text: &str, fg: Rgb, style: Style) -> usize {
    let (cw, ch, baseline) = (font.cell_width, font.cell_height, font.baseline);
    let mut n = 0;
    for c in text.chars() {
        let x = (PADDING + (col + n) as u32 * cw) as i32;
        let y = (PADDING + row as u32 * ch) as i32;
        if c != ' '
            && let Some(glyph) = font.glyph(c, style)
        {
            let color = (!glyph.color).then_some(fg);
            canvas.mask(x + glyph.left, y + baseline - glyph.top, glyph.width, glyph.height, &glyph.data, color);
        }
        n += 1;
    }
    n
}

fn cells(canvas: &mut Canvas, font: &Font, (col, row): (usize, usize), width: usize, color: Rgb) {
    let x = (PADDING + col as u32 * font.cell_width) as i32;
    let y = (PADDING + row as u32 * font.cell_height) as i32;
    canvas.rect(x, y, width as u32 * font.cell_width, font.cell_height, color);
}

/// Where the suggestion panel goes: below the prompt line when it fits, else above.
/// Returns (first column, first row, width in cells).
pub fn panel_place(cursor: (usize, usize), grid: (usize, usize), height: usize, width: usize) -> (usize, usize, usize) {
    let (col, row) = cursor;
    let (cols, lines) = grid;
    let width = width.min(cols);
    let left = col.min(cols - width);
    let top = if row + 1 + height <= lines { row + 1 } else { row.saturating_sub(height) };
    (left, top, width)
}

/// The `@` line at the cursor and the panel of suggestions next to it.
fn draw_picker(canvas: &mut Canvas, font: &mut Font, picker: &Picker, cursor: (usize, usize), grid: (usize, usize), fg: Rgb) {
    // The line being typed, with its own cursor.
    let line = format!("@{}", picker.input());
    let len = line.chars().count();
    cells(canvas, font, cursor, (len + 1).min(grid.0.saturating_sub(cursor.0)), INPUT_BG);
    put_text(canvas, font, cursor, &line, fg, Style::Bold);
    if cursor.0 + len < grid.0 {
        cells(canvas, font, (cursor.0 + len, cursor.1), 1, ACCENT);
    }

    let (first, rows) = picker.window();
    let hint_len = HINT.chars().count() + 2;
    let label_len = rows.iter().map(|c| c.label.chars().count() + 4).max().unwrap_or(0);
    let height = rows.len() + 1;
    let (left, top, width) = panel_place(cursor, grid, height, label_len.max(hint_len.min(48)));

    for (i, candidate) in rows.iter().enumerate() {
        let selected = first + i == picker.selected();
        let at = (left, top + i);
        cells(canvas, font, at, width, if selected { ACCENT } else { PANEL });
        let label: String = candidate.label.chars().take(width.saturating_sub(2)).collect();
        let (color, style) = if selected { (ACCENT_TEXT, Style::Bold) } else { (fg, Style::Regular) };
        put_text(canvas, font, (left + 1, top + i), &label, color, style);
    }
    let hint_row = (left, top + rows.len());
    cells(canvas, font, hint_row, width, PANEL);
    let hint: String = HINT.chars().take(width.saturating_sub(2)).collect();
    put_text(canvas, font, (left + 1, hint_row.1), &hint, DIM, Style::Regular);
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::{
        event::VoidListener,
        index::{Column, Line, Point, Side},
        selection::{Selection, SelectionType},
        term::{Config, test::TermSize},
        vte::ansi::Processor,
    };

    use super::*;

    const COLS: usize = 20;

    /// Render `input` on a one-line terminal; returns the pixels and the font.
    fn render(input: &str, select_cols: Option<(usize, usize)>) -> (Vec<u8>, Font) {
        let mut font = Font::load("monospace", 20.0).unwrap();
        let mut term = Term::new(Config::default(), &TermSize::new(COLS, 2), VoidListener);
        Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new().advance(&mut term, input.as_bytes());
        if let Some((from, to)) = select_cols {
            let mut s = Selection::new(SelectionType::Simple, Point::new(Line(0), Column(from)), Side::Left);
            s.update(Point::new(Line(0), Column(to)), Side::Right);
            term.selection = Some(s);
        }
        let (w, h) = (2 * PADDING + COLS as u32 * font.cell_width, 2 * PADDING + 2 * font.cell_height);
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        draw(&mut Canvas { pixels: &mut pixels, width: w, height: h }, &term, &mut font, false, None);
        (pixels, font)
    }

    /// The pixels (BGRA) of one cell of the first line.
    fn cell(pixels: &[u8], font: &Font, col: usize) -> Vec<[u8; 4]> {
        let width = 2 * PADDING + COLS as u32 * font.cell_width;
        let mut out = Vec::new();
        for y in PADDING..PADDING + font.cell_height {
            for x in 0..font.cell_width {
                let i = ((y * width + PADDING + col as u32 * font.cell_width + x) * 4) as usize;
                out.push([pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]);
            }
        }
        out
    }

    #[test]
    fn bold_and_italic_differ_from_regular() {
        let (p, f) = render("a\x1b[1ma\x1b[0m\x1b[3ma", None);
        let (regular, bold, italic) = (cell(&p, &f, 0), cell(&p, &f, 1), cell(&p, &f, 2));
        assert_ne!(regular, bold);
        assert_ne!(regular, italic);
    }

    #[test]
    fn emoji_is_drawn_in_color() {
        let (p, f) = render("😀", None);
        // Text is gray-scale over a gray-ish background; an emoji has saturated pixels.
        let colorful = cell(&p, &f, 0)
            .iter()
            .chain(cell(&p, &f, 1).iter())
            .any(|[b, g, r, _]| (*r as i32 - *b as i32).abs() > 60 || (*g as i32 - *b as i32).abs() > 60);
        assert!(colorful, "no colored pixels where the emoji should be");
    }

    #[test]
    fn picker_panel_goes_below_or_above() {
        // Room below: under the prompt line, aligned with the cursor.
        assert_eq!(panel_place((2, 0), (80, 24), 5, 30), (2, 1, 30));
        // Bottom line: above it.
        assert_eq!(panel_place((2, 23), (80, 24), 5, 30), (2, 18, 30));
        // Near the right edge: shifted left to fit; wider than the grid: clipped.
        assert_eq!(panel_place((70, 0), (80, 24), 5, 30), (50, 1, 30));
        assert_eq!(panel_place((0, 0), (20, 24), 5, 30), (0, 1, 20));
    }

    #[test]
    fn picker_is_drawn_over_the_terminal() {
        let mut font = Font::load("monospace", 20.0).unwrap();
        let term = Term::new(Config::default(), &TermSize::new(COLS, 6), VoidListener);
        let app = |id: &str, name: &str| dune_common::desktop::App { id: id.into(), name: name.into(), exec: id.into() };
        let mut picker = Picker::new(vec![app("nvim", "Neovim"), app("nvim-qt", "Neovim Qt")], std::env::temp_dir());
        for key in dune_common::picker::parse_keys(b"nv") {
            picker.feed(key);
        }
        let (w, h) = (2 * PADDING + COLS as u32 * font.cell_width, 2 * PADDING + 6 * font.cell_height);
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        draw(&mut Canvas { pixels: &mut pixels, width: w, height: h }, &term, &mut font, true, Some(&picker));
        let has = |c: Rgb| pixels.chunks_exact(4).any(|p| p == argb(c));
        assert!(has(INPUT_BG), "the @ line");
        assert!(has(ACCENT), "the selected suggestion");
        assert!(has(PANEL), "the panel");
        // No picker, none of that.
        let mut plain = vec![0u8; (w * h * 4) as usize];
        draw(&mut Canvas { pixels: &mut plain, width: w, height: h }, &term, &mut font, true, None);
        assert!(!plain.chunks_exact(4).any(|p| p == argb(ACCENT) || p == argb(PANEL)));
    }

    #[test]
    fn selection_is_highlighted() {
        let (p, f) = render("hello world", Some((0, 4)));
        let selected = argb(SELECTION);
        assert!(cell(&p, &f, 2).contains(&selected));
        assert!(!cell(&p, &f, 8).contains(&selected));
    }
}
