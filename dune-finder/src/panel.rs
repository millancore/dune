//! Drawing the panel: a box with a prompt line and a list of rows (an icon in a square as
//! tall as the line, then text), into ARGB8888 pixels, its corners rounded. A compact panel
//! (no rows) is the prompt line alone. Where what is chosen opens is at the prompt line's
//! right: a note, and a mini tile for a side of the focused tile.

use dune_common::{
    font::{Font, Style},
    protocol::Side,
};

use crate::icons::Icon;

/// Text size, in pixels.
pub const FONT_SIZE: f32 = 18.0;
/// Size of a row's second line (`Row::sub`), in pixels.
pub const SMALL_FONT_SIZE: f32 = FONT_SIZE * 0.75;
/// Characters across the panel.
pub const COLUMNS: usize = 60;
pub const PADDING: i32 = 16;
/// Space around an icon, inside its line.
pub const ICON_MARGIN: i32 = 2;
/// How many pixels the panel's corners are rounded by.
pub const RADIUS: i32 = 6;
/// Rows of the list shown at once, in every mode.
pub const ROWS: usize = 10;
pub const BACKGROUND: [u8; 3] = [0x1e, 0x20, 0x30];
pub const SELECTED: [u8; 3] = [0x3b, 0x42, 0x61];
pub const TEXT: [u8; 3] = [0xd8, 0xdc, 0xe6];
pub const DIM: [u8; 3] = [0x7f, 0x84, 0x9c];
pub const ACCENT: [u8; 3] = [0xfa, 0x9e, 0x38];

/// Height of one line of text.
pub fn line_height(font: Option<&Font>) -> i32 {
    font.map_or(24, |f| f.cell_height as i32 + 6)
}

/// The prompt line, then a list `list` pixels tall; at most `max_width` wide.
pub fn panel_size(font: Option<&Font>, max_width: i32, list: i32) -> (i32, i32) {
    let cell = font.map_or(10, |f| f.cell_width as i32);
    let w = (COLUMNS as i32 * cell + 2 * PADDING)
        .min(max_width)
        .max(2 * PADDING + 1);
    let h = line_height(font) + list + 2 * PADDING;
    (w, h)
}

/// The one of `rows` under `y` (pixels from the panel's top), in a list `list` pixels tall.
pub fn row_at(font: Option<&Font>, y: f64, rows: &[Row], list: i32) -> Option<usize> {
    let mut top = (PADDING + line_height(font)) as f64;
    let bottom = top + list as f64;
    for (i, row) in rows.iter().enumerate() {
        let next = top + row.height(font) as f64;
        if next > bottom {
            break;
        }
        if (top..next).contains(&y) {
            return Some(i);
        }
        top = next;
    }
    None
}

/// A row of the list: its icon, its text and, dim at the right, a detail; with `sub`, a
/// second line under the text, dim, both smaller, by a bigger icon.
pub struct Row<'a> {
    pub icon: Option<&'a Icon>,
    pub text: &'a str,
    pub detail: Option<String>,
    pub sub: Option<String>,
    pub selected: bool,
}

impl Row<'_> {
    /// Its height in pixels: a line, or with `sub` a line and a half.
    pub fn height(&self, font: Option<&Font>) -> i32 {
        match self.sub {
            Some(_) => icon_size(font) as i32 * 3 / 2 + 2 * ICON_MARGIN,
            None => line_height(font),
        }
    }
}

/// The height of the first of `rows` that fit in `max` lines.
pub fn list_height(font: Option<&Font>, rows: &[Row], max: usize) -> i32 {
    let room = max as i32 * line_height(font);
    let mut height = 0;
    for row in rows {
        let next = height + row.height(font);
        if next > room {
            break;
        }
        height = next;
    }
    height
}

/// What the panel shows.
pub struct View<'a> {
    pub prompt: String,
    pub rows: Vec<Row<'a>>,
    /// Shown dim after the prompt while nothing is typed; empty: nothing.
    pub placeholder: &'a str,
    /// Where what is chosen opens, dim at the prompt line's right; empty: nothing to say.
    pub note: String,
    /// On that side of the focused tile: a mini tile at the prompt line's right shows it.
    pub side: Option<Side>,
    /// The keys, `key: what it does · ...`, listed by `?`.
    pub hint: &'a str,
}

