//! G7: async feedback surface. A typed `Feedback` queue collects
//! notifications + progress for long ops (sync / eval / llm) as state, and
//! `to_nodes()` renders each as a `Node::Toast` (G1c) — so *every* shell
//! already displays it, no per-shell notification code.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{Feedback, FeedbackKind, Node, ToastLevel};

#[test]
fn notifications_queue_with_their_severity() {
    let mut fb = Feedback::default();
    fb.notify(ToastLevel::Success, "saved");
    fb.notify(ToastLevel::Error, "sync failed");
    assert_eq!(fb.items().len(), 2);
    assert_eq!(fb.items()[1].kind, FeedbackKind::Error);
}

#[test]
fn progress_for_a_long_op_updates_in_place() {
    let mut fb = Feedback::default();
    fb.progress("Syncing", 10);
    fb.progress("Syncing", 80);
    assert_eq!(fb.items().len(), 1, "same label updates, not stacks");
    assert_eq!(fb.items()[0].kind, FeedbackKind::Progress(80));
}

#[test]
fn feedback_renders_as_toast_nodes_every_shell_already_draws() {
    let mut fb = Feedback::default();
    fb.notify(ToastLevel::Warning, "unsaved");
    fb.progress("Indexing", 42);
    let nodes = fb.to_nodes();
    assert_eq!(nodes.len(), 2);
    // Notifications and progress both become toasts (G1c vocabulary).
    assert!(matches!(
        nodes[0],
        Node::Toast {
            level: ToastLevel::Warning,
            ..
        }
    ));
    match &nodes[1] {
        Node::Toast { level, text } => {
            assert_eq!(*level, ToastLevel::Info, "progress is a polite status");
            assert!(
                text.contains("Indexing") && text.contains("42%"),
                "label + pct: {text}"
            );
        }
        other => panic!("progress should render as a toast, got {other:?}"),
    }
}

#[test]
fn a_completed_long_op_can_clear_its_progress_and_announce() {
    let mut fb = Feedback::default();
    fb.progress("Eval", 0);
    fb.progress("Eval", 100);
    fb.clear();
    assert!(fb.items().is_empty(), "cleared");
    fb.notify(ToastLevel::Success, "Eval done");
    assert_eq!(fb.items().len(), 1);
}
