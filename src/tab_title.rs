//! The cell of a dock entry: its app's icon, with marks for a pin, tabs and sound; and
//! the label with its window title on a dark strip. And the bar of a tabbed container: its
//! tabs side by side, the shown one lighter.

use std::collections::HashMap;

use dune_common::{
    font::{Font, Style},
    icons::Icon,
};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::Transform,
};

use crate::layout::{Rect, TAB_HEIGHT, TabWidths, TileId, tab_strip};

/// Tab text size, in pixels.
const FONT_SIZE: f32 = 14.0;
const PADDING: i32 = 8;
const BACKGROUND: [u8; 3] = [0x38, 0x3d, 0x4d];
const TEXT: [u8; 3] = [0xd8, 0xdc, 0xe6];
/// A tabbed container's bar: behind its tabs, the shown tab, and the text of the others.
const STRIP_BACKGROUND: [u8; 3] = [0x1e, 0x20, 0x29];
const ACTIVE_TAB: [u8; 3] = [0x4c, 0x56, 0x6a];
const INACTIVE_TEXT: [u8; 3] = [0x90, 0x96, 0xa4];
/// The marks of a scrolled bar: how many tabs are past each end.
const MARK_TEXT: [u8; 3] = [0xfa, 0x9e, 0x38];
/// The titles in the fan of the dock's chosen column, in characters at most.
const FAN_TITLE_CHARS: usize = 24;

/// What a tabbed container's bar shows: its tabs' titles, which is shown, its size.
type Strip = (Vec<String>, usize, (i32, i32));

/// What a dock entry's bar shows: its title, size and sound (its speaker's waves).
type Bar = (String, (i32, i32), Option<u8>);

/// What a dock entry's cell shows (see `TabTitles::cell`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// Its app, whose icon it shows.
    pub app: String,
    /// The letter shown without an icon.
    pub letter: char,
    pub size: i32,
    pub pinned: bool,
    /// How many tabs its column has (0: a tile).
    pub tabs: usize,
    /// While its sound plays, the speaker's waves.
    pub sound: Option<u8>,
}

/// Rendered tab bars, redrawn only when their title or size changes.
pub struct TabTitles {
    /// Loaded on first use: fontconfig runs a process.
    font: Option<Option<Font>>,
    family: String,
    widths: TabWidths,
    bars: HashMap<TileId, (Bar, MemoryRenderBuffer)>,
    /// The titles of the tabs in the fan of the dock's chosen column, by their first tile.
    fan_bars: HashMap<TileId, (Bar, MemoryRenderBuffer)>,
    /// The cells of the dock's entries, by their shown tile.
    cells: HashMap<TileId, (Cell, MemoryRenderBuffer)>,
    /// The cells of the tabs in the fan of the dock's chosen column, by their first tile.
    fan_cells: HashMap<TileId, (Cell, MemoryRenderBuffer)>,
    /// Bars of tabbed containers, by their shown tab.
    strips: HashMap<TileId, (Strip, MemoryRenderBuffer)>,
    /// The marks at the edges of the screens, by their label, with their size.
    marks: HashMap<String, ((i32, i32), MemoryRenderBuffer)>,
}

impl TabTitles {
    pub fn new(family: &str, widths: TabWidths) -> Self {
        TabTitles {
            font: None,
            family: family.to_string(),
            widths,
            bars: HashMap::new(),
            fan_bars: HashMap::new(),
            cells: HashMap::new(),
            fan_cells: HashMap::new(),
            strips: HashMap::new(),
            marks: HashMap::new(),
        }
    }

    fn font(&mut self) -> Option<&mut Font> {
        load_font(&mut self.font, &self.family)
    }

    /// Forget the bars of containers whose shown tab is not in `active`.
    pub fn retain_strips(&mut self, active: &[TileId]) {
        self.strips.retain(|tile, _| active.contains(tile));
    }

