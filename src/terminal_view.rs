//! gpui でターミナルを描画・操作するビュー。

use std::ops::Range;

use alacritty_terminal::event::Event;
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    Context, FocusHandle, Focusable, Font, FontStyle, FontWeight, HighlightStyle, KeyDownEvent,
    Pixels, Render, StyledText, Task, UnderlineStyle, Window, div, font, prelude::*, px, rgb,
};

use crate::terminal::content::Row;
use crate::terminal::size::GridSize;
use crate::terminal::{SpawnOptions, Terminal, colors, keys};

const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT_RATIO: f32 = 1.3;
const PADDING: f32 = 4.0;

pub struct TerminalView {
    terminal: Terminal,
    focus_handle: FocusHandle,
    font: Font,
    cell_width: Pixels,
    cell_height: Pixels,
    _events: Task<()>,
}

impl TerminalView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> anyhow::Result<Self> {
        let text_system = window.text_system();
        let font = font(crate::font::choose_monospace_family(
            &text_system.all_font_names(),
        ));
        let font_size = px(FONT_SIZE);
        let font_id = text_system.resolve_font(&font);
        let cell_width = text_system.advance(font_id, font_size, 'm')?.width;
        let cell_height = (font_size * LINE_HEIGHT_RATIO).round();

        let viewport = window.viewport_size();
        let size = grid_size(viewport.width, viewport.height, cell_width, cell_height);
        let (terminal, events) = Terminal::spawn(SpawnOptions::default(), size)?;

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);

        Ok(Self {
            terminal,
            focus_handle,
            font,
            cell_width,
            cell_height,
            _events: Self::handle_events(events, window, cx),
        })
    }

    fn handle_events(
        mut events: UnboundedReceiver<Event>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                let result = match event {
                    Event::Wakeup => this.update(cx, |_, cx| cx.notify()),
                    Event::PtyWrite(text) => {
                        this.update(cx, |view, _| view.terminal.write(text.into_bytes()))
                    }
                    // fish などは起動時に色を問い合わせ、応答を待つので必ず返す
                    Event::ColorRequest(index, format) => this.update(cx, |view, _| {
                        if let Some(rgb) = colors::index_to_rgb(index) {
                            view.terminal.write(format(rgb).into_bytes());
                        }
                    }),
                    Event::TextAreaSizeRequest(format) => this.update(cx, |view, _| {
                        let size = view.terminal.size().window_size();
                        view.terminal.write(format(size).into_bytes());
                    }),
                    Event::Title(title) => cx.update(|window, _| window.set_window_title(&title)),
                    Event::Exit => {
                        let _ = cx.update(|window, _| window.remove_window());
                        break;
                    }
                    _ => Ok(()),
                };
                if result.is_err() {
                    break;
                }
            }
        })
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let app_cursor = self.terminal.snapshot().app_cursor;
        if let Some(bytes) = keys::keystroke_to_bytes(&event.keystroke, app_cursor) {
            self.terminal.write(bytes);
            cx.stop_propagation();
        }
    }
}

fn grid_size(width: Pixels, height: Pixels, cell_width: Pixels, cell_height: Pixels) -> GridSize {
    GridSize::from_pixels(
        f32::from(width) - PADDING * 2.0,
        f32::from(height) - PADDING * 2.0,
        f32::from(cell_width),
        f32::from(cell_height),
    )
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        self.terminal.resize(grid_size(
            viewport.width,
            viewport.height,
            self.cell_width,
            self.cell_height,
        ));
        let snapshot = self.terminal.snapshot();

        let cursor = snapshot.cursor.map(|cursor| {
            div()
                .absolute()
                .left(self.cell_width * cursor.col as f32)
                .top(self.cell_height * cursor.row as f32)
                .w(self.cell_width)
                .h(self.cell_height)
                .bg(rgb(colors::CURSOR))
                .opacity(0.6)
        });

        div()
            .id("terminal")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .p(px(PADDING))
            .bg(rgb(colors::DEFAULT_BACKGROUND))
            .text_color(rgb(colors::DEFAULT_FOREGROUND))
            .font(self.font.clone())
            .text_size(px(FONT_SIZE))
            .line_height(self.cell_height)
            .child(
                div()
                    .relative()
                    .children(snapshot.rows.iter().map(|row| {
                        div().h(self.cell_height).whitespace_nowrap().child(
                            StyledText::new(row.text.clone()).with_highlights(row_highlights(row)),
                        )
                    }))
                    .children(cursor),
            )
    }
}

/// 行の Run を gpui の StyledText 用ハイライトに変換する。
fn row_highlights(row: &Row) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut start = 0;
    row.runs
        .iter()
        .map(|run| {
            let range = start..start + run.len;
            start = range.end;
            let style = HighlightStyle {
                color: Some(rgb(run.fg).into()),
                background_color: Some(rgb(run.bg).into()),
                font_weight: run.bold.then_some(FontWeight::BOLD),
                font_style: run.italic.then_some(FontStyle::Italic),
                underline: run.underline.then(|| UnderlineStyle {
                    thickness: px(1.0),
                    ..Default::default()
                }),
                ..Default::default()
            };
            (range, style)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::content::Run;

    fn run(len: usize, fg: u32, bg: u32) -> Run {
        Run {
            len,
            fg,
            bg,
            bold: false,
            italic: false,
            underline: false,
        }
    }

    #[test]
    fn runs_become_contiguous_byte_ranges() {
        let row = Row {
            text: "abあ".into(),
            runs: vec![run(1, 0x111111, 0x222222), run(4, 0x333333, 0x444444)],
        };
        let highlights = row_highlights(&row);
        assert_eq!(highlights.len(), 2);
        assert_eq!(highlights[0].0, 0..1);
        assert_eq!(highlights[1].0, 1..5);
        assert_eq!(highlights[0].1.color, Some(rgb(0x111111).into()));
        assert_eq!(highlights[1].1.background_color, Some(rgb(0x444444).into()));
    }

    #[test]
    fn text_attributes() {
        let row = Row {
            text: "a".into(),
            runs: vec![Run {
                bold: true,
                italic: true,
                underline: true,
                ..run(1, 0, 0)
            }],
        };
        let style = row_highlights(&row)[0].1;
        assert_eq!(style.font_weight, Some(FontWeight::BOLD));
        assert_eq!(style.font_style, Some(FontStyle::Italic));
        assert_eq!(style.underline.map(|u| u.thickness), Some(px(1.0)));
    }

    #[test]
    fn plain_runs_have_no_attributes() {
        let row = Row {
            text: "a".into(),
            runs: vec![run(1, 0, 0)],
        };
        let style = row_highlights(&row)[0].1;
        assert_eq!(style.font_weight, None);
        assert_eq!(style.font_style, None);
        assert!(style.underline.is_none());
    }
}
