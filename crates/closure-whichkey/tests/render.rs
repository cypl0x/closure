#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::InputMode;
use closure_core::{Registry, RenameHeadline};
use closure_input::Dispatcher;

#[test]
fn render_includes_every_registered_chord() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let out = closure_whichkey::render(&disp);
    assert!(out.contains("rename-headline"));
    assert!(out.contains("C-c C-x r"));
}

#[test]
fn prefix_filters_listing() {
    let mut reg = Registry::new();
    reg.register(Box::new(RenameHeadline::new_placeholder()));
    let disp = Dispatcher::from_registry(&reg, InputMode::Doom);
    let nothing = closure_whichkey::render_prefix(&disp, "SPC");
    assert!(nothing.is_empty());
    let some = closure_whichkey::render_prefix(&disp, "C-c");
    assert!(some.contains("rename-headline"));
}