    /// The bar of the tabbed container showing `active`, `size` pixels, with `titles`
    /// (the shown one at `shown`).
    pub fn strip(&mut self, active: TileId, titles: Vec<String>, shown: usize, size: (i32, i32)) -> &MemoryRenderBuffer {
        let strip = (titles, shown, size);
        if self.strips.get(&active).is_none_or(|(s, _)| *s != strip) {
            let widths = self.widths;
            let pixels = draw_strip(self.font(), &strip.0, shown, size, widths);
            let buffer = MemoryRenderBuffer::from_slice(&pixels, Fourcc::Argb8888, size, 1, Transform::Normal, None);
            self.strips.insert(active, (strip, buffer));
        }
        &self.strips[&active].1
    }

    /// The mark floating at the left (`‹3`) or right (`3›`) edge of a screen with `n` tiles
    /// past it, and its size.
    pub fn edge_mark(&mut self, left: bool, n: usize) -> (&MemoryRenderBuffer, (i32, i32)) {
        let label = if left { format!("‹{n}") } else { format!("{n}›") };
        self.mark(&label)
    }

    /// A mark with `label` (an edge's, the lock on private tiles), and its size.
    pub fn mark(&mut self, label: &str) -> (&MemoryRenderBuffer, (i32, i32)) {
        if !self.marks.contains_key(label) {
            let font = self.font();
            let cell = font.as_ref().map_or(8, |f| f.cell_width as i32);
            let size = (2 * PADDING + cell * label.chars().count() as i32, TAB_HEIGHT);
            let pixels = draw_mark(font, label, size);
            let buffer = MemoryRenderBuffer::from_slice(&pixels, Fourcc::Argb8888, size, 1, Transform::Normal, None);
            self.marks.insert(label.to_string(), (size, buffer));
        }
        let (size, buffer) = &self.marks[label];
        (buffer, *size)
    }

    /// Forget the bars and cells of tiles that are no longer in the dock shown.
    pub fn retain(&mut self, tiles: &[TileId]) {
        self.bars.retain(|tile, _| tiles.contains(tile));
        self.fan_bars.retain(|tile, _| tiles.contains(tile));
        self.cells.retain(|tile, _| tiles.contains(tile));
        self.fan_cells.retain(|tile, _| tiles.contains(tile));
    }

    /// How wide a bar showing all of `title` is, `max` at most.
    pub fn bar_width(&mut self, title: &str, max: i32) -> i32 {
        let cell = self.font().map_or(8, |f| f.cell_width as i32);
        (2 * PADDING + cell * title.chars().count() as i32).min(max)
    }

    /// The cell of the dock entry shown by `tile`: `icon`, drawn when `what` changes.
    pub fn cell(&mut self, tile: TileId, what: Cell, icon: Option<&Icon>) -> &MemoryRenderBuffer {
        cached_cell(&mut self.cells, load_font(&mut self.font, &self.family), tile, what, icon)
    }

    /// The cell of the tab `tile` leads in the fan of the dock's chosen column (see `cell`).
    pub fn fan_cell(&mut self, tile: TileId, what: Cell, icon: Option<&Icon>) -> &MemoryRenderBuffer {
        cached_cell(&mut self.fan_cells, load_font(&mut self.font, &self.family), tile, what, icon)
    }

    /// The bar of `tile`, `size` pixels, showing `title`. With `sound` (its window plays
    /// some), a speaker at the end of the title, with that many waves (0 to `WAVES`).
    pub fn bar(&mut self, tile: TileId, title: &str, size: (i32, i32), sound: Option<u8>) -> &MemoryRenderBuffer {
        cached_bar(&mut self.bars, load_font(&mut self.font, &self.family), tile, (title.to_string(), size, sound), BACKGROUND)
    }

