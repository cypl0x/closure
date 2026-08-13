//! Minimal gpui window that repaints on every keystroke.
//!
//! Built to answer one question the queue has carried for days: when
//! the closure window stops presenting under held-down keys on the
//! lavapipe software rasteriser, is that closure or is it gpui?
//!
//! There is no closure in here. One window, one counter, one key
//! handler that bumps it and calls `cx.notify()` — the same
//! `track_focus` / `on_key_down` shape the real root element uses, so
//! that the answer is about the rasteriser and not about a different
//! way of taking keys.
//!
//! If this wedges the same way, the fault is below anything the kernel
//! or the shell owns and this file is the artifact to file upstream. If
//! it does not, the fault is ours and the search space is whatever
//! closure does that this does not.
//!
//! Run with `DISPLAY=:1`:
//! `nix develop -c cargo run -p closure-shell-gpui --features gpui --example wedge_repro`

use gpui::{
    App, Application, Bounds, Context, Focusable, IntoElement, KeyDownEvent, Render, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};

struct Counter {
    /// How many keystrokes have been seen. Painted, so a frame that
    /// lands proves both halves at once: the key arrived *and* the
    /// picture changed. A number that stops climbing while the key is
    /// held is the wedge.
    seen: u64,
    focus: gpui::FocusHandle,
}

impl Focusable for Counter {
    fn focus_handle(&self, _: &App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Counter {
    fn on_key(&mut self, _ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.seen += 1;
        // Also to stdout, so the two halves can be told apart from
        // outside: a rising count here with a frozen window is input
        // arriving and presentation stopping, which is the whole
        // question.
        println!("key {}", self.seen);
        cx.notify();
    }
}

impl Render for Counter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle(cx))
            .on_key_down(cx.listener(Self::on_key))
            .flex()
            .size_full()
            .bg(rgb(0x001c_1e26))
            .text_color(rgb(0x00d5_d8da))
            .text_size(px(64.0))
            .child(format!("{}", self.seen))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(520.0), px(320.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| Counter {
                    seen: 0,
                    focus: cx.focus_handle(),
                });
                window.focus(&view.focus_handle(cx));
                view
            },
        );
        if opened.is_err() {
            eprintln!("could not open a window");
        }
    });
}
