//! alacritty の `Term` から描画用のスナップショットを作る。
//!
//! グリフをセル格子に正確に揃えるため、1 行を「描画位置を固定できる区間（Segment）」に分割する。
//! - 半角セルが同じスタイルで続く区間は、1 文字 = 1 セルとして並べられる（`monospaced = true`）。
//! - 全角文字や結合文字を含むセルは単独の区間にし、そのセル位置に置く（`monospaced = false`）。

use alacritty_terminal::Term;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;

use super::colors;

/// セルの描画スタイル（色は 0xRRGGBB）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub fg: u32,
    pub bg: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fg: colors::DEFAULT_FOREGROUND,
            bg: colors::DEFAULT_BACKGROUND,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }
}

/// 行内で、開始列から連続して描画される同一スタイルの区間。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// 開始列。
    pub col: usize,
    /// 占有するセル数。
    pub cells: usize,
    pub text: String,
    pub style: Style,
    /// true なら `text` の 1 文字が 1 セルに対応する（グリフ送り幅をセル幅に固定して描画できる）。
    pub monospaced: bool,
}

/// 画面の 1 行分。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    pub segments: Vec<Segment>,
}

#[cfg(test)]
impl Row {
    /// 行全体のテキスト（全角文字の後半セルは含まない）。
    pub fn text(&self) -> String {
        self.segments.iter().map(|s| s.text.as_str()).collect()
    }
}

/// カーソル位置（画面上の行・列）と幅（全角文字上なら 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub cells: usize,
}

/// 描画に必要なターミナルの状態。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub rows: Vec<Row>,
    /// 非表示（DECTCEM off）または画面外の場合は `None`。
    pub cursor: Option<Cursor>,
}

pub fn snapshot<T: EventListener>(term: &Term<T>) -> Snapshot {
    let content = term.renderable_content();
    let display_offset = content.display_offset as i32;
    let palette = content.colors;
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
        push_cell(
            &mut row.segments,
            indexed.point.column.0,
            cell,
            style_of(cell, palette),
        );
    }

    let cursor = if content.mode.contains(TermMode::SHOW_CURSOR) {
        cursor_position(term)
    } else {
        None
    };

    Snapshot { rows, cursor }
}

/// 表示・非表示（DECTCEM）に関わらないカーソル位置。画面外（スクロールバック表示中）なら `None`。
pub fn cursor_position<T: EventListener>(term: &Term<T>) -> Option<Cursor> {
    let mut point = term.grid().cursor.point;
    // 全角文字の後半セルにいる場合はその全角文字の位置とする（alacritty の描画と同じ）
    if term.grid()[point].flags.contains(Flags::WIDE_CHAR_SPACER) {
        point.column -= 1;
    }
    let display_offset = term.grid().display_offset() as i32;
    usize::try_from(point.line.0 + display_offset)
        .ok()
        .filter(|&row| row < term.screen_lines())
        .map(|row| Cursor {
            row,
            col: point.column.0,
            cells: if term.grid()[point].flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            },
        })
}

/// スクロール位置に関わらない、最下部の画面の各行の文字列（行末の空白は除く）。
pub fn bottom_screen_lines<T: EventListener>(term: &Term<T>) -> Vec<String> {
    let grid = term.grid();
    (0..term.screen_lines())
        .map(|line| {
            let mut text = String::new();
            for cell in grid[Line(line as i32)].into_iter() {
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                text.push(cell.c);
                text.extend(cell.zerowidth().into_iter().flatten());
            }
            text.trim_end().to_string()
        })
        .collect()
}

/// セルを行の区間に追加する。半角セルは直前の同一スタイル区間に連結する。
fn push_cell(segments: &mut Vec<Segment>, col: usize, cell: &Cell, style: Style) {
    let wide = cell.flags.contains(Flags::WIDE_CHAR);
    let zerowidth = cell.zerowidth();

    if wide || zerowidth.is_some() {
        let mut text = String::from(cell.c);
        text.extend(zerowidth.into_iter().flatten());
        segments.push(Segment {
            col,
            cells: if wide { 2 } else { 1 },
            text,
            style,
            monospaced: false,
        });
        return;
    }

    match segments.last_mut() {
        Some(last) if last.monospaced && last.style == style && last.col + last.cells == col => {
            last.text.push(cell.c);
            last.cells += 1;
        }
        _ => segments.push(Segment {
            col,
            cells: 1,
            text: String::from(cell.c),
            style,
            monospaced: true,
        }),
    }
}

