//! The terminal font: faces (regular, bold, italic), per-character fallback fonts,
//! cell metrics and a cache of rasterized glyphs.

use std::collections::HashMap;

use swash::{
    FontRef,
    scale::{Render, ScaleContext, Source, StrikeWith, image::Content},
    zeno::{Format, Transform},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    Regular = 0,
    Bold = 1,
    Italic = 2,
    BoldItalic = 3,
}

impl Style {
    pub fn new(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (false, false) => Style::Regular,
            (true, false) => Style::Bold,
            (false, true) => Style::Italic,
            (true, true) => Style::BoldItalic,
        }
    }

    fn bold(self) -> bool {
        matches!(self, Style::Bold | Style::BoldItalic)
    }

    fn italic(self) -> bool {
        matches!(self, Style::Italic | Style::BoldItalic)
    }

    fn fontconfig_suffix(self) -> &'static str {
        ["", ":bold", ":italic", ":bold:italic"][self as usize]
    }
}

pub struct Glyph {
    /// Offset of the bitmap from the pen position (x right, y up from the baseline).
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    /// Coverage (one byte per pixel) or, for color glyphs, RGBA.
    pub data: Vec<u8>,
    pub color: bool,
}

pub struct Font {
    family: String,
    /// Regular, bold, italic, bold italic; `None` when the family has no such face
    /// (then the regular face is emboldened or slanted).
    faces: [Option<FontRef<'static>>; 4],
    /// Fonts used for characters the main family lacks, by file and index.
    fallback_fonts: HashMap<(String, u32), FontRef<'static>>,
    fallback_for: HashMap<char, Option<FontRef<'static>>>,
    context: ScaleContext,
    size: f32,
    pub cell_width: u32,
    pub cell_height: u32,
    /// Distance from the top of a cell to the baseline.
    pub baseline: i32,
    cache: HashMap<(char, Style), Option<Glyph>>,
}

/// Points to pixels at the usual 96 DPI.
pub fn points_to_pixels(points: f32) -> f32 {
    points * 96.0 / 72.0
}

impl Font {
    /// `family` as fontconfig resolves it, at `size` pixels.
    pub fn load(family: &str, size: f32) -> Result<Self, String> {
        let (path, index) = fontconfig_match(family).ok_or_else(|| format!("no font found for `{family}`"))?;
        let regular = load_face(&path, index).or_else(|| fontdb_face(family)).ok_or("cannot read the font")?;

        // The other faces only count when fontconfig found a real one, not the regular again.
        let mut faces = [Some(regular), None, None, None];
        for style in [Style::Bold, Style::Italic, Style::BoldItalic] {
            if let Some((p, i)) = fontconfig_match(&format!("{family}{}", style.fontconfig_suffix()))
                && (p.as_str(), i) != (path.as_str(), index)
            {
                faces[style as usize] = load_face(&p, i);
            }
        }

        let metrics = regular.metrics(&[]).scale(size);
        let advance = regular
            .glyph_metrics(&[])
            .scale(size)
            .advance_width(regular.charmap().map('M'));
        Ok(Font {
            family: family.to_string(),
            faces,
            fallback_fonts: HashMap::new(),
            fallback_for: HashMap::new(),
            context: ScaleContext::new(),
            size,
            cell_width: advance.round().max(1.0) as u32,
            cell_height: (metrics.ascent + metrics.descent + metrics.leading).ceil().max(1.0) as u32,
            baseline: metrics.ascent.round() as i32,
            cache: HashMap::new(),
        })
    }

    pub fn size(&self) -> f32 {
        self.size
    }

    pub fn family(&self) -> &str {
        &self.family
    }

    /// The rasterized glyph for `c`, or `None` when no font has it.
    pub fn glyph(&mut self, c: char, style: Style) -> Option<&Glyph> {
        if !self.cache.contains_key(&(c, style)) {
            let glyph = self.rasterize(c, style);
            self.cache.insert((c, style), glyph);
        }
        self.cache[&(c, style)].as_ref()
    }

