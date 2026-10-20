//! The bar of a tile folded into a tab: its window title on a dark strip. And the bar
//! of a tabbed container: its tabs side by side, the shown one lighter.

use std::collections::HashMap;

use dune_common::font::{Font, Style};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::Transform,
};

use crate::layout::{Rect, TileId, plus_rect, tab_rects};

/// Tab text size, in pixels.
const FONT_SIZE: f32 = 14.0;
const PADDING: i32 = 8;
const BACKGROUND: [u8; 3] = [0x38, 0x3d, 0x4d];
const TEXT: [u8; 3] = [0xd8, 0xdc, 0xe6];
/// A tabbed container's bar: behind its tabs, the shown tab, and the text of the others.
const STRIP_BACKGROUND: [u8; 3] = [0x1e, 0x20, 0x29];
const ACTIVE_TAB: [u8; 3] = [0x4c, 0x56, 0x6a];
const INACTIVE_TEXT: [u8; 3] = [0x90, 0x96, 0xa4];

/// What a tabbed container's bar shows: its tabs' titles, which is shown, its size.
type Strip = (Vec<String>, usize, (i32, i32));

/// Rendered tab bars, redrawn only when their title or size changes.
pub struct TabTitles {
    /// Loaded on first use: fontconfig runs a process.
    font: Option<Option<Font>>,
    family: String,
    bars: HashMap<TileId, (String, (i32, i32), MemoryRenderBuffer)>,
    /// Bars of tabbed containers, by their shown tab.
    strips: HashMap<TileId, (Strip, MemoryRenderBuffer)>,
}

impl TabTitles {
    pub fn new(family: &str) -> Self {
        TabTitles {
            font: None,
            family: family.to_string(),
            bars: HashMap::new(),
            strips: HashMap::new(),
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
            let pixels = draw_strip(self.font(), &strip.0, shown, size);
            let buffer = MemoryRenderBuffer::from_slice(&pixels, Fourcc::Argb8888, size, 1, Transform::Normal, None);
            self.strips.insert(active, (strip, buffer));
        }
        &self.strips[&active].1
    }

    /// Forget the bars of tiles that are no longer tabs.
    pub fn retain(&mut self, tabs: &[TileId]) {
        self.bars.retain(|tile, _| tabs.contains(tile));
    }

    /// The bar of `tile`, `size` pixels, showing `title`.
    pub fn bar(&mut self, tile: TileId, title: &str, size: (i32, i32)) -> &MemoryRenderBuffer {
        let fresh = self.bars.get(&tile).is_some_and(|(t, s, _)| t == title && *s == size);
        if !fresh {
            let pixels = draw(self.font(), title, size);
            let buffer = MemoryRenderBuffer::from_slice(&pixels, Fourcc::Argb8888, size, 1, Transform::Normal, None);
            self.bars.insert(tile, (title.to_string(), size, buffer));
        }
        &self.bars[&tile].2
    }
}

/// ARGB8888 pixels (B, G, R, A in memory) of a bar with `title` on it, cut with `…`
/// when it does not fit.
fn draw(font: Option<&mut Font>, title: &str, (w, h): (i32, i32)) -> Vec<u8> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let mut pixels: Vec<u8> = [BACKGROUND[2], BACKGROUND[1], BACKGROUND[0], 0xff].repeat(w * h);
    if let Some(font) = font {
        text(&mut pixels, (w, h), font, title, (0, w as i32), TEXT);
    }
    pixels
}

/// ARGB8888 pixels of a tabbed container's bar: each title in its tab (see `tab_rects`),
/// the shown tab on a lighter background.
fn draw_strip(mut font: Option<&mut Font>, titles: &[String], shown: usize, (w, h): (i32, i32)) -> Vec<u8> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let mut pixels: Vec<u8> = [STRIP_BACKGROUND[2], STRIP_BACKGROUND[1], STRIP_BACKGROUND[0], 0xff].repeat(w * h);
    let bar = Rect::new(0, 0, w as i32, h as i32);
    for (i, (title, tab)) in titles.iter().zip(tab_rects(bar, titles.len())).enumerate() {
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
    // The "+": two strokes in the middle of its square.
    let plus = plus_rect(bar, titles.len());
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

        let second = draw_strip(None, &titles, 1, size);
        // Tabs from the left at most TAB_MAX_WIDTH wide: the second one is lit, the rest not.
        assert_eq!(px(&second, 2, 1), back);
        assert_eq!(px(&second, crate::layout::TAB_MAX_WIDTH as usize + 2, 1), active);
        assert_eq!(px(&second, 590, 1), back);

        let mut font = Font::load("monospace", FONT_SIZE).expect("a monospace font");
        let text = draw_strip(Some(&mut font), &titles, 0, size);
        let blank = draw_strip(None, &titles, 0, size);
        let differs = |x0: usize, x1: usize| (0..24).any(|y| (x0..x1).any(|x| px(&text, x, y) != px(&blank, x, y)));
        let tab = crate::layout::TAB_MAX_WIDTH as usize;
        assert!(differs(0, tab) && differs(tab, 2 * tab), "both titles drawn");
        assert!(!differs(2 * tab, 600), "nothing past the tabs");
    }

    #[test]
    fn bar_has_text_on_the_background() {
        let size = (200, 24);
        let blank = draw(None, "zsh", size);
        assert_eq!(blank.len(), 200 * 24 * 4);
        assert!(blank.chunks(4).all(|p| p == [BACKGROUND[2], BACKGROUND[1], BACKGROUND[0], 0xff]));

        let mut font = Font::load("monospace", FONT_SIZE).expect("a monospace font");
        let text = draw(Some(&mut font), "zsh", size);
        let lit = |px: &[u8]| px.chunks(4).filter(|p| p[0] > BACKGROUND[2] + 40).count();
        assert!(lit(&text) > 20, "the title is drawn");
        // Too long for the bar: cut, nothing drawn past the right padding.
        let long = draw(Some(&mut font), &"x".repeat(100), (100, 24));
        for row in long.chunks(100 * 4) {
            assert!(lit(&row[(100 - PADDING as usize + 1) * 4..]) == 0);
        }
    }
}
