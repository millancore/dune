//! Drawing the panel: a framed box with a prompt line, a list of rows (an icon in a square
//! as tall as the line, then text) and two footer lines, into ARGB8888 pixels. A compact
//! panel (no rows) is the prompt and the footer lines alone.

use dune_common::font::{Font, Style};

use crate::icons::Icon;

/// Text size, in pixels.
pub const FONT_SIZE: f32 = 18.0;
/// Characters across the panel.
pub const COLUMNS: usize = 60;
pub const PADDING: i32 = 16;
/// Space around an icon, inside its line.
pub const ICON_MARGIN: i32 = 2;
pub const FRAME: i32 = 2;
/// Rows of the list shown at once, in every mode.
pub const ROWS: usize = 10;
pub const BACKGROUND: [u8; 3] = [0x1e, 0x20, 0x30];
pub const FRAME_COLOR: [u8; 3] = [0x5c, 0x9e, 0xf2];
pub const SELECTED: [u8; 3] = [0x3b, 0x42, 0x61];
pub const TEXT: [u8; 3] = [0xd8, 0xdc, 0xe6];
pub const DIM: [u8; 3] = [0x7f, 0x84, 0x9c];
pub const ACCENT: [u8; 3] = [0xfa, 0x9e, 0x38];

/// Height of one line of text.
pub fn line_height(font: Option<&Font>) -> i32 {
    font.map_or(24, |f| f.cell_height as i32 + 6)
}

/// The prompt line, `rows` rows and a gap (none: neither), then two footer lines; at most
/// `max_width` wide.
pub fn panel_size(font: Option<&Font>, max_width: i32, rows: usize) -> (i32, i32) {
    let cell = font.map_or(10, |f| f.cell_width as i32);
    let w = (COLUMNS as i32 * cell + 2 * PADDING)
        .min(max_width)
        .max(2 * PADDING + 1);
    let h = (footer_line(rows) + 2) * line_height(font) + 2 * PADDING;
    (w, h)
}

/// The line the footer starts on, under the prompt and `rows` rows with a gap.
fn footer_line(rows: usize) -> i32 {
    if rows == 0 { 1 } else { rows as i32 + 2 }
}

/// The list row under `y` (pixels from the panel's top), if it is one of the `rows`.
pub fn row_at(font: Option<&Font>, y: f64, rows: usize) -> Option<usize> {
    let line = line_height(font) as f64;
    let row = ((y - PADDING as f64) / line).floor() - 1.0;
    (row >= 0.0 && row < rows as f64).then_some(row as usize)
}

/// A row of the list: its icon, its text and, dim at the right, a detail; with `sub`, a
/// second line under the text, dim too.
pub struct Row<'a> {
    pub icon: Option<&'a Icon>,
    pub text: &'a str,
    pub detail: Option<String>,
    pub sub: Option<String>,
    pub selected: bool,
}

impl Row<'_> {
    /// The lines of the list it takes.
    pub fn lines(&self) -> usize {
        if self.sub.is_some() { 2 } else { 1 }
    }
}

/// What the panel shows.
pub struct View<'a> {
    pub prompt: String,
    pub rows: Vec<Row<'a>>,
    /// The first footer line, in the accent colour, and the hint under it.
    pub status: String,
    pub hint: &'a str,
}

/// The panel's ARGB8888 pixels (B, G, R, A in memory), `w` × `h`, with `rows` lines of the list.
pub fn draw(font: Option<&mut Font>, view: &View, (w, h): (i32, i32), rows: usize) -> Vec<u8> {
    let mut canvas = Canvas {
        pixels: [BACKGROUND[2], BACKGROUND[1], BACKGROUND[0], 0xff].repeat((w * h) as usize),
        w,
        h,
    };
    canvas.fill(0, 0, w, FRAME, FRAME_COLOR);
    canvas.fill(0, h - FRAME, w, FRAME, FRAME_COLOR);
    canvas.fill(0, 0, FRAME, h, FRAME_COLOR);
    canvas.fill(w - FRAME, 0, FRAME, h, FRAME_COLOR);
    let Some(font) = font else { return canvas.pixels };

    let line = line_height(Some(font));
    let cell = font.cell_width.max(1) as i32;
    let cols = ((w - 2 * PADDING) / cell).max(1) as usize;
    let row_y = |row: i32| PADDING + row * line;

    canvas.text(font, PADDING, row_y(0), line, &view.prompt, TEXT, cols);
    let text_x = PADDING + line + ICON_MARGIN;
    let text_cols = cols
        .saturating_sub(((line + ICON_MARGIN) / cell) as usize)
        .max(1);
    let mut at = 0;
    for row in &view.rows {
        if at + row.lines() > rows {
            break;
        }
        let y = row_y(1 + at as i32);
        at += row.lines();
        if row.selected {
            canvas.fill(FRAME, y, w - 2 * FRAME, line * row.lines() as i32, SELECTED);
        }
        if let Some(sub) = &row.sub {
            canvas.text(font, text_x, y + line, line, sub, DIM, text_cols);
        }
        if let Some(icon) = row.icon {
            canvas.image(PADDING + ICON_MARGIN, y + ICON_MARGIN, icon);
        }
        match row.detail.as_deref() {
            Some(detail) => {
                let detail_cols = detail.chars().count().min(text_cols / 2);
                canvas.text(
                    font,
                    text_x,
                    y,
                    line,
                    row.text,
                    TEXT,
                    text_cols.saturating_sub(detail_cols + 2).max(1),
                );
                let detail_x = text_x + (text_cols - detail_cols) as i32 * cell;
                canvas.text(font, detail_x, y, line, detail, DIM, detail_cols);
            }
            None => canvas.text(font, text_x, y, line, row.text, TEXT, text_cols),
        }
    }
    let bottom = footer_line(rows);
    canvas.text(
        font,
        PADDING,
        row_y(bottom),
        line,
        &view.status,
        ACCENT,
        cols,
    );
    canvas.text(font, PADDING, row_y(bottom + 1), line, view.hint, DIM, cols);
    canvas.pixels
}

