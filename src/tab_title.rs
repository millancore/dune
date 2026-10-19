//! The bar of a tile folded into a tab: its window title on a dark strip.

use std::collections::HashMap;

use dune_common::font::{Font, Style};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::Transform,
};

use crate::layout::TileId;

/// Tab text size, in pixels.
const FONT_SIZE: f32 = 14.0;
const PADDING: i32 = 8;
const BACKGROUND: [u8; 3] = [0x38, 0x3d, 0x4d];
const TEXT: [u8; 3] = [0xd8, 0xdc, 0xe6];

/// Rendered tab bars, redrawn only when their title or size changes.
pub struct TabTitles {
    /// Loaded on first use: fontconfig runs a process.
    font: Option<Option<Font>>,
    family: String,
    bars: HashMap<TileId, (String, (i32, i32), MemoryRenderBuffer)>,
}

impl TabTitles {
    pub fn new(family: &str) -> Self {
        TabTitles {
            font: None,
            family: family.to_string(),
            bars: HashMap::new(),
        }
    }

    /// Forget the bars of tiles that are no longer tabs.
    pub fn retain(&mut self, tabs: &[TileId]) {
        self.bars.retain(|tile, _| tabs.contains(tile));
    }

    /// The bar of `tile`, `size` pixels, showing `title`.
    pub fn bar(&mut self, tile: TileId, title: &str, size: (i32, i32)) -> &MemoryRenderBuffer {
        let fresh = self.bars.get(&tile).is_some_and(|(t, s, _)| t == title && *s == size);
        if !fresh {
            let family = &self.family;
            let font = self.font.get_or_insert_with(|| {
                Font::load(family, FONT_SIZE)
                    .or_else(|_| Font::load("monospace", FONT_SIZE))
                    .inspect_err(|err| tracing::warn!("tab titles without text: {err}"))
                    .ok()
            });
            let pixels = draw(font.as_mut(), title, size);
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
    let Some(font) = font else { return pixels };

    let cell = font.cell_width as i32;
    let fits = ((w as i32 - 2 * PADDING) / cell.max(1)).max(0) as usize;
    let mut chars: Vec<char> = title.chars().collect();
    if chars.len() > fits {
        chars.truncate(fits.saturating_sub(1));
        chars.push('…');
    }
    let baseline = (h as i32 - font.cell_height as i32) / 2 + font.baseline;

    for (i, c) in chars.into_iter().enumerate() {
        let Some(glyph) = font.glyph(c, Style::Regular) else { continue };
        let (left, top) = (PADDING + i as i32 * cell + glyph.left, baseline - glyph.top);
        for gy in 0..glyph.height as i32 {
            for gx in 0..glyph.width as i32 {
                let (x, y) = (left + gx, top + gy);
                if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                    continue;
                }
                let g = (gy * glyph.width as i32 + gx) as usize;
                let (rgb, alpha) = if glyph.color {
                    let p = &glyph.data[g * 4..g * 4 + 4];
                    ([p[0], p[1], p[2]], p[3])
                } else {
                    (TEXT, glyph.data[g])
                };
                let at = (y as usize * w + x as usize) * 4;
                for (channel, value) in [(2, rgb[0]), (1, rgb[1]), (0, rgb[2])] {
                    let under = pixels[at + channel] as u32;
                    pixels[at + channel] = ((value as u32 * alpha as u32 + under * (255 - alpha as u32)) / 255) as u8;
                }
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

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
