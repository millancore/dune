//! The bar of a tile folded into a tab: its window title on a dark strip. And the bar
//! of a tabbed container: its tabs side by side, the shown one lighter.

use std::collections::HashMap;

use dune_common::font::{Font, Style};
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

/// What a tabbed container's bar shows: its tabs' titles, which is shown, its size.
type Strip = (Vec<String>, usize, (i32, i32));

/// What a folded tile's bar shows: its title, size, sound (its speaker's waves) and whether the title leads.
type Bar = (String, (i32, i32), Option<u8>, bool);

/// Rendered tab bars, redrawn only when their title or size changes.
pub struct TabTitles {
    /// Loaded on first use: fontconfig runs a process.
    font: Option<Option<Font>>,
    family: String,
    widths: TabWidths,
    bars: HashMap<TileId, (Bar, MemoryRenderBuffer)>,
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
            strips: HashMap::new(),
            marks: HashMap::new(),
        }
    }

    fn font(&mut self) -> Option<&mut Font> {
        let family = &self.family;
        self.font
            .get_or_insert_with(|| {
                Font::load(family, FONT_SIZE)
                    .or_else(|_| Font::load("monospace", FONT_SIZE))
                    .inspect_err(|err| tracing::warn!("tab titles without text: {err}"))
                    .ok()
            })
            .as_mut()
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

    /// Forget the bars of tiles that are no longer tabs.
    pub fn retain(&mut self, tabs: &[TileId]) {
        self.bars.retain(|tile, _| tabs.contains(tile));
    }

    /// The bar of `tile`, `size` pixels, showing `title`. A bar taller than wide (a tile
    /// folded inside its row) has its title turned, read from the bottom up. With `sound`
    /// (its window plays some), a speaker at the end of the title, with that many waves
    /// (0 to `WAVES`): on top of an upright bar. A `leading` title (a folded tabbed
    /// column's tab count) goes at that end instead, the speaker after it; a number, it is
    /// not turned.
    pub fn bar(&mut self, tile: TileId, title: &str, size: (i32, i32), sound: Option<u8>, leading: bool) -> &MemoryRenderBuffer {
        let key = (title.to_string(), size, sound, leading);
        if self.bars.get(&tile).is_none_or(|(k, _)| *k != key) {
            let pixels = bar_pixels(self.font(), title, size, sound, leading);
            let buffer = MemoryRenderBuffer::from_slice(&pixels, Fourcc::Argb8888, size, 1, Transform::Normal, None);
            self.bars.insert(tile, (key, buffer));
        }
        &self.bars[&tile].1
    }
}

/// ARGB8888 pixels of the bar of a folded tile (see `TabTitles::bar`).
fn bar_pixels(mut font: Option<&mut Font>, title: &str, (w, h): (i32, i32), sound: Option<u8>, leading: bool) -> Vec<u8> {
    let long = w.max(h);
    let cell = font.as_ref().map_or(8, |f| f.cell_width as i32);
    // The title's span along the bar, and how far from its end the speaker goes.
    let (span, speaker_at) = if leading {
        let width = (2 * PADDING + cell * title.chars().count() as i32).min(long);
        ((long - width, long), width)
    } else {
        ((0, if sound.is_some() { long - SPEAKER - PADDING } else { long }), PADDING)
    };
    let mut pixels = if h > w && leading {
        // The count read as it is, centered across the top of the bar.
        let mut pixels = draw(None, "", (w, h), (0, 0));
        if let Some(font) = font {
            let band = speaker_at.min(h).max(0) as usize;
            let x0 = ((w - cell * title.chars().count() as i32) / 2 - PADDING).max(0);
            text(&mut pixels[..band * w.max(1) as usize * 4], (w.max(1) as usize, band), font, title, (x0, w), TEXT);
        }
        pixels
    } else if h > w {
        upright(&draw(font.take(), title, (h, w), span), (h, w))
    } else {
        draw(font, title, (w, h), span)
    };
    if let Some(waves) = sound {
        let at = if h > w { ((w - SPEAKER) / 2, speaker_at) } else { (w - speaker_at - SPEAKER, (h - SPEAKER) / 2) };
        speaker(&mut pixels, (w, h), at, waves, TEXT);
    }
    pixels
}

/// ARGB8888 pixels (B, G, R, A in memory) of a bar with `title` on it between `x0` and
/// `x1`, cut with `…` when it does not fit.
fn draw(font: Option<&mut Font>, title: &str, (w, h): (i32, i32), (x0, x1): (i32, i32)) -> Vec<u8> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let mut pixels: Vec<u8> = [BACKGROUND[2], BACKGROUND[1], BACKGROUND[0], 0xff].repeat(w * h);
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

