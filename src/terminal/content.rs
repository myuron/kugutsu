//! alacritty の `Term` から描画用のスナップショットを作る。

use alacritty_terminal::Term;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};

use super::colors;

/// 同じスタイルが続く文字列の区間。`len` は UTF-8 のバイト長。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub len: usize,
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// 画面の 1 行分。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    pub text: String,
    pub runs: Vec<Run>,
}

/// カーソル位置（画面上の行・列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    pub row: usize,
    pub col: usize,
}

/// 描画に必要なターミナルの状態。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub rows: Vec<Row>,
    /// 非表示（DECTCEM off）の場合は `None`。
    pub cursor: Option<CursorPosition>,
    /// DECCKM（アプリケーションカーソルキーモード）が有効か。
    pub app_cursor: bool,
}

pub fn snapshot<T: EventListener>(term: &Term<T>) -> Snapshot {
    let content = term.renderable_content();
    let display_offset = content.display_offset as i32;
    let mut rows = vec![Row::default(); term.screen_lines()];

    for indexed in content.display_iter {
        let cell = indexed.cell;
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let Some(row) = usize::try_from(indexed.point.line.0 + display_offset)
            .ok()
            .and_then(|line| rows.get_mut(line))
        else {
            continue;
        };

        let start = row.text.len();
        row.text.push(cell.c);
        if let Some(zerowidth) = cell.zerowidth() {
            row.text.extend(zerowidth);
        }
        push_run(&mut row.runs, style_of(cell), row.text.len() - start);
    }

    let cursor = content
        .mode
        .contains(TermMode::SHOW_CURSOR)
        .then(|| {
            let point = content.cursor.point;
            usize::try_from(point.line.0 + display_offset)
                .ok()
                .map(|row| CursorPosition {
                    row,
                    col: point.column.0,
                })
        })
        .flatten();

    Snapshot {
        rows,
        cursor,
        app_cursor: content.mode.contains(TermMode::APP_CURSOR),
    }
}

fn style_of(cell: &Cell) -> Run {
    let flags = cell.flags;
    let (mut fg, mut bg) = (colors::to_rgb(cell.fg), colors::to_rgb(cell.bg));
    if flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if flags.contains(Flags::HIDDEN) {
        fg = bg;
    }
    Run {
        len: 0,
        fg,
        bg,
        bold: flags.contains(Flags::BOLD),
        italic: flags.contains(Flags::ITALIC),
        underline: flags.intersects(Flags::ALL_UNDERLINES),
    }
}

/// 直前の区間と同じスタイルなら伸ばし、違えば新しい区間を追加する。
fn push_run(runs: &mut Vec<Run>, style: Run, len: usize) {
    match runs.last_mut() {
        Some(last) if Run { len: 0, ..*last } == style => last.len += len,
        _ => runs.push(Run { len, ..style }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::colors::{ANSI_16, DEFAULT_BACKGROUND, DEFAULT_FOREGROUND};
    use crate::terminal::size::GridSize;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    fn snapshot_of(cols: u16, rows: u16, bytes: &[u8]) -> Snapshot {
        let size = GridSize {
            cols,
            rows,
            cell_width: 1.0,
            cell_height: 1.0,
        };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: Processor<StdSyncHandler> = Processor::new();
        parser.advance(&mut term, bytes);
        snapshot(&term)
    }

    fn plain(len: usize) -> Run {
        Run {
            len,
            fg: DEFAULT_FOREGROUND,
            bg: DEFAULT_BACKGROUND,
            bold: false,
            italic: false,
            underline: false,
        }
    }

    #[test]
    fn has_one_row_per_screen_line_padded_with_spaces() {
        let snap = snapshot_of(5, 3, b"hi");
        assert_eq!(snap.rows.len(), 3);
        assert_eq!(snap.rows[0].text, "hi   ");
        assert_eq!(snap.rows[1].text, "     ");
        assert_eq!(snap.rows[0].runs, vec![plain(5)]);
    }

    #[test]
    fn newline_moves_to_next_row() {
        let snap = snapshot_of(4, 2, b"a\r\nb");
        assert_eq!(snap.rows[0].text, "a   ");
        assert_eq!(snap.rows[1].text, "b   ");
    }

    #[test]
    fn sgr_colors_split_runs() {
        let snap = snapshot_of(6, 1, b"\x1b[31mred\x1b[0mok");
        assert_eq!(snap.rows[0].text, "redok ");
        assert_eq!(
            snap.rows[0].runs,
            vec![
                Run {
                    fg: ANSI_16[1],
                    ..plain(3)
                },
                plain(3),
            ]
        );
    }

    #[test]
    fn bold_italic_underline() {
        let snap = snapshot_of(3, 1, b"\x1b[1ma\x1b[0;3mb\x1b[0;4mc");
        let runs = &snap.rows[0].runs;
        assert!(runs[0].bold && !runs[0].italic);
        assert!(runs[1].italic && !runs[1].bold);
        assert!(runs[2].underline);
    }

    #[test]
    fn inverse_swaps_colors() {
        let snap = snapshot_of(2, 1, b"\x1b[7mx\x1b[0m");
        assert_eq!(
            snap.rows[0].runs[0],
            Run {
                fg: DEFAULT_BACKGROUND,
                bg: DEFAULT_FOREGROUND,
                ..plain(1)
            }
        );
    }

    #[test]
    fn wide_characters_skip_spacer_cells() {
        let snap = snapshot_of(4, 1, "あb".as_bytes());
        assert_eq!(snap.rows[0].text, "あb ");
        assert_eq!(snap.rows[0].runs, vec![plain("あb ".len())]);
    }

    #[test]
    fn cursor_position_follows_output() {
        let snap = snapshot_of(5, 3, b"ab\r\nc");
        assert_eq!(snap.cursor, Some(CursorPosition { row: 1, col: 1 }));
    }

    #[test]
    fn hidden_cursor() {
        let snap = snapshot_of(5, 3, b"\x1b[?25l");
        assert_eq!(snap.cursor, None);
    }

    #[test]
    fn clear_screen() {
        let snap = snapshot_of(3, 1, b"abc\x1b[2J");
        assert_eq!(snap.rows[0].text, "   ");
    }

    #[test]
    fn app_cursor_mode() {
        assert!(!snapshot_of(3, 1, b"").app_cursor);
        assert!(snapshot_of(3, 1, b"\x1b[?1h").app_cursor);
    }
}
