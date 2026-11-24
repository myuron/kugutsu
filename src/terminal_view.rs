//! gpui でターミナルを描画・操作するビュー。

use alacritty_terminal::event::Event;
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    App, Bounds, Context, EventEmitter, FocusHandle, Focusable, Font, FontFeatures, FontStyle,
    FontWeight, Hsla, KeyDownEvent, Pixels, Render, StrikethroughStyle, Task, TextRun,
    UnderlineStyle, Window, canvas, div, fill, font, point, prelude::*, px, rgb, size,
};

use crate::terminal::content::{Snapshot, Style};
use crate::terminal::size::GridSize;
use crate::terminal::{Terminal, TerminalEvent, colors, keys};

const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT_RATIO: f32 = 1.3;
const PADDING: f32 = 4.0;
const CURSOR_OPACITY: f32 = 0.6;
/// 等幅フォントが見つからないときに gpui へ渡すファミリー名。
const FALLBACK_FONT_FAMILY: &str = "monospace";

/// 親（タブなど）が反応すべきビューのイベント。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalViewEvent {
    /// [`TerminalView::title`] が変わった。
    TitleChanged,
    /// プロセスが終了した。
    Exited,
}

// TODO: IME（日本語入力）対応。EntityInputHandler を実装して確定文字列を PTY に書き込む。
pub struct TerminalView {
    terminal: Terminal,
    focus_handle: FocusHandle,
    metrics: CellMetrics,
    title: Option<String>,
    _events: Task<()>,
}

/// セル格子の描画に使うフォントとセルサイズ。
#[derive(Clone)]
struct CellMetrics {
    font: Font,
    font_size: Pixels,
    cell_width: Pixels,
    cell_height: Pixels,
}

impl CellMetrics {
    fn measure(window: &Window) -> Self {
        let text_system = window.text_system();
        let family = crate::font::choose_monospace_family(&text_system.all_font_names())
            .unwrap_or_else(|| {
                eprintln!("warning: no monospace font found; terminal cells may be misaligned");
                FALLBACK_FONT_FAMILY.to_string()
            });
        let font = Font {
            // 合字（-> や != など）はセル格子を崩すので無効にする
            features: FontFeatures::disable_ligatures(),
            ..font(family)
        };
        let font_size = px(FONT_SIZE);
        let font_id = text_system.resolve_font(&font);
        let cell_width = text_system
            .advance(font_id, font_size, 'm')
            .map(|advance| advance.width)
            .unwrap_or(font_size * 0.6);
        Self {
            font,
            font_size,
            cell_width,
            cell_height: (font_size * LINE_HEIGHT_RATIO).round(),
        }
    }

    fn grid_size(&self, bounds: Bounds<Pixels>) -> GridSize {
        GridSize::from_pixels(
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
            f32::from(self.cell_width),
            f32::from(self.cell_height),
        )
    }
}

impl TerminalView {
    pub fn new(
        terminal: Terminal,
        events: UnboundedReceiver<Event>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            terminal,
            focus_handle: cx.focus_handle(),
            metrics: CellMetrics::measure(window),
            title: None,
            _events: Self::handle_events(events, window, cx),
        }
    }

    /// プログラムが OSC で設定したタイトル。
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn grid_size(&self) -> GridSize {
        self.terminal.size()
    }

    fn handle_events(
        mut events: UnboundedReceiver<Event>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                let Ok(event) = this.update(cx, |view, _| view.terminal.handle_event(event)) else {
                    break;
                };
                let Some(event) = event else { continue };
                let exited = event == TerminalEvent::Exit;
                let updated = this.update(cx, |view, cx| match event {
                    TerminalEvent::Wakeup => cx.notify(),
                    TerminalEvent::Title(title) => view.set_title(Some(title), cx),
                    TerminalEvent::ResetTitle => view.set_title(None, cx),
                    TerminalEvent::Exit => cx.emit(TerminalViewEvent::Exited),
                });
                if exited || updated.is_err() {
                    break;
                }
            }
        })
    }

    fn set_title(&mut self, title: Option<String>, cx: &mut Context<Self>) {
        if self.title != title {
            self.title = title;
            cx.emit(TerminalViewEvent::TitleChanged);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(bytes) = keys::keystroke_to_bytes(&event.keystroke, self.terminal.app_cursor())
        {
            self.terminal.write(bytes);
            cx.stop_propagation();
        }
    }
}

