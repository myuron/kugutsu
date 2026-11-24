//! alacritty のイベントを futures のチャネルへ転送する。

use alacritty_terminal::event::{Event, EventListener};
use futures::channel::mpsc::UnboundedSender;

#[derive(Clone)]
pub struct Listener(pub UnboundedSender<Event>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        // 受信側（ビュー）が既に破棄されていれば捨てる
        let _ = self.0.unbounded_send(event);
    }
}
