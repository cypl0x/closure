//! Assemble the self-contained client-side editor HTML (X2b) from a
//! wasm-bindgen `--target web` bundle. Used by `just wasm-web-bundle`:
//!
//! `build_editor <glue.js> <module.wasm> [base_page.html] > editor.html`
//!
//! `base_page` defaults to a minimal placeholder; in a real export it is
//! the `closure export html` page.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let glue = fs::read_to_string(&args[1]).expect("read glue js");
    let wasm = fs::read(&args[2]).expect("read wasm");
    let base = args.get(3).map_or_else(
        || "<!doctype html><html><body><h1>closure</h1></body></html>".to_owned(),
        |p| fs::read_to_string(p).expect("read base page"),
    );
    print!("{}", closure_wasm::inline_wasm_editor(&base, &glue, &wasm));
}
