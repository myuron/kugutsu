//! ANSI カラーを 0xRRGGBB に変換する。

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

/// 既定の前景色。
pub const DEFAULT_FOREGROUND: u32 = 0xd4d4d4;
/// 既定の背景色。
pub const DEFAULT_BACKGROUND: u32 = 0x1e1e1e;
/// カーソル色。
pub const CURSOR: u32 = 0xaeafad;

/// ANSI 16 色パレット（0-7: 通常, 8-15: 明るい色）。
pub const ANSI_16: [u32; 16] = [
    0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666,
    0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xffffff,
];

/// alacritty の `Color` を 0xRRGGBB に変換する。
///
/// `palette` はアプリが OSC 4/10/11/12 で上書きした色（`Term::colors()`）で、設定されていれば優先する。
pub fn to_rgb(color: Color, palette: &Colors) -> u32 {
    match color {
        Color::Named(named) => palette[named].map_or_else(|| named_to_rgb(named), pack_rgb),
        Color::Indexed(index) => {
            palette[index as usize].map_or_else(|| indexed_to_rgb(index), pack_rgb)
        }
        Color::Spec(rgb) => pack_rgb(rgb),
    }
}

/// OSC 4/10/11/12 の問い合わせで使われる色番号（0-255: パレット, 256: 前景, 257: 背景, 258: カーソル）を RGB に変換する。
pub fn index_to_rgb(index: usize, palette: &Colors) -> Option<Rgb> {
    let packed = match index {
        0..=255 => indexed_to_rgb(index as u8),
        256 => DEFAULT_FOREGROUND,
        257 => DEFAULT_BACKGROUND,
        258 => CURSOR,
        _ => return None,
    };
    Some(palette[index].unwrap_or_else(|| unpack(packed)))
}

/// SGR 2 (DIM) 用に各チャンネルを 2/3 に暗くする。
pub fn dim(rgb: u32) -> u32 {
    let scale = |shift: u32| ((rgb >> shift & 0xff) * 2 / 3) << shift;
    scale(16) | scale(8) | scale(0)
}

fn named_to_rgb(named: NamedColor) -> u32 {
    match named {
        NamedColor::Foreground | NamedColor::BrightForeground => DEFAULT_FOREGROUND,
        NamedColor::DimForeground => dim(DEFAULT_FOREGROUND),
        NamedColor::Background => DEFAULT_BACKGROUND,
        NamedColor::Cursor => CURSOR,
        NamedColor::DimBlack
        | NamedColor::DimRed
        | NamedColor::DimGreen
        | NamedColor::DimYellow
        | NamedColor::DimBlue
        | NamedColor::DimMagenta
        | NamedColor::DimCyan
        | NamedColor::DimWhite => ANSI_16[named as usize - NamedColor::DimBlack as usize],
        // 残りは Black(0) 〜 BrightWhite(15)
        _ => ANSI_16[named as usize],
    }
}

/// xterm 256 色: 0-15 は ANSI 16 色、16-231 は 6x6x6 カラーキューブ、232-255 はグレースケール。
fn indexed_to_rgb(index: u8) -> u32 {
    const CUBE_LEVELS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    match index {
        0..=15 => ANSI_16[index as usize],
        16..=231 => {
            let i = index - 16;
            pack(
                CUBE_LEVELS[(i / 36) as usize],
                CUBE_LEVELS[(i / 6 % 6) as usize],
                CUBE_LEVELS[(i % 6) as usize],
            )
        }
        232..=255 => {
            let level = 8 + (index - 232) * 10;
            pack(level, level, level)
        }
    }
}