    /// The title of the tab `tile` leads in the fan of the dock's chosen column (see `bar`):
    /// on the fan's own colour, no bar behind it, `FAN_TITLE_CHARS` characters at most.
    pub fn fan_bar(&mut self, tile: TileId, title: &str, size: (i32, i32), sound: Option<u8>) -> &MemoryRenderBuffer {
        let title = clip(title, FAN_TITLE_CHARS);
        cached_bar(&mut self.fan_bars, load_font(&mut self.font, &self.family), tile, (title, size, sound), STRIP_BACKGROUND)
    }
}

/// ARGB8888 pixels of the cell of a dock entry (see `TabTitles::cell`): its `icon` in the
/// middle (else its letter), a pin mark at the top right when pinned, a dot per tab (5 at
/// most) along the bottom of a column's, the speaker at the bottom right while it plays.
/// The font in `slot`, loaded on first use: `family`, else monospace.
fn load_font<'a>(slot: &'a mut Option<Option<Font>>, family: &str) -> Option<&'a mut Font> {
    slot.get_or_insert_with(|| {
        Font::load(family, FONT_SIZE)
            .or_else(|_| Font::load("monospace", FONT_SIZE))
            .inspect_err(|err| tracing::warn!("tab titles without text: {err}"))
            .ok()
    })
    .as_mut()
}

/// The bar of `tile` in `bars`, on `background`, drawn anew when `key` changed.
fn cached_bar<'a>(
    bars: &'a mut HashMap<TileId, (Bar, MemoryRenderBuffer)>,
    font: Option<&mut Font>,
    tile: TileId,
    key: Bar,
    background: [u8; 3],
) -> &'a MemoryRenderBuffer {
    if bars.get(&tile).is_none_or(|(k, _)| *k != key) {
        let pixels = bar_pixels(font, &key.0, key.1, key.2, background);
        let buffer = MemoryRenderBuffer::from_slice(&pixels, Fourcc::Argb8888, key.1, 1, Transform::Normal, None);
        bars.insert(tile, (key, buffer));
    }
    &bars[&tile].1
}

/// `title` cut to `max` characters, the last one `…` when cut.
fn clip(title: &str, max: usize) -> String {
    match title.chars().count() > max {
        true => title.chars().take(max.saturating_sub(1)).chain(std::iter::once('…')).collect(),
        false => title.to_string(),
    }
}

/// The cell of `tile` in `cells`, drawn anew when `what` changed.
fn cached_cell<'a>(
    cells: &'a mut HashMap<TileId, (Cell, MemoryRenderBuffer)>,
    font: Option<&mut Font>,
    tile: TileId,
    what: Cell,
    icon: Option<&Icon>,
) -> &'a MemoryRenderBuffer {
    if cells.get(&tile).is_none_or(|(k, _)| *k != what) {
        let pixels = cell_pixels(font, &what, icon);
        let size = (what.size, what.size);
        let buffer = MemoryRenderBuffer::from_slice(&pixels, Fourcc::Argb8888, size, 1, Transform::Normal, None);
        cells.insert(tile, (what, buffer));
    }
    &cells[&tile].1
}

