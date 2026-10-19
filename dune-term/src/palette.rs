//! Terminal colors: the default palette and resolving a cell's colors.

use alacritty_terminal::{
    term::{cell::Flags, color::Colors},
    vte::ansi::{Color, NamedColor, Rgb},
};

const FOREGROUND: Rgb = Rgb { r: 0xd8, g: 0xde, b: 0xe9 };
const BACKGROUND: Rgb = Rgb { r: 0x14, g: 0x14, b: 0x1a };

/// The 16 base colors: black, red, green, yellow, blue, magenta, cyan, white, then bright.
const BASE: [Rgb; 16] = [
    Rgb { r: 0x2e, g: 0x34, b: 0x40 },
    Rgb { r: 0xe0, g: 0x6c, b: 0x75 },
    Rgb { r: 0x98, g: 0xc3, b: 0x79 },
    Rgb { r: 0xe5, g: 0xc0, b: 0x7b },
    Rgb { r: 0x61, g: 0xaf, b: 0xef },
    Rgb { r: 0xc6, g: 0x78, b: 0xdd },
    Rgb { r: 0x56, g: 0xb6, b: 0xc2 },
    Rgb { r: 0xd8, g: 0xde, b: 0xe9 },
    Rgb { r: 0x5c, g: 0x63, b: 0x70 },
    Rgb { r: 0xff, g: 0x7b, b: 0x86 },
    Rgb { r: 0xb5, g: 0xe8, b: 0x90 },
    Rgb { r: 0xff, g: 0xd8, b: 0x8a },
    Rgb { r: 0x7d, g: 0xc4, b: 0xff },
    Rgb { r: 0xde, g: 0x92, b: 0xf5 },
    Rgb { r: 0x6f, g: 0xd3, b: 0xe0 },
    Rgb { r: 0xff, g: 0xff, b: 0xff },
];

pub fn background(colors: &Colors) -> Rgb {
    resolve(Color::Named(NamedColor::Background), colors)
}

pub fn foreground(colors: &Colors) -> Rgb {
    resolve(Color::Named(NamedColor::Foreground), colors)
}

/// A color as set by the application (OSC overrides in `colors`), else our palette.
pub fn resolve(color: Color, colors: &Colors) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,
        Color::Named(name) => colors[name].unwrap_or_else(|| default_named(name)),
        Color::Indexed(i) => colors[i as usize].unwrap_or_else(|| default_indexed(i)),
    }
}

/// Foreground and background of a cell, after bold-brightening, dim and inverse.
pub fn cell_colors(fg: Color, bg: Color, flags: Flags, colors: &Colors) -> (Rgb, Rgb) {
    let fg = match fg {
        // Bold text in one of the 8 base colors uses the bright variant.
        Color::Named(name) if flags.contains(Flags::BOLD) && (name as usize) < 8 => {
            Color::Indexed(name as u8 + 8)
        }
        other => other,
    };
    let mut fg = resolve(fg, colors);
    let mut bg = resolve(bg, colors);
    if flags.contains(Flags::DIM) {
        fg = scale(fg, 0.66);
    }
    if flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    (fg, bg)
}

fn scale(c: Rgb, f: f32) -> Rgb {
    let s = |v: u8| (v as f32 * f).round() as u8;
    Rgb { r: s(c.r), g: s(c.g), b: s(c.b) }
}

fn default_named(name: NamedColor) -> Rgb {
    let i = name as usize;
    match name {
        NamedColor::Foreground | NamedColor::Cursor | NamedColor::BrightForeground => FOREGROUND,
        NamedColor::Background => BACKGROUND,
        NamedColor::DimForeground => scale(FOREGROUND, 0.66),
        _ if i < 16 => BASE[i],
        // DimBlack..DimWhite follow Cursor.
        _ => scale(BASE[(i - NamedColor::DimBlack as usize) % 8], 0.66),
    }
}

/// xterm 256 colors: 16 base, a 6×6×6 cube, then 24 grays.
fn default_indexed(i: u8) -> Rgb {
    match i {
        0..=15 => BASE[i as usize],
        16..=231 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Rgb { r: level(i / 36), g: level((i / 6) % 6), b: level(i % 6) }
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            Rgb { r: v, g: v, b: v }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm_256_palette() {
        assert_eq!(default_indexed(16), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(default_indexed(196), Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(default_indexed(231), Rgb { r: 255, g: 255, b: 255 });
        assert_eq!(default_indexed(232), Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(default_indexed(255), Rgb { r: 238, g: 238, b: 238 });
    }

    #[test]
    fn cell_attributes() {
        let colors = Colors::default();
        let red = Color::Named(NamedColor::Red);
        let bg = Color::Named(NamedColor::Background);
        assert_eq!(cell_colors(red, bg, Flags::empty(), &colors), (BASE[1], BACKGROUND));
        assert_eq!(cell_colors(red, bg, Flags::BOLD, &colors), (BASE[9], BACKGROUND));
        assert_eq!(cell_colors(red, bg, Flags::INVERSE, &colors), (BACKGROUND, BASE[1]));
        let rgb = Rgb { r: 1, g: 2, b: 3 };
        assert_eq!(cell_colors(Color::Spec(rgb), bg, Flags::empty(), &colors).0, rgb);
    }

    #[test]
    fn application_overrides_win() {
        let mut colors = Colors::default();
        let custom = Rgb { r: 9, g: 9, b: 9 };
        colors[NamedColor::Background] = Some(custom);
        assert_eq!(background(&colors), custom);
        assert_eq!(foreground(&colors), FOREGROUND);
    }
}