/// The size an icon is drawn at, for a line of text.
pub fn icon_size(font: Option<&Font>) -> u32 {
    (line_height(font) - 2 * ICON_MARGIN).max(1) as u32
}

pub struct Canvas {
    pub pixels: Vec<u8>,
    pub w: i32,
    pub h: i32,
}

impl Canvas {
    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: [u8; 3]) {
        for py in y.max(0)..(y + h).min(self.h) {
            for px in x.max(0)..(x + w).min(self.w) {
                let at = ((py * self.w + px) * 4) as usize;
                self.pixels[at..at + 3].copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
            }
        }
    }

    /// Blend `icon` (RGBA premultiplied) with its top-left corner at (x, y).
    pub fn image(&mut self, x: i32, y: i32, icon: &Icon) {
        let size = icon.size as i32;
        for iy in 0..size {
            for ix in 0..size {
                let (px, py) = (x + ix, y + iy);
                if px < 0 || py < 0 || px >= self.w || py >= self.h {
                    continue;
                }
                let src = &icon.pixels[((iy * size + ix) * 4) as usize..][..4];
                let alpha = src[3] as u32;
                if alpha == 0 {
                    continue;
                }
                let at = ((py * self.w + px) * 4) as usize;
                for (channel, value) in [(2, src[0]), (1, src[1]), (0, src[2])] {
                    let under = self.pixels[at + channel] as u32;
                    self.pixels[at + channel] = (value as u32 + under * (255 - alpha) / 255).min(255) as u8;
                }
            }
        }
    }

    /// `text` in a line of `height` pixels starting at (x, y), cut with `…` past `cols`.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        font: &mut Font,
        x: i32,
        y: i32,
        height: i32,
        text: &str,
        rgb: [u8; 3],
        cols: usize,
    ) {
        let mut chars: Vec<char> = text.chars().collect();
        if chars.len() > cols {
            chars.truncate(cols.saturating_sub(1));
            chars.push('…');
        }
        let cell = font.cell_width as i32;
        let baseline = y + (height - font.cell_height as i32) / 2 + font.baseline;
        for (i, c) in chars.into_iter().enumerate() {
            let Some(glyph) = font.glyph(c, Style::Regular) else { continue };
            let (left, top) = (x + i as i32 * cell + glyph.left, baseline - glyph.top);
            for gy in 0..glyph.height as i32 {
                for gx in 0..glyph.width as i32 {
                    let (px, py) = (left + gx, top + gy);
                    if px < 0 || py < 0 || px >= self.w || py >= self.h {
                        continue;
                    }
                    let g = (gy * glyph.width as i32 + gx) as usize;
                    let (color, alpha) = if glyph.color {
                        let p = &glyph.data[g * 4..g * 4 + 4];
                        ([p[0], p[1], p[2]], p[3])
                    } else {
                        (rgb, glyph.data[g])
                    };
                    let at = ((py * self.w + px) * 4) as usize;
                    for (channel, value) in [(2, color[0]), (1, color[1]), (0, color[2])] {
                        let under = self.pixels[at + channel] as u32;
                        self.pixels[at + channel] = ((value as u32 * alpha as u32 + under * (255 - alpha as u32)) / 255) as u8;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_blended() {
        let mut canvas = Canvas { pixels: [0, 0, 0, 255].repeat(4), w: 2, h: 2 };
        // Opaque red, then half-transparent green (premultiplied).
        let icon = Icon { size: 1, pixels: vec![255, 0, 0, 255] };
        canvas.image(0, 0, &icon);
        canvas.image(1, 1, &Icon { size: 1, pixels: vec![0, 128, 0, 128] });
        canvas.image(5, 5, &icon); // off the canvas
        assert_eq!(&canvas.pixels[0..4], &[0, 0, 255, 255]); // B, G, R, A
        assert_eq!(&canvas.pixels[12..16], &[0, 128, 0, 255]);
    }

    #[test]
    fn rows_are_found_under_the_pointer() {
        let line = line_height(None) as f64;
        let top = PADDING as f64;
        assert_eq!(row_at(None, top + 0.5 * line, ROWS), None); // the prompt
        assert_eq!(row_at(None, top + 1.5 * line, ROWS), Some(0));
        assert_eq!(
            row_at(None, top + (ROWS as f64 + 0.5) * line, ROWS),
            Some(ROWS - 1)
        );
        assert_eq!(row_at(None, top + (ROWS as f64 + 1.5) * line, ROWS), None); // the footer
        assert_eq!(row_at(None, 1.0, ROWS), None);
        // Compact: no rows to click.
        assert_eq!(row_at(None, top + 1.5 * line, 0), None);
    }

    #[test]
    fn panel_fits_the_width() {
        let (w, h) = panel_size(None, 4000, ROWS);
        assert!(w > 2 * PADDING && h > 2 * PADDING);
        assert_eq!(panel_size(None, 300, ROWS).0, 300);
        // Compact: the prompt and the two footer lines.
        assert_eq!(panel_size(None, 4000, 0).1, 3 * line_height(None) + 2 * PADDING);
        let pixels = draw(
            None,
            &View {
                prompt: String::new(),
                rows: Vec::new(),
                status: String::new(),
                hint: "",
            },
            (w, h),
            ROWS,
        );
        assert_eq!(pixels.len(), (w * h * 4) as usize);
    }
}