fn cell_pixels(font: Option<&mut Font>, what: &Cell, icon: Option<&Icon>) -> Vec<u8> {
    let side = what.size.max(1);
    let w = side as usize;
    let mut pixels: Vec<u8> = [BACKGROUND[2], BACKGROUND[1], BACKGROUND[0], 0xff].repeat(w * w);
    match (icon, font) {
        (Some(icon), _) => {
            let (n, at) = (icon.size as i32, (side - icon.size as i32) / 2);
            for y in 0..n {
                for x in 0..n {
                    let (px, py) = (at + x, at + y);
                    if px < 0 || py < 0 || px >= side || py >= side {
                        continue;
                    }
                    // RGBA premultiplied over the background.
                    let src = &icon.pixels[((y * n + x) * 4) as usize..((y * n + x) * 4 + 4) as usize];
                    let under = 255 - src[3] as u32;
                    let at = ((py * side + px) * 4) as usize;
                    for (channel, value) in [(2, src[0]), (1, src[1]), (0, src[2])] {
                        pixels[at + channel] = (value as u32 + pixels[at + channel] as u32 * under / 255).min(255) as u8;
                    }
                }
            }
        }
        (None, Some(font)) => {
            let letter: String = what.letter.to_uppercase().collect();
            let x0 = (side - font.cell_width as i32) / 2 - PADDING;
            text(&mut pixels, (w, w), font, &letter, (x0, x0 + font.cell_width as i32 + 2 * PADDING), TEXT);
        }
        (None, None) => {}
    }
    if what.pinned {
        dot(&mut pixels, side, (side as f64 - 7.0, 7.0), 3.5, MARK_TEXT);
    }
    let dots = what.tabs.min(5) as i32;
    if what.tabs > 1 {
        let first = (side - (dots - 1) * 6) as f64 / 2.0;
        for i in 0..dots {
            dot(&mut pixels, side, (first + (i * 6) as f64, side as f64 - 4.0), 1.5, TEXT);
        }
    }
    if let Some(waves) = what.sound {
        speaker(&mut pixels, (side, side), (side - SPEAKER - 1, side - SPEAKER - 1), waves, TEXT);
    }
    pixels
}

/// Paint a disc of `radius` around `center` in `color` on a `side`×`side` ARGB8888 image.
fn dot(pixels: &mut [u8], side: i32, center: (f64, f64), radius: f64, color: [u8; 3]) {
    for y in 0..side {
        for x in 0..side {
            let d = (x as f64 + 0.5 - center.0).hypot(y as f64 + 0.5 - center.1);
            let alpha = ((radius + 0.5 - d).clamp(0.0, 1.0) * 255.0) as u32;
            if alpha == 0 {
                continue;
            }
            let at = ((y * side + x) * 4) as usize;
            for (channel, value) in [(2, color[0]), (1, color[1]), (0, color[2])] {
                let under = pixels[at + channel] as u32;
                pixels[at + channel] = ((value as u32 * alpha + under * (255 - alpha)) / 255) as u8;
            }
        }
    }
}

/// ARGB8888 pixels of the bar of a dock entry (see `TabTitles::bar`): the title, and the
/// speaker at its end while it plays.
fn bar_pixels(font: Option<&mut Font>, title: &str, (w, h): (i32, i32), sound: Option<u8>, background: [u8; 3]) -> Vec<u8> {
    let end = if sound.is_some() { w - SPEAKER - PADDING } else { w };
    let mut pixels = draw(font, title, (w, h), (0, end), background);
    if let Some(waves) = sound {
        speaker(&mut pixels, (w, h), (w - PADDING - SPEAKER, (h - SPEAKER) / 2), waves, TEXT);
    }
    pixels
}

/// ARGB8888 pixels (B, G, R, A in memory) of a bar on `background` with `title` on it
/// between `x0` and `x1`, cut with `…` when it does not fit.
fn draw(font: Option<&mut Font>, title: &str, (w, h): (i32, i32), (x0, x1): (i32, i32), background: [u8; 3]) -> Vec<u8> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let mut pixels: Vec<u8> = [background[2], background[1], background[0], 0xff].repeat(w * h);
    if let Some(font) = font {
        text(&mut pixels, (w, h), font, title, (x0.max(0), x1.max(0)), TEXT);
    }
    pixels
}

/// Side of the speaker drawn on the bar of a tile playing sound, in pixels.
const SPEAKER: i32 = 16;

/// The sound waves of the speaker at most: while the sound plays they come one after
/// another (none, one, both) and start again.
pub const WAVES: u8 = 2;