fn style_of(cell: &Cell, palette: &Colors) -> Style {
    let flags = cell.flags;
    let mut fg = colors::to_rgb(cell.fg, palette);
    let mut bg = colors::to_rgb(cell.bg, palette);
    if flags.contains(Flags::DIM) {
        fg = colors::dim(fg);
    }
    if flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if flags.contains(Flags::HIDDEN) {
        fg = bg;
    }
    Style {
        fg,
        bg,
        bold: flags.contains(Flags::BOLD),
        italic: flags.contains(Flags::ITALIC),
        underline: flags.intersects(Flags::ALL_UNDERLINES),
        strikethrough: flags.contains(Flags::STRIKEOUT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::colors::{ANSI_16, DEFAULT_BACKGROUND, DEFAULT_FOREGROUND, dim};
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

    fn seg(col: usize, text: &str, style: Style) -> Segment {
        Segment {
            col,
            cells: text.chars().count(),
            text: text.into(),
            style,
            monospaced: true,
        }
    }

    fn fg(fg: u32) -> Style {
        Style {
            fg,
            ..Style::default()
        }
    }

    #[test]
    fn has_one_row_per_screen_line_padded_with_spaces() {
        let snap = snapshot_of(5, 3, b"hi");
        assert_eq!(snap.rows.len(), 3);
        assert_eq!(
            snap.rows[0].segments,
            vec![seg(0, "hi   ", Style::default())]
        );
        assert_eq!(snap.rows[1].text(), "     ");
    }

    #[test]
    fn newline_moves_to_next_row() {
        let snap = snapshot_of(4, 2, b"a\r\nb");
        assert_eq!(snap.rows[0].text(), "a   ");
        assert_eq!(snap.rows[1].text(), "b   ");
    }

    #[test]
    fn sgr_colors_split_segments() {
        let snap = snapshot_of(6, 1, b"\x1b[31mred\x1b[0mok");
        assert_eq!(
            snap.rows[0].segments,
            vec![
                seg(0, "red", fg(ANSI_16[1])),
                seg(3, "ok ", Style::default())
            ]
        );
    }

    #[test]
    fn text_attributes() {
        let snap = snapshot_of(4, 1, b"\x1b[1ma\x1b[0;3mb\x1b[0;4mc\x1b[0;9md");
        let styles: Vec<Style> = snap.rows[0].segments.iter().map(|s| s.style).collect();
        assert!(styles[0].bold && !styles[0].italic);
        assert!(styles[1].italic && !styles[1].bold);
        assert!(styles[2].underline);
        assert!(styles[3].strikethrough);
    }

    #[test]
    fn inverse_swaps_colors() {
        let snap = snapshot_of(2, 1, b"\x1b[7mx\x1b[0m");
        assert_eq!(
            snap.rows[0].segments[0].style,
            Style {
                fg: DEFAULT_BACKGROUND,
                bg: DEFAULT_FOREGROUND,
                ..Style::default()
            }
        );
    }

    #[test]
    fn hidden_text_uses_background_color() {
        let snap = snapshot_of(2, 1, b"\x1b[8mx");
        let style = snap.rows[0].segments[0].style;
        assert_eq!(style.fg, style.bg);
    }

    #[test]
    fn dim_darkens_foreground() {
        let snap = snapshot_of(2, 1, b"\x1b[2mx");
        assert_eq!(snap.rows[0].segments[0].style.fg, dim(DEFAULT_FOREGROUND));
    }

    #[test]
    fn palette_changes_from_osc_are_applied() {
        let snap = snapshot_of(2, 1, b"\x1b]4;1;rgb:01/02/03\x07\x1b[31mx");
        assert_eq!(snap.rows[0].segments[0].style.fg, 0x010203);
    }

    #[test]
    fn wide_characters_get_their_own_two_cell_segment() {
        let snap = snapshot_of(5, 1, "aあb".as_bytes());
        assert_eq!(
            snap.rows[0].segments,
            vec![
                seg(0, "a", Style::default()),
                Segment {
                    col: 1,
                    cells: 2,
                    text: "あ".into(),
                    style: Style::default(),
                    monospaced: false,
                },
                seg(3, "b ", Style::default()),
            ]
        );
        assert_eq!(snap.rows[0].text(), "aあb ");
    }

    #[test]
    fn combining_characters_get_their_own_segment() {
        let snap = snapshot_of(3, 1, "e\u{301}x".as_bytes());
        assert_eq!(
            snap.rows[0].segments,
            vec![
                Segment {
                    col: 0,
                    cells: 1,
                    text: "e\u{301}".into(),
                    style: Style::default(),
                    monospaced: false,
                },
                seg(1, "x ", Style::default()),
            ]
        );
    }

    #[test]
    fn cursor_position_follows_output() {
        let snap = snapshot_of(5, 3, b"ab\r\nc");
        assert_eq!(
            snap.cursor,
            Some(Cursor {
                row: 1,
                col: 1,
                cells: 1
            })
        );
    }

    #[test]
    fn cursor_on_wide_character_covers_two_cells() {
        // 全角文字を書いた後、CHA でカーソルを 1 列目（その全角文字の上）に戻す
        let snap = snapshot_of(5, 1, "あ\x1b[1G".as_bytes());
        assert_eq!(
            snap.cursor,
            Some(Cursor {
                row: 0,
                col: 0,
                cells: 2
            })
        );
    }

    #[test]
    fn hidden_cursor() {
        let snap = snapshot_of(5, 3, b"\x1b[?25l");
        assert_eq!(snap.cursor, None);
    }

    fn term_with(cols: u16, rows: u16, bytes: &[u8]) -> Term<VoidListener> {
        let size = GridSize {
            cols,
            rows,
            cell_width: 1.0,
            cell_height: 1.0,
        };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: Processor<StdSyncHandler> = Processor::new();
        parser.advance(&mut term, bytes);
        term
    }

    #[test]
    fn cursor_position_is_known_while_hidden() {
        // TUI はカーソルを隠して自前で描くが、IME の変換位置にはカーソル位置を使う
        let term = term_with(5, 3, b"ab\r\nc\x1b[?25l");
        assert_eq!(
            cursor_position(&term),
            Some(Cursor {
                row: 1,
                col: 1,
                cells: 1
            })
        );
    }

    #[test]
    fn cursor_position_matches_visible_cursor() {
        let term = term_with(5, 1, "あ\x1b[1G".as_bytes());
        assert_eq!(cursor_position(&term), snapshot(&term).cursor);
    }

    /// 3 行の画面に 1〜6 を 1 行ずつ出力し、2 行分スクロールバックをさかのぼったもの。
    fn scrolled_term() -> Term<VoidListener> {
        let mut term = term_with(3, 3, b"1\r\n2\r\n3\r\n4\r\n5\r\n6");
        term.scroll_display(alacritty_terminal::grid::Scroll::Delta(2));
        term
    }

    #[test]
    fn snapshot_shows_scrollback_while_scrolled() {
        let snap = snapshot(&scrolled_term());
        let rows: Vec<String> = snap.rows.iter().map(|r| r.text()).collect();
        assert_eq!(rows, ["2  ", "3  ", "4  "]);
        // カーソルは最下部の画面にあるので見えない
        assert_eq!(snap.cursor, None);
    }

    #[test]
    fn bottom_screen_lines_ignore_scrolling() {
        assert_eq!(bottom_screen_lines(&scrolled_term()), ["4", "5", "6"]);
    }

    #[test]
    fn bottom_screen_lines_skip_wide_char_spacers() {
        let term = term_with(5, 1, "aあb".as_bytes());
        assert_eq!(bottom_screen_lines(&term), ["aあb"]);
    }

    #[test]
    fn clear_screen() {
        let snap = snapshot_of(3, 1, b"abc\x1b[2J");
        assert_eq!(snap.rows[0].text(), "   ");
    }
}
