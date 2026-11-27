//! マウスホイールの入力を、スクロール行数や PTY へ送るバイト列に変換する。

use alacritty_terminal::term::TermMode;
use gpui::{Modifiers, Pixels, ScrollDelta};

/// ホイールの移動量を行数に換算する。タッチパッドのように 1 行に満たない移動が続く場合に備え、
/// 端数を持ち越す。正の値は上（スクロールバックの過去側）。
#[derive(Debug, Default)]
pub struct WheelAccumulator {
    remainder: f32,
}

impl WheelAccumulator {
    pub fn lines(&mut self, delta: ScrollDelta, cell_height: Pixels) -> i32 {
        let lines = match delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / f32::from(cell_height),
        };
        if !lines.is_finite() {
            return 0;
        }
        let total = self.remainder + lines;
        let whole = total.trunc();
        self.remainder = total - whole;
        whole as i32
    }
}

/// ホイールの向き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wheel {
    Up,
    Down,
}

/// マウスレポート（xterm）でホイール 1 回分を送るバイト列。`col` / `row` は 0 始まりのセル位置。
/// `sgr` は SGR 形式（DECSET 1006）が有効か。
pub fn mouse_report(
    wheel: Wheel,
    col: usize,
    row: usize,
    modifiers: &Modifiers,
    sgr: bool,
) -> Vec<u8> {
    let button = match wheel {
        Wheel::Up => 64,
        Wheel::Down => 65,
    } + 4 * u8::from(modifiers.shift)
        + 8 * u8::from(modifiers.alt)
        + 16 * u8::from(modifiers.control);
    if sgr {
        return format!("\x1b[<{button};{};{}M", col + 1, row + 1).into_bytes();
    }
    // 通常形式は値に 32 を足した 1 バイトで送る
    let encode = |position: usize| (position + 1 + 32).min(255) as u8;
    vec![0x1b, b'[', b'M', 32 + button, encode(col), encode(row)]
}

/// ホイールをどう扱うか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelMode {
    /// アプリケーションがマウスレポートを要求しているので、ホイールをマウスイベントとして送る。
    Report { sgr: bool },
    /// alternate screen（less や vim）でスクロールバックが無いので、矢印キーとして送る。
    Arrows { app_cursor: bool },
    /// ターミナル自身のスクロールバックを動かす。
    Scrollback,
}

pub fn wheel_mode(mode: TermMode) -> WheelMode {
    if mode.intersects(TermMode::MOUSE_MODE) {
        WheelMode::Report {
            sgr: mode.contains(TermMode::SGR_MOUSE),
        }
    } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
        WheelMode::Arrows {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
        }
    } else {
        WheelMode::Scrollback
    }
}

/// alternate screen でホイールを矢印キーとして送るバイト列（`lines` が正なら上）。
pub fn alternate_scroll(lines: i32, app_cursor: bool) -> Vec<u8> {
    let letter = if lines > 0 { b'A' } else { b'B' };
    let introducer = if app_cursor { b'O' } else { b'[' };
    [0x1b, introducer, letter].repeat(lines.unsigned_abs() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px};

    const CELL: Pixels = px(20.0);

    #[test]
    fn line_deltas_are_used_as_is() {
        let mut wheel = WheelAccumulator::default();
        assert_eq!(wheel.lines(ScrollDelta::Lines(point(0.0, 3.0)), CELL), 3);
        assert_eq!(wheel.lines(ScrollDelta::Lines(point(0.0, -3.0)), CELL), -3);
    }

    #[test]
    fn pixel_deltas_are_divided_by_cell_height() {
        let mut wheel = WheelAccumulator::default();
        assert_eq!(
            wheel.lines(ScrollDelta::Pixels(point(px(0.0), px(45.0))), CELL),
            2
        );
    }

    #[test]
    fn fractions_carry_over() {
        let mut wheel = WheelAccumulator::default();
        let half = ScrollDelta::Pixels(point(px(0.0), px(10.0)));
        assert_eq!(wheel.lines(half, CELL), 0);
        assert_eq!(wheel.lines(half, CELL), 1);
        let back = ScrollDelta::Pixels(point(px(0.0), px(-30.0)));
        assert_eq!(wheel.lines(back, CELL), -1);
        assert_eq!(wheel.lines(back, CELL), -2);
    }

    #[test]
    fn sgr_mouse_report() {
        let none = Modifiers::default();
        assert_eq!(mouse_report(Wheel::Up, 2, 4, &none, true), b"\x1b[<64;3;5M");
        assert_eq!(
            mouse_report(Wheel::Down, 0, 0, &none, true),
            b"\x1b[<65;1;1M"
        );
    }

    #[test]
    fn mouse_report_includes_modifiers() {
        let modifiers = Modifiers {
            shift: true,
            alt: true,
            control: true,
            ..Modifiers::default()
        };
        // Shift(4) + Alt(8) + Ctrl(16)
        assert_eq!(
            mouse_report(Wheel::Up, 0, 0, &modifiers, true),
            b"\x1b[<92;1;1M"
        );
    }

    #[test]
    fn normal_mouse_report() {
        let none = Modifiers::default();
        assert_eq!(
            mouse_report(Wheel::Up, 2, 4, &none, false),
            [0x1b, b'[', b'M', 32 + 64, 32 + 3, 32 + 5]
        );
        assert_eq!(
            mouse_report(Wheel::Down, 0, 0, &none, false),
            [0x1b, b'[', b'M', 32 + 65, 32 + 1, 32 + 1]
        );
    }

    #[test]
    fn normal_mouse_report_clamps_far_positions() {
        // 通常形式は 1 バイトで座標を表すので 223 列（行）までしか送れない
        let report = mouse_report(Wheel::Up, 500, 300, &Modifiers::default(), false);
        assert_eq!(report, [0x1b, b'[', b'M', 32 + 64, 255, 255]);
    }

    #[test]
    fn wheel_scrolls_back_by_default() {
        assert_eq!(wheel_mode(TermMode::default()), WheelMode::Scrollback);
    }

    #[test]
    fn wheel_is_reported_when_mouse_mode_is_on() {
        for mouse in [
            TermMode::MOUSE_REPORT_CLICK,
            TermMode::MOUSE_DRAG,
            TermMode::MOUSE_MOTION,
        ] {
            assert_eq!(wheel_mode(mouse), WheelMode::Report { sgr: false });
            assert_eq!(
                wheel_mode(mouse | TermMode::SGR_MOUSE | TermMode::ALT_SCREEN),
                WheelMode::Report { sgr: true }
            );
        }
    }

    #[test]
    fn wheel_becomes_arrows_on_alternate_screen() {
        let alt = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(wheel_mode(alt), WheelMode::Arrows { app_cursor: false });
        assert_eq!(
            wheel_mode(alt | TermMode::APP_CURSOR),
            WheelMode::Arrows { app_cursor: true }
        );
        // DECSET 1007 を切っていれば送らない（alternate screen にはスクロールバックが無いので何も起きない）
        assert_eq!(wheel_mode(TermMode::ALT_SCREEN), WheelMode::Scrollback);
    }

    #[test]
    fn alternate_scroll_sends_arrow_keys() {
        assert_eq!(alternate_scroll(2, false), b"\x1b[A\x1b[A");
        assert_eq!(alternate_scroll(-1, false), b"\x1b[B");
        assert_eq!(alternate_scroll(1, true), b"\x1bOA");
        assert_eq!(alternate_scroll(-2, true), b"\x1bOB\x1bOB");
        assert_eq!(alternate_scroll(0, true), b"");
    }
}
