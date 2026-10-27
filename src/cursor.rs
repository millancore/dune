//! The pointer image dune draws on a real session: the user's xcursor theme
//! (`XCURSOR_THEME`, `XCURSOR_SIZE`), or a plain arrow when none is installed.

use std::{collections::HashMap, time::Duration};

use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    input::pointer::CursorIcon,
    utils::{Logical, Point, Transform},
};
use xcursor::{
    CursorTheme,
    parser::{Image, parse_xcursor},
};

type Frames = Vec<(Image, MemoryRenderBuffer)>;

pub struct Cursor {
    theme: CursorTheme,
    size: u32,
    /// Frames of each shape asked for so far, at the configured size.
    shapes: HashMap<CursorIcon, Frames>,
}

impl Cursor {
    pub fn load() -> Cursor {
        let name = std::env::var("XCURSOR_THEME").unwrap_or_else(|_| "default".into());
        let size = std::env::var("XCURSOR_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(24);
        let mut cursor = Cursor { theme: CursorTheme::load(&name), size, shapes: HashMap::new() };
        if load_icon(&cursor.theme, "default").is_none() {
            tracing::warn!("no xcursor theme `{name}` found, using a plain arrow");
        }
        cursor.frames(CursorIcon::Default);
        cursor
    }

    /// The image of `icon` to show at `time` (animated themes cycle) and its hotspot.
    pub fn image(&mut self, icon: CursorIcon, time: Duration) -> (MemoryRenderBuffer, Point<i32, Logical>) {
        let frames = self.frames(icon);
        let total: u32 = frames.iter().map(|(i, _)| i.delay).sum();
        let mut millis = if total == 0 { 0 } else { time.as_millis() as u32 % total };
        for (image, buffer) in frames {
            if millis < image.delay || total == 0 {
                return (buffer.clone(), (image.xhot as i32, image.yhot as i32).into());
            }
            millis -= image.delay;
        }
        let (image, buffer) = &frames[0];
        (buffer.clone(), (image.xhot as i32, image.yhot as i32).into())
    }

    /// The frames of `icon`, loaded the first time: its name, the older names themes use
    /// for it, else the default arrow (else a plain one).
    fn frames(&mut self, icon: CursorIcon) -> &Frames {
        let (theme, size) = (&self.theme, self.size);
        self.shapes.entry(icon).or_insert_with(|| {
            let images = std::iter::once(icon.name())
                .chain(icon.alt_names().iter().copied())
                .chain(std::iter::once("default"))
                .find_map(|name| load_icon(theme, name))
                .unwrap_or_else(|| vec![fallback_arrow()]);
            nearest_frames(images, size)
        })
    }
}

/// The frames of `images` closest to `size`, ready to draw.
fn nearest_frames(images: Vec<Image>, size: u32) -> Frames {
    let nearest = images.iter().min_by_key(|i| (i.size as i32 - size as i32).abs()).map(|i| i.size);
    images
        .into_iter()
        .filter(|i| Some(i.size) == nearest)
        .map(|image| {
            let buffer = MemoryRenderBuffer::from_slice(
                &image.pixels_rgba,
                Fourcc::Abgr8888,
                (image.width as i32, image.height as i32),
                1,
                Transform::Normal,
                None,
            );
            (image, buffer)
        })
        .collect()
}

fn load_icon(theme: &CursorTheme, name: &str) -> Option<Vec<Image>> {
    let path = theme.load_icon(name)?;
    let data = std::fs::read(path).ok()?;
    parse_xcursor(&data).filter(|images| !images.is_empty())
}

/// A 16×24 white arrow with a black outline.
fn fallback_arrow() -> Image {
    const ROWS: [&str; 24] = [
        "X...............",
        "XX..............",
        "XWX.............",
        "XWWX............",
        "XWWWX...........",
        "XWWWWX..........",
        "XWWWWWX.........",
        "XWWWWWWX........",
        "XWWWWWWWX.......",
        "XWWWWWWWWX......",
        "XWWWWWWWWWX.....",
        "XWWWWWWWWWWX....",
        "XWWWWWWXXXXXX...",
        "XWWWXWWX........",
        "XWWX.XWWX.......",
        "XWX..XWWX.......",
        "XX....XWWX......",
        "X.....XWWX......",
        ".......XWWX.....",
        ".......XWWX.....",
        "........XX......",
        "................",
        "................",
        "................",
    ];
    let pixels_rgba = ROWS
        .iter()
        .flat_map(|row| row.chars())
        .flat_map(|c| match c {
            'X' => [0, 0, 0, 255],
            'W' => [255, 255, 255, 255],
            _ => [0, 0, 0, 0],
        })
        .collect();
    Image { size: 24, width: 16, height: 24, xhot: 0, yhot: 0, delay: 0, pixels_rgba, pixels_argb: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_arrow_is_well_formed() {
        let arrow = fallback_arrow();
        assert_eq!(arrow.pixels_rgba.len(), (arrow.width * arrow.height * 4) as usize);
    }

    #[test]
    fn loads_something() {
        let mut cursor = Cursor::load();
        let (_, hotspot) = cursor.image(CursorIcon::Default, Duration::from_millis(1234));
        assert!(hotspot.x >= 0 && hotspot.y >= 0);
    }

    #[test]
    fn every_shape_has_frames() {
        // Whatever the installed theme has, a named shape always draws something.
        let mut cursor = Cursor::load();
        for icon in [CursorIcon::Pointer, CursorIcon::Text, CursorIcon::EwResize, CursorIcon::Wait] {
            assert!(!cursor.frames(icon).is_empty());
            cursor.image(icon, Duration::ZERO);
        }
    }
}