fn pack(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

fn pack_rgb(Rgb { r, g, b }: Rgb) -> u32 {
    pack(r, g, b)
}

fn unpack(packed: u32) -> Rgb {
    Rgb {
        r: (packed >> 16) as u8,
        g: (packed >> 8) as u8,
        b: packed as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::term::color::Colors;

    fn rgb_of(color: Color) -> u32 {
        to_rgb(color, &Colors::default())
    }

    #[test]
    fn palette_overrides_take_precedence() {
        let mut palette = Colors::default();
        palette[1] = Some(unpack(0x010203));
        palette[NamedColor::Background] = Some(unpack(0xffffff));
        assert_eq!(to_rgb(Color::Named(NamedColor::Red), &palette), 0x010203);
        assert_eq!(to_rgb(Color::Indexed(1), &palette), 0x010203);
        assert_eq!(
            to_rgb(Color::Named(NamedColor::Background), &palette),
            0xffffff
        );
        assert_eq!(index_to_rgb(257, &palette), Some(unpack(0xffffff)));
        assert_eq!(index_to_rgb(2, &palette), Some(unpack(ANSI_16[2])));
    }

    #[test]
    fn dim_darkens_each_channel() {
        assert_eq!(dim(0xccccff), 0x8888aa);
        assert_eq!(dim(0x000000), 0x000000);
    }

    #[test]
    fn color_request_indices() {
        assert_eq!(
            index_to_rgb(1, &Colors::default()),
            Some(unpack(ANSI_16[1]))
        );
        assert_eq!(
            index_to_rgb(196, &Colors::default()),
            Some(unpack(0xff0000))
        );
        assert_eq!(
            index_to_rgb(256, &Colors::default()),
            Some(unpack(DEFAULT_FOREGROUND))
        );
        assert_eq!(
            index_to_rgb(257, &Colors::default()),
            Some(unpack(DEFAULT_BACKGROUND))
        );
        assert_eq!(index_to_rgb(258, &Colors::default()), Some(unpack(CURSOR)));
        assert_eq!(index_to_rgb(300, &Colors::default()), None);
    }

    #[test]
    fn named_ansi_colors_use_palette() {
        assert_eq!(rgb_of(Color::Named(NamedColor::Black)), ANSI_16[0]);
        assert_eq!(rgb_of(Color::Named(NamedColor::Red)), ANSI_16[1]);
        assert_eq!(rgb_of(Color::Named(NamedColor::White)), ANSI_16[7]);
        assert_eq!(rgb_of(Color::Named(NamedColor::BrightRed)), ANSI_16[9]);
        assert_eq!(rgb_of(Color::Named(NamedColor::BrightWhite)), ANSI_16[15]);
    }

    #[test]
    fn dim_colors_map_to_normal_colors() {
        assert_eq!(rgb_of(Color::Named(NamedColor::DimGreen)), ANSI_16[2]);
    }

    #[test]
    fn default_colors() {
        assert_eq!(
            rgb_of(Color::Named(NamedColor::Foreground)),
            DEFAULT_FOREGROUND
        );
        assert_eq!(
            rgb_of(Color::Named(NamedColor::Background)),
            DEFAULT_BACKGROUND
        );
        assert_eq!(rgb_of(Color::Named(NamedColor::Cursor)), CURSOR);
    }

    #[test]
    fn indexed_low_colors_use_palette() {
        assert_eq!(rgb_of(Color::Indexed(1)), ANSI_16[1]);
        assert_eq!(rgb_of(Color::Indexed(15)), ANSI_16[15]);
    }

    #[test]
    fn indexed_color_cube() {
        assert_eq!(rgb_of(Color::Indexed(16)), 0x000000);
        assert_eq!(rgb_of(Color::Indexed(196)), 0xff0000);
        assert_eq!(rgb_of(Color::Indexed(67)), 0x5f87af);
        assert_eq!(rgb_of(Color::Indexed(231)), 0xffffff);
    }

    #[test]
    fn indexed_grayscale() {
        assert_eq!(rgb_of(Color::Indexed(232)), 0x080808);
        assert_eq!(rgb_of(Color::Indexed(255)), 0xeeeeee);
    }

    #[test]
    fn true_color() {
        assert_eq!(
            rgb_of(Color::Spec(Rgb {
                r: 0x12,
                g: 0x34,
                b: 0x56
            })),
            0x123456
        );
    }
}