/// Paint a speaker with `waves` sound waves (inner first), `SPEAKER` pixels square, in
/// `color` with its top left corner `at` of a `w`×`h` ARGB8888 image (smoothed: each pixel
/// sampled 4×4 times).
fn speaker(pixels: &mut [u8], (w, h): (i32, i32), at: (i32, i32), waves: u8, color: [u8; 3]) {
    // In a 16×16 box: the magnet, the cone widening to the right, and two arcs around (8, 8).
    let inside = |x: f64, y: f64| {
        let magnet = (1.0..5.0).contains(&x) && (5.5..10.5).contains(&y);
        let cone = (5.0..8.5).contains(&x) && (y - 8.0).abs() <= 2.5 + (x - 5.0) * 1.3;
        let (dx, dy) = (x - 8.0, y - 8.0);
        let r = dx.hypot(dy);
        let wave = dx > 0.0 && dy.abs() < dx * 1.2 && ((waves >= 1 && (r - 4.5).abs() < 0.8) || (waves >= 2 && (r - 7.0).abs() < 0.8));
        magnet || cone || wave
    };
    let scale = 16.0 / SPEAKER as f64;
    for py in 0..SPEAKER {
        for px in 0..SPEAKER {
            let (x, y) = (at.0 + px, at.1 + py);
            if x < 0 || y < 0 || x >= w || y >= h {
                continue;
            }
            let hits = (0..16).filter(|s| inside((px as f64 + (s % 4) as f64 / 4.0 + 0.125) * scale, (py as f64 + (s / 4) as f64 / 4.0 + 0.125) * scale)).count() as u32;
            if hits == 0 {
                continue;
            }
            let alpha = hits * 255 / 16;
            let at = ((y * w + x) * 4) as usize;
            for (channel, value) in [(2, color[0]), (1, color[1]), (0, color[2])] {
                let under = pixels[at + channel] as u32;
                pixels[at + channel] = ((value as u32 * alpha + under * (255 - alpha)) / 255) as u8;
            }
        }
    }
}

/// ARGB8888 pixels of a mark at a screen edge: `label` in the colour of a scrolled bar's
/// marks, on the bar's background.
fn draw_mark(font: Option<&mut Font>, label: &str, (w, h): (i32, i32)) -> Vec<u8> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let mut pixels: Vec<u8> = [STRIP_BACKGROUND[2], STRIP_BACKGROUND[1], STRIP_BACKGROUND[0], 0xff].repeat(w * h);
    if let Some(font) = font {
        text(&mut pixels, (w, h), font, label, (0, w as i32), MARK_TEXT);
    }
    pixels
}

/// ARGB8888 pixels of a tabbed container's bar: each title in its tab (see `tab_strip`),
/// the shown tab on a lighter background, and at the ends of a scrolled bar how many tabs
/// are past them (`‹3`, `5›`).
fn draw_strip(mut font: Option<&mut Font>, titles: &[String], shown: usize, (w, h): (i32, i32), widths: TabWidths) -> Vec<u8> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let mut pixels: Vec<u8> = [STRIP_BACKGROUND[2], STRIP_BACKGROUND[1], STRIP_BACKGROUND[0], 0xff].repeat(w * h);
    let bar = Rect::new(0, 0, w as i32, h as i32);
    let strip = tab_strip(bar, titles.len(), shown, widths);
    for (i, (title, tab)) in titles.iter().zip(strip.tabs.iter().copied()).enumerate().skip(strip.first).take(strip.count) {
        // A pixel between tabs.
        let (x0, x1) = (tab.x, (tab.x + tab.w - 1).max(tab.x + 1));
        if i == shown {
            for y in 0..h {
                for x in x0.max(0) as usize..(x1 as usize).min(w) {
                    pixels[(y * w + x) * 4..(y * w + x) * 4 + 3].copy_from_slice(&[ACTIVE_TAB[2], ACTIVE_TAB[1], ACTIVE_TAB[0]]);
                }
            }
        }
        if let Some(font) = font.as_deref_mut() {
            let color = if i == shown { TEXT } else { INACTIVE_TEXT };
            text(&mut pixels, (w, h), font, title, (x0, x1), color);
        }
    }
    if let Some(font) = font.as_deref_mut() {
        for (mark, left) in [(strip.left, true), (strip.right, false)] {
            let Some((r, n)) = mark else { continue };
            let label = if left { format!("‹{n}") } else { format!("{n}›") };
            text(&mut pixels, (w, h), font, &label, (r.x, r.x + r.w), MARK_TEXT);
        }
    }
    // The "+": two strokes in the middle of its square.
    let plus = strip.plus;
    let (cx, cy, arm) = (plus.x + plus.w / 2, plus.y + plus.h / 2, plus.h / 4);
    for (x, y, sw, sh) in [(cx - arm, cy - 1, 2 * arm, 2), (cx - 1, cy - arm, 2, 2 * arm)] {
        for py in y.max(0)..(y + sh).min(h as i32) {
            for px in x.max(0)..(x + sw).min(w as i32) {
                let i = (py as usize * w + px as usize) * 4;
                pixels[i..i + 3].copy_from_slice(&[INACTIVE_TEXT[2], INACTIVE_TEXT[1], INACTIVE_TEXT[0]]);
            }
        }
    }
    pixels
}