impl EventEmitter<TerminalViewEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.terminal.snapshot();
        let current_size = self.terminal.size();
        let view = cx.entity().downgrade();
        let prepaint_metrics = self.metrics.clone();
        let paint_metrics = self.metrics.clone();

        div()
            .id("terminal")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .p(px(PADDING))
            .bg(rgb(colors::DEFAULT_BACKGROUND))
            .child(
                canvas(
                    // 要素自身の大きさからグリッドサイズを決める。変わっていれば PTY をリサイズし、
                    // 同じフレームで描けるようにスナップショットを取り直す（描画中の notify は無視されるため）
                    move |bounds, _, cx| {
                        let size = prepaint_metrics.grid_size(bounds);
                        if size == current_size {
                            return snapshot;
                        }
                        view.update(cx, |view, _| {
                            view.terminal.resize(size);
                            view.terminal.snapshot()
                        })
                        .unwrap_or(snapshot)
                    },
                    move |bounds, snapshot, window, cx| {
                        paint_snapshot(&snapshot, &paint_metrics, bounds, window, cx)
                    },
                )
                .size_full(),
            )
    }
}

/// スナップショットをセル格子に沿って描画する。
fn paint_snapshot(
    snapshot: &Snapshot,
    metrics: &CellMetrics,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let CellMetrics {
        cell_width,
        cell_height,
        ..
    } = *metrics;
    let cell_origin = |row: usize, col: usize| {
        point(
            bounds.origin.x + cell_width * col as f32,
            bounds.origin.y + cell_height * row as f32,
        )
    };

    for (row_index, row) in snapshot.rows.iter().enumerate() {
        for segment in &row.segments {
            let origin = cell_origin(row_index, segment.col);
            if segment.style.bg != colors::DEFAULT_BACKGROUND {
                let area =
                    Bounds::new(origin, size(cell_width * segment.cells as f32, cell_height));
                window.paint_quad(fill(area, rgb(segment.style.bg)));
            }
            if is_blank(&segment.text, &segment.style) {
                continue;
            }
            let run = text_run(&segment.style, segment.text.len(), &metrics.font);
            // 半角の連続区間はグリフ送り幅をセル幅に固定し、フォールバックグリフでも格子からずれないようにする
            let force_width = segment.monospaced.then_some(cell_width);
            let line = window.text_system().shape_line(
                segment.text.clone().into(),
                metrics.font_size,
                &[run],
                force_width,
            );
            let _ = line.paint(origin, cell_height, window, cx);
        }
    }

    if let Some(cursor) = snapshot.cursor {
        let area = Bounds::new(
            cell_origin(cursor.row, cursor.col),
            size(cell_width * cursor.cells as f32, cell_height),
        );
        let mut color: Hsla = rgb(colors::CURSOR).into();
        color.a = CURSOR_OPACITY;
        window.paint_quad(fill(area, color));
    }
}

/// 描画しても見た目が変わらない区間（空白のみで線の装飾も無い）か。
fn is_blank(text: &str, style: &Style) -> bool {
    !style.underline && !style.strikethrough && text.chars().all(|c| c == ' ')
}

/// 区間のスタイルを gpui の TextRun に変換する（背景は別途矩形で塗る）。
fn text_run(style: &Style, len: usize, base_font: &Font) -> TextRun {
    let color: Hsla = rgb(style.fg).into();
    TextRun {
        len,
        font: Font {
            weight: if style.bold {
                FontWeight::BOLD
            } else {
                base_font.weight
            },
            style: if style.italic {
                FontStyle::Italic
            } else {
                base_font.style
            },
            ..base_font.clone()
        },
        color,
        background_color: None,
        underline: style.underline.then(|| UnderlineStyle {
            thickness: px(1.0),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: style.strikethrough.then(|| StrikethroughStyle {
            thickness: px(1.0),
            color: Some(color),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_run_uses_foreground_and_length() {
        let style = Style {
            fg: 0x112233,
            ..Style::default()
        };
        let run = text_run(&style, 5, &font("Mono"));
        assert_eq!(run.len, 5);
        assert_eq!(run.color, rgb(0x112233).into());
        assert_eq!(run.background_color, None);
        assert_eq!(run.font.weight, FontWeight::default());
        assert_eq!(run.font.style, FontStyle::Normal);
        assert!(run.underline.is_none() && run.strikethrough.is_none());
    }

    #[test]
    fn text_run_attributes() {
        let style = Style {
            bold: true,
            italic: true,
            underline: true,
            strikethrough: true,
            ..Style::default()
        };
        let run = text_run(&style, 1, &font("Mono"));
        assert_eq!(run.font.family.as_ref(), "Mono");
        assert_eq!(run.font.weight, FontWeight::BOLD);
        assert_eq!(run.font.style, FontStyle::Italic);
        assert!(run.underline.is_some());
        assert!(run.strikethrough.is_some());
    }

    #[test]
    fn blank_segments() {
        let plain = Style::default();
        assert!(is_blank("   ", &plain));
        assert!(!is_blank(" a ", &plain));
        let underlined = Style {
            underline: true,
            ..plain
        };
        assert!(!is_blank("   ", &underlined));
    }
}
