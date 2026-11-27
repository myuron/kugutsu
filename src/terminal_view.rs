//! gpui でターミナルを描画・操作するビュー。

use alacritty_terminal::event::Event;
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    App, Bounds, ContentMask, Context, ElementInputHandler, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, Font, FontFeatures, FontStyle, FontWeight, Hsla, KeyDownEvent,
    Keystroke, Pixels, Point, Render, ScrollWheelEvent, StrikethroughStyle, Subscription, Task,
    TextRun, UTF16Selection, UnderlineStyle, Window, canvas, div, fill, font, point, prelude::*,
    px, rgb, size,
};

use crate::agents::{self, AgentTracker};
use crate::terminal::content::{Cursor, Snapshot, Style};
use crate::terminal::scroll::{self, Wheel, WheelAccumulator, WheelMode};
use crate::terminal::size::GridSize;
use crate::terminal::{Scroll, Terminal, TerminalEvent, colors, keys};

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

pub struct TerminalView {
    terminal: Terminal,
    focus_handle: FocusHandle,
    metrics: CellMetrics,
    title: Option<String>,
    /// IME で変換中の文字列（preedit）。PTY には確定するまで送らない。
    marked_text: Option<String>,
    /// フォアグラウンドで動いている Agent（[`TerminalView::refresh_agent`] で更新する）。
    agent: Option<AgentTracker>,
    /// ホイールの 1 行に満たない移動量の持ち越し。
    wheel: WheelAccumulator,
    /// 前回描画したセル格子の左上（ホイール位置をセルに換算するため）。
    grid_origin: Rc<Cell<Point<Pixels>>>,
    _events: Task<()>,
    _blur: Subscription,
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
        let focus_handle = cx.focus_handle();
        // フォーカスが移ると IME は変換中文字列をこのビューに消しに来ないことがある。
        // 残るとキー入力を握りつぶし続けるので、ここで捨てる
        let blur = cx.on_blur(&focus_handle, window, |view, _, cx| {
            if view.marked_text.take().is_some() {
                cx.notify();
            }
        });
        Self {
            terminal,
            focus_handle,
            metrics: CellMetrics::measure(window),
            title: None,
            marked_text: None,
            agent: None,
            wheel: WheelAccumulator::default(),
            grid_origin: Rc::default(),
            _events: Self::handle_events(events, window, cx),
            _blur: blur,
        }
    }

    /// プログラムが OSC で設定したタイトル。
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn grid_size(&self) -> GridSize {
        self.terminal.size()
    }

    /// フォアグラウンドで動いている Agent とその状態。
    pub fn agent(&self) -> Option<AgentTracker> {
        self.agent
    }

    /// フォアグラウンドのプロセスと画面から Agent の状態を更新する。`focused` はユーザーが
    /// このペインを見ているか。変化があれば true を返す。
    pub fn refresh_agent(&mut self, focused: bool) -> bool {
        let kind = self
            .terminal
            .foreground_process()
            .and_then(|process| agents::detect_agent(&process));
        let terminal = &self.terminal;
        agents::refresh(&mut self.agent, kind, || terminal.screen_lines(), focused)
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
        // 変換中のキーは IME のものなので PTY に送らない。伝播させると未処理キーとして
        // input handler に文字が渡ってしまうので、ここで止める
        if self.marked_text.is_some() {
            cx.stop_propagation();
            return;
        }
        if let Some(scroll) = self.page_scroll(&event.keystroke) {
            self.terminal.scroll(scroll);
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if let Some(bytes) = keys::keystroke_to_bytes(&event.keystroke, self.terminal.app_cursor())
        {
            // スクロールバックを見ていれば最下部に戻るので描き直す
            self.terminal.write(bytes);
            cx.notify();
            cx.stop_propagation();
        }
    }

    /// Shift+PageUp / Shift+PageDown でスクロールバックをページ単位で動かす。
    /// alternate screen にはスクロールバックが無いので、プログラムにそのまま渡す。
    fn page_scroll(&self, keystroke: &Keystroke) -> Option<Scroll> {
        let modifiers = &keystroke.modifiers;
        let shift_only = modifiers.shift
            && !(modifiers.control || modifiers.alt || modifiers.platform || modifiers.function);
        if !shift_only || self.terminal.alt_screen() {
            return None;
        }
        match keystroke.key.as_str() {
            "pageup" => Some(Scroll::PageUp),
            "pagedown" => Some(Scroll::PageDown),
            _ => None,
        }
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let lines = self.wheel.lines(event.delta, self.metrics.cell_height);
        if lines == 0 {
            return;
        }
        match self.terminal.wheel_mode() {
            WheelMode::Report { sgr } => {
                let (col, row) = cell_at(
                    event.position,
                    self.grid_origin.get(),
                    &self.metrics,
                    self.terminal.size(),
                );
                let wheel = if lines > 0 { Wheel::Up } else { Wheel::Down };
                let report = scroll::mouse_report(wheel, col, row, &event.modifiers, sgr);
                self.terminal
                    .write(report.repeat(lines.unsigned_abs() as usize));
            }
            WheelMode::Arrows { app_cursor } => {
                self.terminal
                    .write(scroll::alternate_scroll(lines, app_cursor));
            }
            WheelMode::Scrollback => {
                self.terminal.scroll(Scroll::Delta(lines));
                cx.notify();
            }
        }
    }
}

