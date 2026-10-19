//! The pointer image dune draws on a real session: the user's xcursor theme
//! (`XCURSOR_THEME`, `XCURSOR_SIZE`), or a plain arrow when none is installed.

use std::time::Duration;

use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::{Logical, Point, Transform},
};
use xcursor::{
    CursorTheme,
    parser::{Image, parse_xcursor},
};

pub struct Cursor {
    /// Frames of the theme's default cursor at the configured size.
    frames: Vec<(Image, MemoryRenderBuffer)>,
}

impl Cursor {
    pub fn load() -> Cursor {
        let theme = std::env::var("XCURSOR_THEME").unwrap_or_else(|_| "default".into());
        let size = std::env::var("XCURSOR_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(24);
        let images = load_theme(&theme).unwrap_or_else(|| {
            tracing::warn!("no xcursor theme `{theme}` found, using a plain arrow");
            vec![fallback_arrow()]
        });
        // The frames closest to the wanted size.
        let nearest = images.iter().min_by_key(|i| (i.size as i32 - size as i32).abs()).map(|i| i.size);
        let frames = images
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
            .collect();
        Cursor { frames }
    }

    /// The image to show at `time` (animated themes cycle) and its hotspot.
    pub fn image(&self, time: Duration) -> (MemoryRenderBuffer, Point<i32, Logical>) {
        let total: u32 = self.frames.iter().map(|(i, _)| i.delay).sum();
        let mut millis = if total == 0 { 0 } else { time.as_millis() as u32 % total };
        for (image, buffer) in &self.frames {
            if millis < image.delay || total == 0 {
                return (buffer.clone(), (image.xhot as i32, image.yhot as i32).into());
            }
            millis -= image.delay;
        }
        let (image, buffer) = &self.frames[0];
        (buffer.clone(), (image.xhot as i32, image.yhot as i32).into())
    }
}

fn load_theme(name: &str) -> Option<Vec<Image>> {
    let path = CursorTheme::load(name).load_icon("default")?;
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
        let cursor = Cursor::load();
        assert!(!cursor.frames.is_empty());
        let (_, hotspot) = cursor.image(Duration::from_millis(1234));
        assert!(hotspot.x >= 0 && hotspot.y >= 0);
    }
}