    fn rasterize(&mut self, c: char, style: Style) -> Option<Glyph> {
        let regular = self.faces[0].unwrap();
        let (font, synthetic) = match self.faces[style as usize] {
            Some(face) if face.charmap().map(c) != 0 => (face, false),
            _ if regular.charmap().map(c) != 0 => (regular, true),
            _ => (self.fallback(c)?, true),
        };
        let id = font.charmap().map(c);

        let mut scaler = self.context.builder(font).size(self.size).hint(true).build();
        let mut render = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ]);
        render.format(Format::Alpha);
        if synthetic && style.bold() {
            render.embolden(self.size / 24.0);
        }
        if synthetic && style.italic() {
            render.transform(Some(Transform::skew(swash::zeno::Angle::from_degrees(12.0), swash::zeno::Angle::ZERO)));
        }
        let image = render.render(&mut scaler, id)?;
        let color = match image.content {
            Content::Mask => false,
            Content::Color => true,
            Content::SubpixelMask => return None,
        };
        let glyph = Glyph {
            left: image.placement.left,
            top: image.placement.top,
            width: image.placement.width,
            height: image.placement.height,
            data: image.data,
            color,
        };
        // Bitmap fonts (emoji) come in fixed sizes: fit them to two cells.
        Some(if color { self.fit_color(glyph) } else { glyph })
    }

    /// A font that has `c`, found through fontconfig and kept for later.
    fn fallback(&mut self, c: char) -> Option<FontRef<'static>> {
        if let Some(font) = self.fallback_for.get(&c) {
            return *font;
        }
        let font = fontconfig_match(&format!("{}:charset={:x}", self.family, c as u32)).and_then(|(path, index)| {
            if let Some(font) = self.fallback_fonts.get(&(path.clone(), index)) {
                return Some(*font);
            }
            let font = load_face(&path, index)?;
            self.fallback_fonts.insert((path, index), font);
            Some(font)
        });
        let font = font.filter(|f| f.charmap().map(c) != 0);
        self.fallback_for.insert(c, font);
        font
    }

    fn fit_color(&self, glyph: Glyph) -> Glyph {
        let max_h = self.cell_height;
        let max_w = 2 * self.cell_width;
        if glyph.height <= max_h && glyph.width <= max_w {
            return glyph;
        }
        let scale = (max_h as f32 / glyph.height as f32).min(max_w as f32 / glyph.width as f32);
        let (w, h) = (
            ((glyph.width as f32 * scale).round() as u32).max(1),
            ((glyph.height as f32 * scale).round() as u32).max(1),
        );
        let mut data = vec![0u8; (w * h * 4) as usize];
        // Box filter: average the source pixels each target pixel covers.
        for ty in 0..h {
            let (sy0, sy1) = (ty * glyph.height / h, ((ty + 1) * glyph.height / h).max(ty * glyph.height / h + 1));
            for tx in 0..w {
                let (sx0, sx1) = (tx * glyph.width / w, ((tx + 1) * glyph.width / w).max(tx * glyph.width / w + 1));
                let mut sum = [0u32; 4];
                let mut n = 0;
                for sy in sy0..sy1.min(glyph.height) {
                    for sx in sx0..sx1.min(glyph.width) {
                        let i = ((sy * glyph.width + sx) * 4) as usize;
                        for (k, s) in sum.iter_mut().enumerate() {
                            *s += glyph.data[i + k] as u32;
                        }
                        n += 1;
                    }
                }
                let o = ((ty * w + tx) * 4) as usize;
                for k in 0..4 {
                    data[o + k] = (sum[k] / n.max(1)) as u8;
                }
            }
        }
        Glyph {
            // Sit on the cell: from the top of the cell down `h` pixels.
            left: 0,
            top: self.baseline,
            width: w,
            height: h,
            data,
            color: true,
        }
    }
}

fn load_face(path: &str, index: u32) -> Option<FontRef<'static>> {
    let data = std::fs::read(path).ok()?;
    // Fonts live as long as the terminal.
    let data: &'static [u8] = Box::leak(data.into_boxed_slice());
    FontRef::from_index(data, index as usize)
}

/// File and face index fontconfig picks for a pattern (it knows the user's preferences).
fn fontconfig_match(pattern: &str) -> Option<(String, u32)> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}\n%{index}", pattern])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let path = lines.next()?.to_string();
    let index = lines.next().and_then(|i| i.parse().ok()).unwrap_or(0);
    (!path.is_empty()).then_some((path, index))
}

/// Without fontconfig: scan the system fonts.
fn fontdb_face(family: &str) -> Option<FontRef<'static>> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let families = [fontdb::Family::Name(family), fontdb::Family::Monospace];
    let id = db.query(&fontdb::Query { families: &families, ..Default::default() })?;
    let (data, index) = db.with_face_data(id, |data, index| (data.to_vec(), index))?;
    FontRef::from_index(Box::leak(data.into_boxed_slice()), index as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_a_monospace_font() {
        let mut font = Font::load("monospace", 16.0).expect("a monospace font on this system");
        assert!(font.cell_width > 4 && font.cell_height > font.cell_width);
        let a = font.glyph('a', Style::Regular).expect("glyph for `a`");
        assert!(a.width > 0 && a.data.len() == (a.width * a.height) as usize && !a.color);
        // Bold exists either as a face or emboldened, and differs from regular.
        let regular: Vec<u8> = font.glyph('a', Style::Regular).unwrap().data.clone();
        assert_ne!(font.glyph('a', Style::Bold).unwrap().data, regular);
    }

    #[test]
    fn falls_back_for_missing_characters() {
        let mut font = Font::load("monospace", 16.0).unwrap();
        // CJK is not in usual monospace fonts; a fallback (if installed) provides it.
        if fontconfig_match("monospace:charset=4e2d").is_some() {
            assert!(font.glyph('中', Style::Regular).is_some());
        }
        // An emoji is a color glyph fitted to at most two cells.
        let (cell_w, cell_h) = (font.cell_width, font.cell_height);
        if let Some(g) = font.glyph('😀', Style::Regular) {
            assert!(g.color && g.height <= cell_h && g.width <= 2 * cell_w);
        }
    }

    #[test]
    fn points() {
        assert_eq!(points_to_pixels(12.0), 16.0);
    }
}