/// Write `title` in `color` between `x0` and `x1` of a `w`×`h` bar, cut with `…` when it
/// does not fit.
fn text(pixels: &mut [u8], (w, h): (usize, usize), font: &mut Font, title: &str, (x0, x1): (i32, i32), color: [u8; 3]) {
    let cell = font.cell_width as i32;
    let fits = ((x1 - x0 - 2 * PADDING) / cell.max(1)).max(0) as usize;
    let mut chars: Vec<char> = title.chars().collect();
    if chars.len() > fits {
        chars.truncate(fits.saturating_sub(1));
        chars.push('…');
    }
    let baseline = (h as i32 - font.cell_height as i32) / 2 + font.baseline;

    for (i, c) in chars.into_iter().enumerate() {
        let Some(glyph) = font.glyph(c, Style::Regular) else { continue };
        let (left, top) = (x0 + PADDING + i as i32 * cell + glyph.left, baseline - glyph.top);
        for gy in 0..glyph.height as i32 {
            for gx in 0..glyph.width as i32 {
                let (x, y) = (left + gx, top + gy);
                if x < x0 || y < 0 || x >= x1.min(w as i32) || y >= h as i32 {
                    continue;
                }
                let g = (gy * glyph.width as i32 + gx) as usize;
                let (rgb, alpha) = if glyph.color {
                    let p = &glyph.data[g * 4..g * 4 + 4];
                    ([p[0], p[1], p[2]], p[3])
                } else {
                    (color, glyph.data[g])
                };
                let at = (y as usize * w + x as usize) * 4;
                for (channel, value) in [(2, rgb[0]), (1, rgb[1]), (0, rgb[2])] {
                    let under = pixels[at + channel] as u32;
                    pixels[at + channel] = ((value as u32 * alpha as u32 + under * (255 - alpha as u32)) / 255) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_shows_the_icon_and_its_marks() {
        let what = Cell { app: "x".into(), letter: 'x', size: 48, pinned: false, tabs: 0, sound: None };
        let px = |p: &[u8], x: usize, y: usize| p[(y * 48 + x) * 4..(y * 48 + x) * 4 + 3].to_vec();
        let back = vec![BACKGROUND[2], BACKGROUND[1], BACKGROUND[0]];
        // A red icon, half of it see-through, in the middle.
        let mut pixels = [255, 0, 0, 255].repeat(36 * 36);
        pixels[..4].copy_from_slice(&[0, 0, 0, 0]);
        let icon = Icon { size: 36, pixels };
        let p = cell_pixels(None, &what, Some(&icon));
        assert_eq!(px(&p, 2, 2), back);
        assert_eq!(px(&p, 6, 6), back, "see-through");
        assert_eq!(px(&p, 24, 24), vec![0, 0, 255]);
        let pin = vec![MARK_TEXT[2], MARK_TEXT[1], MARK_TEXT[0]];
        assert_ne!(px(&p, 41, 6), pin);
        let pinned = cell_pixels(None, &Cell { pinned: true, tabs: 3, ..what }, None);
        assert_eq!(px(&pinned, 41, 6), pin);
        // Three tabs: three dots along the bottom, the middle one centered.
        assert_eq!(px(&pinned, 24, 44), vec![TEXT[2], TEXT[1], TEXT[0]]);
        assert_eq!(px(&pinned, 24, 30), back);
    }

    #[test]
    fn strip_lights_the_shown_tab() {
        let size = (600, 24);
        let titles = vec!["zsh".to_string(), "firefox".to_string()];
        let px = |p: &[u8], x: usize, y: usize| p[(y * 600 + x) * 4..(y * 600 + x) * 4 + 3].to_vec();
        let active = vec![ACTIVE_TAB[2], ACTIVE_TAB[1], ACTIVE_TAB[0]];
        let back = vec![STRIP_BACKGROUND[2], STRIP_BACKGROUND[1], STRIP_BACKGROUND[0]];

        let second = draw_strip(None, &titles, 1, size, TabWidths::default());
        // Tabs from the left at most TAB_MAX_WIDTH wide: the second one is lit, the rest not.
        assert_eq!(px(&second, 2, 1), back);
        assert_eq!(px(&second, crate::layout::TAB_MAX_WIDTH as usize + 2, 1), active);
        assert_eq!(px(&second, 590, 1), back);

        let mut font = Font::load("monospace", FONT_SIZE).expect("a monospace font");
        let text = draw_strip(Some(&mut font), &titles, 0, size, TabWidths::default());
        let blank = draw_strip(None, &titles, 0, size, TabWidths::default());
        let differs = |x0: usize, x1: usize| (0..24).any(|y| (x0..x1).any(|x| px(&text, x, y) != px(&blank, x, y)));
        let tab = crate::layout::TAB_MAX_WIDTH as usize;
        assert!(differs(0, tab) && differs(tab, 2 * tab), "both titles drawn");
        assert!(!differs(2 * tab, 600), "nothing past the tabs");
    }

    #[test]
    fn an_edge_mark_has_its_label_whole() {
        let mut titles = TabTitles::new("monospace", TabWidths::default());
        let (_, size) = titles.edge_mark(false, 12);
        let mut font = Font::load("monospace", FONT_SIZE).expect("a monospace font");
        assert_eq!(size, (2 * PADDING + 3 * font.cell_width as i32, TAB_HEIGHT));
        let text = draw_mark(Some(&mut font), "12›", size);
        let blank = draw_mark(None, "12›", size);
        let w = size.0 as usize;
        // The last character (the arrow) is drawn, not cut into `…`.
        let arrow = (w - PADDING as usize - font.cell_width as usize)..w;
        assert!((0..TAB_HEIGHT as usize).any(|y| arrow.clone().any(|x| text[(y * w + x) * 4] != blank[(y * w + x) * 4])));
    }

    #[test]
    fn a_scrolled_strip_marks_its_ends() {
        use crate::layout::{ARROW_WIDTH, PLUS_WIDTH, TAB_MIN_WIDTH};
        let w = (4 * TAB_MIN_WIDTH + 2 * ARROW_WIDTH + PLUS_WIDTH) as usize;
        let titles: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        let mut font = Font::load("monospace", FONT_SIZE).expect("a monospace font");
        let blank = draw_strip(None, &titles, 0, (w as i32, 24), TabWidths::default());
        let marked = |p: &[u8], x0: usize, x1: usize| (0..24).any(|y| (x0..x1).any(|x| p[(y * w + x) * 4] != blank[(y * w + x) * 4]));
        let right = ARROW_WIDTH as usize + 4 * TAB_MIN_WIDTH as usize;
        let first = draw_strip(Some(&mut font), &titles, 0, (w as i32, 24), TabWidths::default());
        assert!(!marked(&first, 0, ARROW_WIDTH as usize), "nothing to the left");
        assert!(marked(&first, right, right + ARROW_WIDTH as usize), "tabs to the right");
        let middle = draw_strip(Some(&mut font), &titles, 5, (w as i32, 24), TabWidths::default());
        assert!(marked(&middle, 0, ARROW_WIDTH as usize) && marked(&middle, right, right + ARROW_WIDTH as usize));
    }


    #[test]
    fn a_bar_playing_sound_has_a_speaker_at_the_end() {
        let lit = |px: &[u8]| px.chunks(4).filter(|p| p[0] > BACKGROUND[2] + 40).count();
        let mut titles = TabTitles::new("monospace", TabWidths::default());
        // The speaker at the right end, nothing else (no font: no title).
        let (w, h) = (300, 36);
        let pixels = bar_pixels(None, "mpv", (w, h), Some(WAVES), BACKGROUND);
        let cols = |from: i32, to: i32| (0..h).map(|y| lit(&pixels[((y * w + from) * 4) as usize..((y * w + to) * 4) as usize])).sum::<usize>();
        assert!(cols(w - PADDING - SPEAKER, w - PADDING) > 40, "the speaker is drawn");
        assert_eq!(cols(0, w - PADDING - SPEAKER) + cols(w - PADDING, w), 0, "and nothing else");
        // The bar is drawn again when the sound starts or stops, and as its waves come.
        titles.bar(1, "mpv", (w, h), None);
        assert_eq!(titles.bars[&1].0.2, None);
        titles.bar(1, "mpv", (w, h), Some(1));
        assert_eq!(titles.bars[&1].0.2, Some(1));
    }

    #[test]
    fn the_speaker_waves_come_one_after_another() {
        let lit = |px: &[u8]| px.chunks(4).filter(|p| p[0] > BACKGROUND[2] + 40).count();
        let drawn = |waves| {
            let mut pixels = draw(None, "", (SPEAKER, SPEAKER), (0, 0), BACKGROUND);
            speaker(&mut pixels, (SPEAKER, SPEAKER), (0, 0), waves, TEXT);
            lit(&pixels)
        };
        assert!(drawn(0) > 20, "the speaker alone");
        assert!(drawn(1) > drawn(0), "then the inner wave");
        assert!(drawn(2) > drawn(1), "then both");
    }


    #[test]
    fn bar_has_text_on_the_background() {
        let size = (200, 24);
        let blank = draw(None, "zsh", size, (0, size.0), BACKGROUND);
        assert_eq!(blank.len(), 200 * 24 * 4);
        assert!(blank.chunks(4).all(|p| p == [BACKGROUND[2], BACKGROUND[1], BACKGROUND[0], 0xff]));

        let mut font = Font::load("monospace", FONT_SIZE).expect("a monospace font");
        let text = draw(Some(&mut font), "zsh", size, (0, size.0), BACKGROUND);
        let lit = |px: &[u8]| px.chunks(4).filter(|p| p[0] > BACKGROUND[2] + 40).count();
        assert!(lit(&text) > 20, "the title is drawn");
        // Too long for the bar: cut, nothing drawn past the right padding.
        let long = draw(Some(&mut font), &"x".repeat(100), (100, 24), (0, 100), BACKGROUND);
        for row in long.chunks(100 * 4) {
            assert!(lit(&row[(100 - PADDING as usize + 1) * 4..]) == 0);
        }
    }

    #[test]
    fn fan_titles_are_clipped_to_their_characters() {
        assert_eq!(clip("zsh", 4), "zsh");
        assert_eq!(clip("abcd", 4), "abcd");
        assert_eq!(clip("abcdef", 4), "abc…");
    }
}
