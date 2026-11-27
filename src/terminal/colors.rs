//! ANSI カラーを 0xRRGGBB に変換する。

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

use crate::theme::TerminalColors;

/// alacritty の `Color` を 0xRRGGBB に変換する。
///
/// `palette` はアプリが OSC 4/10/11/12 で上書きした色（`Term::colors()`）で、設定されていれば
/// テーマの配色 `theme` より優先する。
pub fn to_rgb(color: Color, palette: &Colors, theme: &TerminalColors) -> u32 {
    match color {
        Color::Named(named) => palette[named].map_or_else(|| named_to_rgb(named, theme), pack_rgb),
        Color::Indexed(index) => {
            palette[index as usize].map_or_else(|| indexed_to_rgb(index, theme), pack_rgb)
        }
        Color::Spec(rgb) => pack_rgb(rgb),
    }
}

/// OSC 4/10/11/12 の問い合わせで使われる色番号（0-255: パレット, 256: 前景, 257: 背景, 258: カーソル）を RGB に変換する。
pub fn index_to_rgb(index: usize, palette: &Colors, theme: &TerminalColors) -> Option<Rgb> {
    let packed = match index {
        0..=255 => indexed_to_rgb(index as u8, theme),
        256 => theme.foreground,
        257 => theme.background,
        258 => theme.cursor,
        _ => return None,
    };
    Some(palette[index].unwrap_or_else(|| unpack(packed)))
}

/// SGR 2 (DIM) 用に各チャンネルを 2/3 に暗くする。
pub fn dim(rgb: u32) -> u32 {
    let scale = |shift: u32| ((rgb >> shift & 0xff) * 2 / 3) << shift;
    scale(16) | scale(8) | scale(0)
}

fn named_to_rgb(named: NamedColor, theme: &TerminalColors) -> u32 {
    match named {
        NamedColor::Foreground | NamedColor::BrightForeground => theme.foreground,
        NamedColor::DimForeground => dim(theme.foreground),
        NamedColor::Background => theme.background,
        NamedColor::Cursor => theme.cursor,
        NamedColor::DimBlack
        | NamedColor::DimRed
        | NamedColor::DimGreen
        | NamedColor::DimYellow
        | NamedColor::DimBlue
        | NamedColor::DimMagenta
        | NamedColor::DimCyan
        | NamedColor::DimWhite => theme.ansi[named as usize - NamedColor::DimBlack as usize],
        // 残りは Black(0) 〜 BrightWhite(15)
        _ => theme.ansi[named as usize],
    }
}

/// xterm 256 色: 0-15 は ANSI 16 色、16-231 は 6x6x6 カラーキューブ、232-255 はグレースケール。
fn indexed_to_rgb(index: u8, theme: &TerminalColors) -> u32 {
    const CUBE_LEVELS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    match index {
        0..=15 => theme.ansi[index as usize],
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
    use crate::theme::{self, Theme};
    use alacritty_terminal::term::color::Colors;

    fn dark() -> TerminalColors {
        Theme::default().terminal
    }

    fn light() -> TerminalColors {
        theme::builtin("kugutsu-light").unwrap().terminal.clone()
    }

    fn rgb_of(color: Color) -> u32 {
        to_rgb(color, &Colors::default(), &dark())
    }

    #[test]
    fn palette_overrides_take_precedence() {
        let theme = dark();
        let mut palette = Colors::default();
        palette[1] = Some(unpack(0x010203));
        palette[NamedColor::Background] = Some(unpack(0xffffff));
        assert_eq!(
            to_rgb(Color::Named(NamedColor::Red), &palette, &theme),
            0x010203
        );
        assert_eq!(to_rgb(Color::Indexed(1), &palette, &theme), 0x010203);
        assert_eq!(
            to_rgb(Color::Named(NamedColor::Background), &palette, &theme),
            0xffffff
        );
        assert_eq!(index_to_rgb(257, &palette, &theme), Some(unpack(0xffffff)));
        assert_eq!(
            index_to_rgb(2, &palette, &theme),
            Some(unpack(theme.ansi[2]))
        );
    }

    #[test]
    fn dim_darkens_each_channel() {
        assert_eq!(dim(0xccccff), 0x8888aa);
        assert_eq!(dim(0x000000), 0x000000);
    }

    #[test]
    fn color_request_indices() {
        let theme = dark();
        let request = |index| index_to_rgb(index, &Colors::default(), &theme);
        assert_eq!(request(1), Some(unpack(theme.ansi[1])));
        assert_eq!(request(196), Some(unpack(0xff0000)));
        assert_eq!(request(256), Some(unpack(theme.foreground)));
        assert_eq!(request(257), Some(unpack(theme.background)));
        assert_eq!(request(258), Some(unpack(theme.cursor)));
        assert_eq!(request(300), None);
    }

    #[test]
    fn color_requests_follow_the_theme() {
        let theme = light();
        let request = |index| index_to_rgb(index, &Colors::default(), &theme);
        assert_eq!(request(257), Some(unpack(0xffffff)));
        assert_eq!(request(256), Some(unpack(0x333333)));
        assert_eq!(request(4), Some(unpack(0x0451a5)));
    }

    #[test]
    fn named_ansi_colors_use_palette() {
        let ansi = dark().ansi;
        assert_eq!(rgb_of(Color::Named(NamedColor::Black)), ansi[0]);
        assert_eq!(rgb_of(Color::Named(NamedColor::Red)), ansi[1]);
        assert_eq!(rgb_of(Color::Named(NamedColor::White)), ansi[7]);
        assert_eq!(rgb_of(Color::Named(NamedColor::BrightRed)), ansi[9]);
        assert_eq!(rgb_of(Color::Named(NamedColor::BrightWhite)), ansi[15]);
    }

    #[test]
    fn named_colors_follow_the_theme() {
        let theme = light();
        let rgb = |named| to_rgb(Color::Named(named), &Colors::default(), &theme);
        assert_eq!(rgb(NamedColor::Foreground), 0x333333);
        assert_eq!(rgb(NamedColor::Background), 0xffffff);
        assert_eq!(rgb(NamedColor::Cursor), 0x5a5a5a);
        assert_eq!(rgb(NamedColor::Green), 0x00bc00);
        assert_eq!(
            to_rgb(Color::Indexed(10), &Colors::default(), &theme),
            0x14ce14
        );
    }

    #[test]
    fn dim_colors_map_to_normal_colors() {
        assert_eq!(rgb_of(Color::Named(NamedColor::DimGreen)), dark().ansi[2]);
    }

    #[test]
    fn default_colors() {
        let theme = dark();
        assert_eq!(
            rgb_of(Color::Named(NamedColor::Foreground)),
            theme.foreground
        );
        assert_eq!(
            rgb_of(Color::Named(NamedColor::Background)),
            theme.background
        );
        assert_eq!(rgb_of(Color::Named(NamedColor::Cursor)), theme.cursor);
    }

    #[test]
    fn indexed_low_colors_use_palette() {
        assert_eq!(rgb_of(Color::Indexed(1)), dark().ansi[1]);
        assert_eq!(rgb_of(Color::Indexed(15)), dark().ansi[15]);
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