impl EventEmitter<TerminalViewEvent> for TerminalView {}

/// ターミナルは編集可能な文書を持たないので、IME の変換中文字列だけを「文書」として扱う。
/// 範囲はすべて変換中文字列の UTF-16 オフセット。
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let marked = self.marked_text.as_deref()?;
        let bytes = utf16_range_to_bytes(marked, range.clone());
        *adjusted_range = Some(range);
        Some(marked[bytes].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.marked_text.as_deref().map_or(0, utf16_len);
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_text.as_deref().map(|text| 0..utf16_len(text))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_text = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = None;
        if !text.is_empty() {
            self.terminal.write(text.as_bytes().to_vec());
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = (!new_text.is_empty()).then(|| new_text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let cursor = self.terminal.cursor_position()?;
        Some(cursor_cell_bounds(
            element_bounds.origin,
            &self.metrics,
            cursor.row,
            cursor.col,
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// UTF-16 オフセットの範囲を `text` のバイト範囲に変換する（範囲外は末尾に丸める）。
fn utf16_range_to_bytes(text: &str, range: Range<usize>) -> Range<usize> {
    let to_byte = |utf16_offset: usize| {
        let mut utf16 = 0;
        for (byte, c) in text.char_indices() {
            if utf16 >= utf16_offset {
                return byte;
            }
            utf16 += c.len_utf16();
        }
        text.len()
    };
    to_byte(range.start)..to_byte(range.end.max(range.start))
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.terminal.snapshot();
        let current_size = self.terminal.size();
        let entity = cx.entity();
        let view = entity.downgrade();
        let prepaint_metrics = self.metrics.clone();
        let paint_metrics = self.metrics.clone();
        let focus_handle = self.focus_handle.clone();
        let grid_origin = self.grid_origin.clone();
        // TUI はカーソルを隠して自前で描くので、表示状態に関わらないカーソル位置に置く
        let preedit = self.marked_text.clone().and_then(|text| {
            Some(Preedit {
                text,
                cursor: self.terminal.cursor_position()?,
            })
        });

        div()
            .id("terminal")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .size_full()
            .p(px(PADDING))
            .bg(rgb(colors::DEFAULT_BACKGROUND))
            .child(
                canvas(
                    // 要素自身の大きさからグリッドサイズを決める。変わっていれば PTY をリサイズし、
                    // 同じフレームで描けるようにスナップショットを取り直す（描画中の notify は無視されるため）
                    move |bounds, _, cx| {
                        grid_origin.set(bounds.origin);
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
                        // IME の確定・変換中文字列をこのビューで受け取る
                        window.handle_input(
                            &focus_handle,
                            ElementInputHandler::new(bounds, entity),
                            cx,
                        );
                        paint_snapshot(
                            &snapshot,
                            preedit.as_ref(),
                            &paint_metrics,
                            bounds,
                            window,
                            cx,
                        )
                    },
                )
                .size_full(),
            )
    }
}

/// `origin` を左上とするセル格子で、`row` 行 `col` 列のセルの矩形。
fn cursor_cell_bounds(
    origin: Point<Pixels>,
    metrics: &CellMetrics,
    row: usize,
    col: usize,
) -> Bounds<Pixels> {
    Bounds::new(
        point(
            origin.x + metrics.cell_width * col as f32,
            origin.y + metrics.cell_height * row as f32,
        ),
        size(metrics.cell_width, metrics.cell_height),
    )
}

/// ウィンドウ上の位置 `position` にある、`origin` を左上とするセル格子のセル（列, 行）。
/// 格子の外は端のセルに丸める。
fn cell_at(
    position: Point<Pixels>,
    origin: Point<Pixels>,
    metrics: &CellMetrics,
    grid: GridSize,
) -> (usize, usize) {
    let index = |offset: Pixels, cell: Pixels, count: u16| {
        let cells = (f32::from(offset) / f32::from(cell)).floor();
        (cells.max(0.0) as usize).min(usize::from(count).saturating_sub(1))
    };
    (
        index(position.x - origin.x, metrics.cell_width, grid.cols),
        index(position.y - origin.y, metrics.cell_height, grid.rows),
    )
}

/// スナップショットをセル格子に沿って描画する。`preedit` は IME の変換中文字列で、カーソル位置に重ねる。
fn paint_snapshot(
    snapshot: &Snapshot,
    preedit: Option<&Preedit>,
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
    let cell_origin =
        |row: usize, col: usize| cursor_cell_bounds(bounds.origin, metrics, row, col).origin;

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

    if let Some(preedit) = preedit {
        paint_preedit(preedit, metrics, bounds, window, cx);
    } else if let Some(cursor) = snapshot.cursor {
        let area = Bounds::new(
            cell_origin(cursor.row, cursor.col),
            size(cell_width * cursor.cells as f32, cell_height),
        );
        paint_cursor(area, window);
    }
}

fn paint_cursor(area: Bounds<Pixels>, window: &mut Window) {
    let mut color: Hsla = rgb(colors::CURSOR).into();
    color.a = CURSOR_OPACITY;
    window.paint_quad(fill(area, color));
}

/// IME の変換中文字列と、その位置（カーソルが非表示でも分かる位置）。
struct Preedit {
    text: String,
    cursor: Cursor,
}

/// 変換中文字列をカーソル位置に重ね、その後ろにカーソルを描く。ターミナルの外には描かない。
fn paint_preedit(
    preedit: &Preedit,
    metrics: &CellMetrics,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    // 自然な幅で描く（全角は 2 セル相当）
    let line = window.text_system().shape_line(
        preedit.text.clone().into(),
        metrics.font_size,
        &[preedit_run(preedit.text.len(), &metrics.font)],
        None,
    );
    let anchor = cursor_cell_bounds(
        bounds.origin,
        metrics,
        preedit.cursor.row,
        preedit.cursor.col,
    )
    .origin;
    let (text_origin, cursor) = preedit_layout(anchor, line.width, metrics, bounds);
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        let area = Bounds::new(text_origin, size(line.width, metrics.cell_height));
        window.paint_quad(fill(area, rgb(colors::DEFAULT_BACKGROUND)));
        let _ = line.paint(text_origin, metrics.cell_height, window, cx);
        paint_cursor(cursor, window);
    });
}

/// 変換中文字列の描画位置と、その後ろに置くカーソルの矩形。
///
/// カーソルは挿入位置を示すので、下のセルが全角でも 1 セル幅にする。文字列とカーソルが右端から
/// はみ出す場合は左へずらす（それでも収まらなければ左端から描き、はみ出した分は切り取られる）。
fn preedit_layout(
    anchor: Point<Pixels>,
    text_width: Pixels,
    metrics: &CellMetrics,
    bounds: Bounds<Pixels>,
) -> (Point<Pixels>, Bounds<Pixels>) {
    let total_width = text_width + metrics.cell_width;
    let x = anchor
        .x
        .min(bounds.right() - total_width)
        .max(bounds.left());
    let text_origin = point(x, anchor.y);
    let cursor = Bounds::new(
        point(x + text_width, anchor.y),
        size(metrics.cell_width, metrics.cell_height),
    );
    (text_origin, cursor)
}

/// IME の変換中文字列のスタイル（既定の前景色に下線）。
fn preedit_run(len: usize, base_font: &Font) -> TextRun {
    text_run(
        &Style {
            fg: colors::DEFAULT_FOREGROUND,
            underline: true,
            ..Style::default()
        },
        len,
        base_font,
    )
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

    #[test]
    fn preedit_run_is_underlined() {
        let run = preedit_run(6, &font("Mono"));
        assert_eq!(run.len, 6);
        assert_eq!(run.font.family.as_ref(), "Mono");
        assert_eq!(run.color, rgb(colors::DEFAULT_FOREGROUND).into());
        assert!(run.underline.is_some());
    }

    #[test]
    fn cursor_cell_bounds_follow_the_grid() {
        let metrics = CellMetrics {
            font: font("Mono"),
            font_size: px(14.0),
            cell_width: px(8.0),
            cell_height: px(16.0),
        };
        let bounds = cursor_cell_bounds(point(px(10.0), px(20.0)), &metrics, 2, 3);
        assert_eq!(bounds.origin, point(px(34.0), px(52.0)));
        assert_eq!(bounds.size, size(px(8.0), px(16.0)));
    }

    fn test_metrics() -> CellMetrics {
        CellMetrics {
            font: font("Mono"),
            font_size: px(14.0),
            cell_width: px(10.0),
            cell_height: px(20.0),
        }
    }

    #[test]
    fn preedit_starts_at_cursor_and_cursor_follows_it() {
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(200.0), px(100.0)));
        let (text_origin, cursor) =
            preedit_layout(point(px(30.0), px(40.0)), px(50.0), &test_metrics(), bounds);
        assert_eq!(text_origin, point(px(30.0), px(40.0)));
        // 挿入位置を示すので、下のセルが全角でもカーソルは 1 セル幅
        assert_eq!(
            cursor,
            Bounds::new(point(px(80.0), px(40.0)), size(px(10.0), px(20.0)))
        );
    }

    #[test]
    fn preedit_near_right_edge_is_shifted_left() {
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(200.0), px(100.0)));
        let (text_origin, cursor) = preedit_layout(
            point(px(170.0), px(40.0)),
            px(50.0),
            &test_metrics(),
            bounds,
        );
        // 文字列とカーソル（計 60px）が右端に収まるよう左へずらす
        assert_eq!(text_origin, point(px(140.0), px(40.0)));
        assert_eq!(cursor.origin, point(px(190.0), px(40.0)));
    }

    #[test]
    fn preedit_wider_than_terminal_starts_at_left_edge() {
        let bounds = Bounds::new(point(px(5.0), px(0.0)), size(px(100.0), px(100.0)));
        let (text_origin, _) = preedit_layout(
            point(px(50.0), px(40.0)),
            px(300.0),
            &test_metrics(),
            bounds,
        );
        assert_eq!(text_origin, point(px(5.0), px(40.0)));
    }

    #[test]
    fn cell_at_maps_positions_into_the_grid() {
        let metrics = test_metrics();
        let origin = point(px(5.0), px(5.0));
        let grid = GridSize::from_pixels(100.0, 100.0, 10.0, 20.0);
        assert_eq!(
            cell_at(point(px(5.0), px(5.0)), origin, &metrics, grid),
            (0, 0)
        );
        assert_eq!(
            cell_at(point(px(39.0), px(46.0)), origin, &metrics, grid),
            (3, 2)
        );
        // 余白や外側は端のセルに丸める
        assert_eq!(
            cell_at(point(px(0.0), px(0.0)), origin, &metrics, grid),
            (0, 0)
        );
        assert_eq!(
            cell_at(point(px(999.0), px(999.0)), origin, &metrics, grid),
            (9, 4)
        );
    }

    mod scrolling {
        use super::ime::{open_script, wait_for_screen};
        use super::*;
        use gpui::{Entity, ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext};

        /// 画面からあふれる出力をした後に `ready` を表示し、1 行読んで `got:<行>|` と表示する。
        const LONG_OUTPUT: &str =
            "seq 1 200; printf ready; read line; printf \"got:$line|\"; sleep 30";

        fn wheel(cx: &mut VisualTestContext, lines: f32) {
            wheel_at(cx, point(px(20.0), px(20.0)), lines);
        }

        fn wheel_at(cx: &mut VisualTestContext, position: Point<Pixels>, lines: f32) {
            cx.simulate_event(ScrollWheelEvent {
                position,
                delta: ScrollDelta::Lines(point(0.0, lines)),
                ..ScrollWheelEvent::default()
            });
        }

        fn display_offset(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> usize {
            view.read_with(cx, |view, _| view.terminal.display_offset())
        }

        #[gpui::test]
        fn wheel_scrolls_through_scrollback(cx: &mut TestAppContext) {
            let (view, cx) = open_script(cx, LONG_OUTPUT);
            wheel(cx, 3.0);
            assert_eq!(display_offset(&view, cx), 3);
            wheel(cx, -1.0);
            assert_eq!(display_offset(&view, cx), 2);
        }

        #[gpui::test]
        fn typing_returns_to_bottom(cx: &mut TestAppContext) {
            let (view, cx) = open_script(cx, LONG_OUTPUT);
            wheel(cx, 3.0);
            cx.simulate_input("ab");
            assert_eq!(display_offset(&view, cx), 0);
            cx.simulate_keystrokes("enter");
            wait_for_screen(&view, cx, "got:ab|");
        }

        #[gpui::test]
        fn shift_page_keys_scroll_without_reaching_the_program(cx: &mut TestAppContext) {
            let (view, cx) = open_script(cx, LONG_OUTPUT);
            let rows = view.read_with(cx, |view, _| view.grid_size().rows as usize);
            cx.simulate_keystrokes("shift-pageup shift-pageup");
            assert_eq!(display_offset(&view, cx), rows * 2);
            cx.simulate_keystrokes("shift-pagedown");
            assert_eq!(display_offset(&view, cx), rows);
            cx.simulate_keystrokes("enter");
            wait_for_screen(&view, cx, "got:|");
        }

        #[gpui::test]
        fn wheel_is_reported_to_programs_using_the_mouse(cx: &mut TestAppContext) {
            // SGR 形式のマウスレポートを有効にし、届いた 10 バイトを表示する
            let script = "stty raw -echo; printf '\\033[?1000h\\033[?1006hready'; \
                reply=$(dd bs=1 count=10 2>/dev/null); \
                printf 'got:%s|' \"$reply\" | tr -d '\\033'; sleep 30";
            let (view, cx) = open_script(cx, script);
            let metrics = view.read_with(cx, |view, _| view.metrics.clone());
            // 3 列目・2 行目のセルの中
            let position = point(
                px(PADDING) + metrics.cell_width * 2.5,
                px(PADDING) + metrics.cell_height * 1.5,
            );
            wheel_at(cx, position, 1.0);
            wait_for_screen(&view, cx, "got:[<64;3;2M|");
        }

        #[gpui::test]
        fn wheel_sends_arrows_on_alternate_screen(cx: &mut TestAppContext) {
            let script = "stty raw -echo; printf '\\033[?1049hready'; \
                reply=$(dd bs=1 count=6 2>/dev/null); \
                printf 'got:%s|' \"$reply\" | tr -d '\\033'; sleep 30";
            let (view, cx) = open_script(cx, script);
            wheel(cx, -2.0);
            wait_for_screen(&view, cx, "got:[B[B|");
        }
    }

    mod ime {
        use super::*;
        use crate::terminal::{SpawnOptions, Terminal};
        use crate::workspace_view::INITIAL_SIZE;
        use gpui::{Entity, EntityInputHandler, TestAppContext, VisualTestContext};
        use std::time::{Duration, Instant};

        /// 1 行読んで `got:<行>|` と表示するプログラムを動かすビューを開き、フォーカスする。
        fn open_reader(cx: &mut TestAppContext) -> (Entity<TerminalView>, &mut VisualTestContext) {
            // 出力直後にプロセスが終了するとその出力が画面に反映されないことがあるため、
            // 表示後もしばらく動かしておく
            open_script(
                cx,
                "printf ready; read line; printf \"got:$line|\"; sleep 30",
            )
        }

        /// `script` を動かすビューを開いてフォーカスし、`ready` が表示されるまで待つ。
        pub(super) fn open_script<'a>(
            cx: &'a mut TestAppContext,
            script: &str,
        ) -> (Entity<TerminalView>, &'a mut VisualTestContext) {
            let options = SpawnOptions {
                program: Some(("sh".into(), vec!["-c".into(), script.into()])),
            };
            let (terminal, events) = Terminal::spawn(options, INITIAL_SIZE).unwrap();
            let (view, cx) =
                cx.add_window_view(|window, cx| TerminalView::new(terminal, events, window, cx));
            cx.update(|window, cx| window.focus(&view.focus_handle(cx)));
            wait_for_screen(&view, cx, "ready");
            (view, cx)
        }

        /// 画面に `needle` が現れるまで待つ（PTY 上のプロセスは実時間で動くため）。
        pub(super) fn wait_for_screen(
            view: &Entity<TerminalView>,
            cx: &mut VisualTestContext,
            needle: &str,
        ) {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                cx.run_until_parked();
                let text = view.read_with(cx, |view, _| view.terminal.screen_lines().concat());
                if text.contains(needle) {
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "{needle:?} never shown: {text:?}"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn set_preedit(view: &Entity<TerminalView>, cx: &mut VisualTestContext, text: &str) {
            view.update_in(cx, |view, window, cx| {
                view.replace_and_mark_text_in_range(None, text, None, window, cx)
            });
        }

        fn commit(
            view: &Entity<TerminalView>,
            cx: &mut VisualTestContext,
            range: Option<std::ops::Range<usize>>,
            text: &str,
        ) {
            view.update_in(cx, |view, window, cx| {
                view.replace_text_in_range(range, text, window, cx)
            });
        }

        fn marked_range(
            view: &Entity<TerminalView>,
            cx: &mut VisualTestContext,
        ) -> Option<std::ops::Range<usize>> {
            view.update_in(cx, |view, window, cx| view.marked_text_range(window, cx))
        }

        #[gpui::test]
        fn ime_commit_writes_text_to_pty(cx: &mut TestAppContext) {
            let (view, cx) = open_reader(cx);
            set_preedit(&view, cx, "にほん");
            assert_eq!(marked_range(&view, cx), Some(0..3));

            commit(&view, cx, None, "日本");
            assert_eq!(marked_range(&view, cx), None);
            cx.simulate_keystrokes("enter");
            wait_for_screen(&view, cx, "got:日本|");
        }

        #[gpui::test]
        fn cancelled_preedit_is_not_sent_to_pty(cx: &mut TestAppContext) {
            let (view, cx) = open_reader(cx);
            set_preedit(&view, cx, "にほん");
            commit(&view, cx, Some(0..3), "");
            assert_eq!(marked_range(&view, cx), None);
            cx.simulate_keystrokes("enter");
            wait_for_screen(&view, cx, "got:|");
        }

        #[gpui::test]
        fn keys_are_not_sent_while_composing(cx: &mut TestAppContext) {
            let (view, cx) = open_reader(cx);
            set_preedit(&view, cx, "にほん");
            cx.simulate_keystrokes("enter");
            commit(&view, cx, None, "日本");
            cx.simulate_keystrokes("enter");
            wait_for_screen(&view, cx, "got:日本|");
        }

        #[gpui::test]
        fn ascii_input_is_not_doubled(cx: &mut TestAppContext) {
            let (view, cx) = open_reader(cx);
            cx.simulate_input("ab");
            cx.simulate_keystrokes("enter");
            wait_for_screen(&view, cx, "got:ab|");
        }

        #[gpui::test]
        fn selection_sits_at_end_of_preedit(cx: &mut TestAppContext) {
            let (view, cx) = open_reader(cx);
            set_preedit(&view, cx, "にほん");
            let selection = view
                .update_in(cx, |view, window, cx| {
                    view.selected_text_range(false, window, cx)
                })
                .unwrap();
            assert_eq!(selection.range, 3..3);
            let text = view.update_in(cx, |view, window, cx| {
                view.text_for_range(1..3, &mut None, window, cx)
            });
            assert_eq!(text.as_deref(), Some("ほん"));
        }

        #[gpui::test]
        fn ime_area_follows_hidden_cursor(cx: &mut TestAppContext) {
            // TUI のようにカーソルを隠して描くプログラム（2 行目の 3 列目にカーソル）
            let (view, cx) = open_script(cx, "printf 'ready\\r\\nab\\033[?25l'; sleep 30");
            set_preedit(&view, cx, "にほん");
            let element = Bounds::new(point(px(0.0), px(0.0)), size(px(400.0), px(300.0)));
            let (area, metrics) = view.update_in(cx, |view, window, cx| {
                (
                    view.bounds_for_range(0..0, element, window, cx),
                    view.metrics.clone(),
                )
            });
            assert_eq!(
                area,
                Some(cursor_cell_bounds(element.origin, &metrics, 1, 2))
            );
        }

        #[gpui::test]
        fn losing_focus_discards_preedit(cx: &mut TestAppContext) {
            let (view, cx) = open_reader(cx);
            // 非アクティブなウィンドウではフォーカスの変化が通知されない
            cx.update(|window, _| window.activate_window());
            cx.run_until_parked();
            set_preedit(&view, cx, "にほん");
            cx.update(|window, _| window.blur());
            cx.run_until_parked();
            assert_eq!(marked_range(&view, cx), None);

            // 戻ってきたらキー入力が効く
            cx.update(|window, cx| window.focus(&view.focus_handle(cx)));
            cx.simulate_input("ab");
            cx.simulate_keystrokes("enter");
            wait_for_screen(&view, cx, "got:ab|");
        }
    }
}
