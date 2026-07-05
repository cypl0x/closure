//! P6: feedback + interaction states render in every window from the
//! shared machines. `with_feedback` composes a `Feedback` queue onto a base
//! `ViewTree` as toast nodes (which every shell already renders, G1c/G8);
//! `ElementState::class` is the stable paint token each shell maps to its
//! native focus-ring / hover / disabled styling.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::{
    ElementState, Feedback, Node, ToastLevel, serialize_view, view_to_json, with_feedback,
};

fn base() -> Node {
    Node::Pane {
        title: "closure".into(),
        children: vec![Node::Text("body".into())],
    }
}

#[test]
fn with_feedback_appends_toasts_to_the_view() {
    let mut fb = Feedback::default();
    fb.notify(ToastLevel::Error, "sync failed");
    fb.progress("Indexing", 40);
    let view = with_feedback(base(), &fb);
    let json = view_to_json(&view);
    assert!(
        json.contains("\"k\":\"toast\""),
        "toasts present in the tree: {json}"
    );
    assert!(json.contains("sync failed"), "the notification renders");
    assert!(json.contains("Indexing 40%"), "progress renders");
    // The original content survives.
    assert!(json.contains("body"));
}

#[test]
fn with_feedback_is_a_noop_when_empty() {
    let fb = Feedback::default();
    assert_eq!(
        with_feedback(base(), &fb),
        base(),
        "no feedback ⇒ unchanged tree"
    );
}

#[test]
fn with_feedback_renders_for_the_llm_snapshot_too() {
    let mut fb = Feedback::default();
    fb.notify(ToastLevel::Success, "saved");
    let snap = serialize_view(&with_feedback(base(), &fb));
    assert!(snap.contains("TOAST success saved"));
}

#[test]
fn element_state_has_a_stable_paint_class() {
    assert_eq!(ElementState::Normal.class(), "normal");
    assert_eq!(ElementState::Hovered.class(), "hovered");
    assert_eq!(ElementState::Focused.class(), "focused");
    assert_eq!(ElementState::Active.class(), "active");
    assert_eq!(ElementState::Disabled.class(), "disabled");
    // Distinct classes — a shell can map each to a native style.
    let all = [
        ElementState::Normal,
        ElementState::Hovered,
        ElementState::Focused,
        ElementState::Active,
        ElementState::Disabled,
    ];
    for (i, s) in all.iter().enumerate() {
        for t in &all[i + 1..] {
            assert_ne!(s.class(), t.class());
        }
    }
}
