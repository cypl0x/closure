//! D4: the full rendered-state loop, hermetic. An OpenAI-wire model
//! READS the `ViewTree` (V3 render tool, permission-gated) and then
//! MUTATES the vault — and the mutation only ever flows through a
//! registry command (I8: `Shell::capture`). After the turn, the
//! re-rendered view reflects the change: proof the agent both perceives
//! rendered state and acts on it, with no network and no privileged
//! `&mut Document`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_llm::{LlmPermissions, OpenAiWireProvider, RENDER_TOOL, tool_loop};
use closure_shell_core::{App, Shell, view_to_json};
use closure_store::Vault;
use tempfile::TempDir;

fn shell() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(dir.path().join("notes.org"), "* Existing\n").expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(v))
}

/// Render the current screen to JSON — the V3 view-render tool surface.
fn render(app: &App, shell: &Shell) -> String {
    view_to_json(&app.view(shell))
}

#[test]
fn model_reads_view_then_mutates_through_registry() {
    let (_d, mut sh) = shell();
    let app = App::new();
    let perms = LlmPermissions::from_config(vec![RENDER_TOOL.to_owned()]);

    let before = render(&app, &sh);
    assert!(!before.contains("Buy milk"), "not captured yet: {before}");

    // The model's scripted turns: first observe the rendered state, then
    // act on it, then finish.
    let provider = OpenAiWireProvider::scripted(&[
        "CALL view-render",
        "CALL capture Buy milk",
        "DONE captured after viewing",
    ]);

    let mut observed_view = String::new();
    let answer = tool_loop(
        &provider,
        |cmd| {
            if let Some(rest) = cmd.strip_prefix("view-render") {
                let _ = rest;
                // Render tool is permission-gated (V3): only exposed when
                // granted; never mutates.
                if perms.allows(RENDER_TOOL) {
                    observed_view = render(&app, &sh);
                    observed_view.clone()
                } else {
                    "DENIED: render not permitted".to_owned()
                }
            } else if let Some(title) = cmd.strip_prefix("capture ") {
                // Mutation flows ONLY through the registry command (I8).
                match sh.capture(title) {
                    Ok(()) => "ok".to_owned(),
                    Err(e) => format!("err: {e}"),
                }
            } else {
                "unknown command".to_owned()
            }
        },
        "capture a todo after looking at the screen",
        6,
    )
    .expect("loop");

    assert_eq!(answer, "captured after viewing");
    assert!(
        observed_view.contains("Existing"),
        "the model saw the rendered ViewTree (V3): {observed_view}"
    );

    // The re-rendered view now reflects the registry mutation.
    let after = render(&app, &sh);
    assert!(
        after.contains("Buy milk"),
        "view changed after the agent's command: {after}"
    );
    assert_ne!(before, after, "rendered state observably changed");
}

#[test]
fn render_tool_is_denied_when_permission_is_off() {
    let (_d, mut sh) = shell();
    let app = App::new();
    // Render OFF by default (V3b): the model cannot read the ViewTree.
    let perms = LlmPermissions::from_config(vec![]);
    assert!(!perms.allows(RENDER_TOOL));

    let provider = OpenAiWireProvider::scripted(&["CALL view-render", "DONE done"]);
    let mut last = String::new();
    let _ = tool_loop(
        &provider,
        |cmd| {
            if cmd.starts_with("view-render") {
                last = if perms.allows(RENDER_TOOL) {
                    render(&app, &sh)
                } else {
                    "DENIED: render not permitted".to_owned()
                };
                last.clone()
            } else {
                let _ = &mut sh;
                "noop".to_owned()
            }
        },
        "try to peek",
        4,
    )
    .expect("loop");

    assert_eq!(
        last, "DENIED: render not permitted",
        "render gated off (V3b)"
    );
}
