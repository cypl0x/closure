//! X2a hermetic core: the parse/print/query surface the browser will
//! call, tested natively (no wasm toolchain). The wasm-bindgen wrappers
//! are a thin pass-through built for wasm32 by `just wasm-web`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_wasm::{base64, headline_titles, headline_titles_joined, inline_wasm_editor, reformat};

#[test]
fn reformat_round_trips_valid_org_byte_exact() {
    let src = "* TODO Ship it\n:PROPERTIES:\n:ID: 01ABC\n:END:\nbody line\n** Sub\n";
    assert_eq!(reformat(src).expect("parses"), src, "I1 byte-exact");
}

#[test]
fn reformat_reports_nothing_to_panic_on_odd_input() {
    // Arbitrary text still parses as org (paragraphs); never panics.
    let r = reformat("just some text\n");
    assert!(r.is_ok());
}

#[test]
fn headline_titles_lists_in_document_order() {
    let src = "* First\n** Nested\n* Second\n";
    assert_eq!(headline_titles(src), vec!["First", "Nested", "Second"]);
    assert_eq!(headline_titles_joined(src), "First\nNested\nSecond");
}

#[test]
fn headline_titles_empty_for_no_headlines() {
    assert!(headline_titles("plain paragraph\n").is_empty());
}

#[test]
fn base64_matches_known_vectors() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foob"), "Zm9vYg==");
    assert_eq!(base64(b"hello"), "aGVsbG8=");
}

#[test]
fn inline_wasm_editor_embeds_glue_wasm_and_harness() {
    let html = inline_wasm_editor("<html><body>BASE</body></html>", "/*GLUE*/", b"WASM");
    assert!(html.contains("BASE"), "keeps the read-only page");
    assert!(html.contains("/*GLUE*/"), "inlines the wasm-bindgen glue");
    assert!(
        html.contains(&base64(b"WASM")),
        "inlines the wasm as base64"
    );
    assert!(html.contains("reformat("), "wires the client-side re-parse");
    assert!(html.contains("<textarea"), "offers an editor");
}

// === Q6-W4: offline command dispatch over org source (single-HTML edit). ===

#[test]
fn dispatch_command_rename_rewrites_the_source() {
    let org = "* Old\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n";
    let out = closure_wasm::dispatch_command(org, "rename", "01HXAAAAAAAAAAAAAAAAAAAAAA", "New")
        .expect("dispatch");
    assert!(out.contains("* New"), "{out}");
    assert!(out.contains(":ID: 01HXAAAAAAAAAAAAAAAAAAAAAA"), "id kept (I2)");
}

#[test]
fn dispatch_command_toggle_todo_and_demote() {
    let org = "* Task\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n";
    let out = closure_wasm::dispatch_command(org, "set-todo", "01HXAAAAAAAAAAAAAAAAAAAAAA", "TODO")
        .expect("todo");
    assert!(out.contains("* TODO Task"), "{out}");
    let out = closure_wasm::dispatch_command(&out, "demote", "01HXAAAAAAAAAAAAAAAAAAAAAA", "")
        .expect("demote");
    assert!(out.contains("** TODO Task"), "{out}");
}

#[test]
fn dispatch_command_errors_never_panic() {
    let org = "* A\n";
    assert!(closure_wasm::dispatch_command(org, "frobnicate", "x", "").is_err());
    assert!(
        closure_wasm::dispatch_command(org, "rename", "01XXXXXXXXXXXXXXXXXXXXXXXX", "t").is_err()
    );
}