/// The panel's ARGB8888 pixels (B, G, R, A in memory), `w` × `h`, with a list `list` pixels
/// tall; rows with a second line in `small` (none: in `font`).
pub fn draw(font: Option<&mut Font>, mut small: Option<&mut Font>, view: &View, (w, h): (i32, i32), list: i32) -> Vec<u8> {
    let mut canvas = Canvas {
        pixels: [BACKGROUND[2], BACKGROUND[1], BACKGROUND[0], 0xff].repeat((w * h) as usize),
        w,
        h,
    };
    let Some(font) = font else {
        canvas.round(RADIUS);
        return canvas.pixels;
    };

    let line = line_height(Some(font));
    let cell = font.cell_width.max(1) as i32;
    let cols = ((w - 2 * PADDING) / cell).max(1) as usize;
    let row_y = |row: i32| PADDING + row * line;

    canvas.text(font, PADDING, row_y(0), line, &view.prompt, TEXT, cols);
    let prompt_cols = view.prompt.chars().count();
    if !view.placeholder.is_empty() {
        let x = PADDING + prompt_cols as i32 * cell;
        canvas.text(font, x, row_y(0), line, view.placeholder, DIM, cols.saturating_sub(prompt_cols));
    }
    // From the right: the mini tile, then the note left of it.
    let mut right = w - PADDING;
    if let Some(side) = view.side {
        let (tile_w, tile_h) = (line * 3 / 2, line - 2 * ICON_MARGIN);
        right -= tile_w;
        canvas.mini_tile(right, row_y(0) + ICON_MARGIN, tile_w, tile_h, side);
        right -= cell;
    }
    let free = ((right - PADDING) / cell).max(0) as usize;
    let room = free.saturating_sub(prompt_cols + view.placeholder.chars().count() + 2);
    let note_cols = view.note.chars().count().min(room);
    if note_cols > 0 {
        let x = right - note_cols as i32 * cell;
        canvas.text(font, x, row_y(0), line, &view.note, DIM, note_cols);
    }
    let text_x = PADDING + line + ICON_MARGIN;
    let text_cols = cols
        .saturating_sub(((line + ICON_MARGIN) / cell) as usize)
        .max(1);
    let bottom = row_y(1) + list;
    let mut y = row_y(1);
    for row in &view.rows {
        let height = row.height(Some(font));
        if y + height > bottom {
            break;
        }
        if row.selected {
            canvas.fill(0, y, w, height, SELECTED);
        }
        if let Some(sub) = &row.sub {
            // Name and sub in the small font, the icon in the middle at their left.
            let icon_w = tall_icon_size(icon_size(Some(font))) as i32;
            if let Some(icon) = row.icon {
                let left = PADDING + ICON_MARGIN + (icon_w - icon.size as i32) / 2;
                canvas.image(left, y + (height - icon.size as i32) / 2, icon);
            }
            let x = PADDING + icon_w + 3 * ICON_MARGIN;
            let small = match small.as_deref_mut() {
                Some(small) => small,
                None => &mut *font,
            };
            let cell_h = small.cell_height as i32;
            let small_cols = ((w - PADDING - x) / small.cell_width.max(1) as i32).max(1) as usize;
            let top = y + (height - 2 * cell_h) / 2;
            canvas.text(small, x, top, cell_h, row.text, TEXT, small_cols);
            canvas.text(small, x, top + cell_h, cell_h, sub, DIM, small_cols);
            y += height;
            continue;
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
        y += height;
    }
    canvas.round(RADIUS);
    canvas.pixels
}

/// The size an icon is drawn at, for a line of text.
pub fn icon_size(font: Option<&Font>) -> u32 {
    (line_height(font) - 2 * ICON_MARGIN).max(1) as u32
}

/// The icon size of a row with a second line (`Row::sub`), for icons of `icon_size`.
pub fn tall_icon_size(icon_size: u32) -> u32 {
    icon_size * 5 / 4
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

    /// A tile split in two, `w` × `h` at (x, y): the focused one dim, the half on `side`,
    /// where the app opens, filled and framed in the accent colour.
    pub fn mini_tile(&mut self, x: i32, y: i32, w: i32, h: i32, side: Side) {
        let (half_w, half_h) = (w / 2, h / 2);
        let (old, new) = match side {
            Side::Left => ((x + half_w, y, w - half_w, h), (x, y, half_w, h)),
            Side::Right => ((x, y, half_w, h), (x + half_w, y, w - half_w, h)),
            Side::Up => ((x, y + half_h, w, h - half_h), (x, y, w, half_h)),
            Side::Down => ((x, y, w, half_h), (x, y + half_h, w, h - half_h)),
        };
        self.frame(old, DIM);
        self.fill(new.0, new.1, new.2, new.3, SELECTED);
        self.frame(new, ACCENT);
    }

    /// A one pixel wide outline of the rectangle (x, y, w, h).
    fn frame(&mut self, (x, y, w, h): (i32, i32, i32, i32), rgb: [u8; 3]) {
        self.fill(x, y, w, 1, rgb);
        self.fill(x, y + h - 1, w, 1, rgb);
        self.fill(x, y, 1, h, rgb);
        self.fill(x + w - 1, y, 1, h, rgb);
    }

    /// Round the corners by `radius`: clear what is past the curve, anti-aliased.
    pub fn round(&mut self, radius: i32) {
        let r = radius.min(self.w / 2).min(self.h / 2);
        if r <= 0 {
            return;
        }
        let outer = r as f32;
        for cy in 0..r {
            for cx in 0..r {
                // From the centre of the curve to the pixel's centre.
                let (dx, dy) = (r as f32 - cx as f32 - 0.5, r as f32 - cy as f32 - 0.5);
                let d = (dx * dx + dy * dy).sqrt();
                let coverage = (outer - d + 0.5).clamp(0.0, 1.0);
                for (px, py) in [(cx, cy), (self.w - 1 - cx, cy), (cx, self.h - 1 - cy), (self.w - 1 - cx, self.h - 1 - cy)] {
                    let at = ((py * self.w + px) * 4) as usize;
                    for channel in 0..3 {
                        // Premultiplied: the colour fades with the alpha.
                        self.pixels[at + channel] = (self.pixels[at + channel] as f32 * coverage).round() as u8;
                    }
                    self.pixels[at + 3] = (255.0 * coverage).round() as u8;
                }
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
    fn corners_are_rounded() {
        let (w, h) = (20, 10);
        let mut canvas = Canvas { pixels: [0, 0, 0, 255].repeat((w * h) as usize), w, h };
        canvas.round(RADIUS);
        let alpha = |x: i32, y: i32| canvas.pixels[((y * w + x) * 4 + 3) as usize];
        for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
            assert_eq!(alpha(x, y), 0);
        }
        assert_eq!(alpha(RADIUS, 0), 255); // past the curve, along the edge
        assert_eq!(alpha(RADIUS - 1, RADIUS - 1), 255); // inside the curve
    }

    #[test]
    fn the_mini_tile_frames_the_half_on_its_side() {
        let (w, h) = (12, 6);
        let mut canvas = Canvas { pixels: [0, 0, 0, 255].repeat((w * h) as usize), w, h };
        canvas.mini_tile(0, 0, w, h, Side::Right);
        let rgb = |x: i32, y: i32| {
            let at = ((y * w + x) * 4) as usize;
            [canvas.pixels[at + 2], canvas.pixels[at + 1], canvas.pixels[at]]
        };
        assert_eq!(rgb(0, 0), DIM);
        assert_eq!(rgb(w - 1, h / 2), ACCENT);
        assert_eq!(rgb(w * 3 / 4, h / 2), SELECTED);
    }

    fn row(sub: Option<&str>) -> Row<'static> {
        Row { icon: None, text: "", detail: None, sub: sub.map(String::from), selected: false }
    }

    #[test]
    fn rows_are_found_under_the_pointer() {
        let line = line_height(None);
        let tall = row(Some("sub")).height(None);
        assert!(line < tall && tall < 2 * line);
        let rows = [row(None), row(Some("sub")), row(None)];
        let list = list_height(None, &rows, ROWS);
        let top = PADDING as f64 + line as f64;
        assert_eq!(row_at(None, top - 1.0, &rows, list), None); // the prompt
        assert_eq!(row_at(None, top + 1.0, &rows, list), Some(0));
        assert_eq!(row_at(None, top + line as f64 + 1.0, &rows, list), Some(1));
        assert_eq!(row_at(None, top + (line + tall) as f64 + 1.0, &rows, list), Some(2));
        assert_eq!(row_at(None, top + (2 * line + tall) as f64 + 1.0, &rows, list), None); // under the list
        // Compact: no rows to click.
        assert_eq!(row_at(None, top + 1.0, &rows, 0), None);
    }

    #[test]
    fn the_list_is_as_tall_as_the_rows_that_fit() {
        let (line, tall) = (line_height(None), row(Some("sub")).height(None));
        assert_eq!(list_height(None, &[], ROWS), 0);
        assert_eq!(list_height(None, &[row(None), row(Some("sub"))], ROWS), line + tall);
        // A taller row does not fit in the last line.
        assert_eq!(list_height(None, &[row(None), row(Some("sub"))], 2), line);
    }

    #[test]
    fn panel_fits_the_width() {
        let list = ROWS as i32 * line_height(None);
        let (w, h) = panel_size(None, 4000, list);
        assert!(w > 2 * PADDING && h > 2 * PADDING);
        assert_eq!(panel_size(None, 300, list).0, 300);
        // Compact: the prompt line alone.
        assert_eq!(panel_size(None, 4000, 0).1, line_height(None) + 2 * PADDING);
        let pixels = draw(
            None,
            None,
            &View {
                prompt: String::new(),
                rows: Vec::new(),
                placeholder: "",
                note: String::new(),
                side: None,
                hint: "",
            },
            (w, h),
            list,
        );
        assert_eq!(pixels.len(), (w * h * 4) as usize);
    }
}