/// The ARGB8888 `pixels` of a `(w, h)` image turned a quarter counter-clockwise: an image
/// `h` wide and `w` tall whose left edge was the bottom.
fn upright(pixels: &[u8], (w, h): (i32, i32)) -> Vec<u8> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let mut out = vec![0; pixels.len()];
    for y in 0..w {
        for x in 0..h {
            let from = (x * w + (w - 1 - y)) * 4;
            let to = (y * h + x) * 4;
            out[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
        }
    }
    out
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
    fn an_upright_image_is_turned_counter_clockwise() {
        // 3 wide, 2 tall: a b c / d e f, one byte per pixel kept in the first channel.
        let px = |v: &[u8]| v.iter().flat_map(|b| [*b, 0, 0, 0]).collect::<Vec<u8>>();
        let turned = upright(&px(b"abcdef"), (3, 2));
        // 2 wide, 3 tall: c f / b e / a d (the left column ends at the bottom).
        assert_eq!(turned, px(b"cfbead"));
    }

    #[test]
    fn a_bar_playing_sound_has_a_speaker_at_the_end() {
        let lit = |px: &[u8]| px.chunks(4).filter(|p| p[0] > BACKGROUND[2] + 40).count();
        let mut titles = TabTitles::new("monospace", TabWidths::default());
        // Upright (folded in its row): the speaker on top, the title kept below it.
        let (w, h) = (36, 300);
        let mut pixels = upright(&draw(None, "", (h, w), (0, h - SPEAKER - PADDING)), (h, w));
        speaker(&mut pixels, (w, h), ((w - SPEAKER) / 2, PADDING), WAVES, TEXT);
        let rows = |from: i32, to: i32| lit(&pixels[(from * w * 4) as usize..(to * w * 4) as usize]);
        assert!(rows(PADDING, PADDING + SPEAKER) > 40, "the speaker is drawn");
        assert_eq!(rows(0, PADDING) + rows(PADDING + SPEAKER, h), 0, "and nothing else");
        // The bar is drawn again when the sound starts or stops, and as its waves come.
        titles.bar(1, "mpv", (w, h), None, false);
        assert_eq!(titles.bars[&1].0.2, None);
        titles.bar(1, "mpv", (w, h), Some(1), false);
        assert_eq!(titles.bars[&1].0.2, Some(1));
    }

    #[test]
    fn the_speaker_waves_come_one_after_another() {
        let lit = |px: &[u8]| px.chunks(4).filter(|p| p[0] > BACKGROUND[2] + 40).count();
        let drawn = |waves| {
            let mut pixels = draw(None, "", (SPEAKER, SPEAKER), (0, 0));
            speaker(&mut pixels, (SPEAKER, SPEAKER), (0, 0), waves, TEXT);
            lit(&pixels)
        };
        assert!(drawn(0) > 20, "the speaker alone");
        assert!(drawn(1) > drawn(0), "then the inner wave");
        assert!(drawn(2) > drawn(1), "then both");
    }

    #[test]
    fn a_leading_count_goes_on_top_and_the_speaker_after_it() {
        let lit = |px: &[u8]| px.chunks(4).filter(|p| p[0] > BACKGROUND[2] + 40).count();
        let mut font = Font::load("monospace", FONT_SIZE).expect("a monospace font");
        let count = 2 * PADDING + font.cell_width as i32;
        let (w, h) = (36, 300);
        let pixels = bar_pixels(Some(&mut font), "3", (w, h), Some(WAVES), true);
        let rows = |from: i32, to: i32| lit(&pixels[(from * w * 4) as usize..(to * w * 4) as usize]);
        assert!(rows(0, count) > 10, "the count on top");
        // Not turned: drawn like the same count on a flat bar, centered across the top.
        let mut flat = draw(None, "", (w, count), (0, 0));
        let x0 = ((w - font.cell_width as i32) / 2 - PADDING).max(0);
        text(&mut flat, (w as usize, count as usize), &mut font, "3", (x0, w), TEXT);
        assert_eq!(&pixels[..(count * w * 4) as usize], &flat[..]);
        assert!(rows(count, count + SPEAKER) > 40, "the speaker right after it");
        assert_eq!(rows(count + SPEAKER, h), 0, "nothing below");
    }

    #[test]
    fn bar_has_text_on_the_background() {
        let size = (200, 24);
        let blank = draw(None, "zsh", size, (0, size.0));
        assert_eq!(blank.len(), 200 * 24 * 4);
        assert!(blank.chunks(4).all(|p| p == [BACKGROUND[2], BACKGROUND[1], BACKGROUND[0], 0xff]));

        let mut font = Font::load("monospace", FONT_SIZE).expect("a monospace font");
        let text = draw(Some(&mut font), "zsh", size, (0, size.0));
        let lit = |px: &[u8]| px.chunks(4).filter(|p| p[0] > BACKGROUND[2] + 40).count();
        assert!(lit(&text) > 20, "the title is drawn");
        // Too long for the bar: cut, nothing drawn past the right padding.
        let long = draw(Some(&mut font), &"x".repeat(100), (100, 24), (0, 100));
        for row in long.chunks(100 * 4) {
            assert!(lit(&row[(100 - PADDING as usize + 1) * 4..]) == 0);
        }
    }
}
