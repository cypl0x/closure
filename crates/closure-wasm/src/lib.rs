//! In-browser kernel surface (X2).
//!
//! The HTML export currently ships a read-only snapshot; this crate is
//! the path to client-side *parse/edit*. The pure functions
//! ([`reformat`], [`headline_titles`]) run the closure-core parser/
//! printer over a string and are unit-tested natively in the default
//! workspace; the `#[wasm_bindgen]` wrappers (behind the opt-in `wasm`
//! feature) expose them to JavaScript when built for
//! `wasm32-unknown-unknown`. The default build never pulls wasm-bindgen
//! (I10).

#![forbid(unsafe_code)]

use closure_core::Document;

/// Parse `org` and re-serialise it through the kernel — the round-trip
/// the browser runs to validate + normalise an edit. Byte-exact for
/// valid org (I1).
///
/// # Errors
///
/// A message string if the source does not parse.
pub fn reformat(org: &str) -> Result<String, String> {
    Document::load_str(org)
        .map(|d| d.source())
        .map_err(|e| e.to_string())
}

/// The headline titles of `org`, in document order. Empty when the
/// source does not parse (the browser shows nothing rather than erroring
/// on a half-typed edit).
#[must_use]
pub fn headline_titles(org: &str) -> Vec<String> {
    Document::load_str(org)
        .map(|doc| doc.all_headlines().map(|h| h.title().to_owned()).collect())
        .unwrap_or_default()
}

/// `\n`-joined headline titles — a JS-friendly scalar return for the
/// wasm boundary (avoids marshalling a `Vec<String>`).
#[must_use]
pub fn headline_titles_joined(org: &str) -> String {
    headline_titles(org).join("\n")
}

/// Q6-W4: apply one registry command to org *source*.
///
/// Returns the rewritten source — offline editing for the single-HTML
/// export. The same command vocabulary as the served `/command`
/// endpoint, routed through kernel `Command`s (I8); ids survive
/// verbatim (I2).
///
/// # Errors
///
/// A message string for an unknown command, unknown id, or unparsable
/// source — never a panic (I5).
pub fn dispatch_command(org: &str, cmd: &str, id: &str, arg: &str) -> Result<String, String> {
    use closure_core::{
        AddSibling, BlockId, Command as _, Demote, Promote, RemoveSubtree, RenameHeadline, SetBody,
        SetTodo,
    };
    let mut doc = Document::load_str(org).map_err(|e| e.to_string())?;
    let bid = BlockId::from_existing(id);
    let result = match cmd {
        "rename" => RenameHeadline::new(bid, arg.to_owned()).apply(&mut doc),
        "set-todo" => SetTodo::new(
            bid,
            if arg.is_empty() {
                None
            } else {
                Some(arg.to_owned())
            },
        )
        .apply(&mut doc),
        "set-body" => SetBody::new(bid, arg.to_owned()).apply(&mut doc),
        "add-sibling" => AddSibling::new(
            bid,
            if arg.is_empty() {
                "untitled".to_owned()
            } else {
                arg.to_owned()
            },
        )
        .apply(&mut doc),
        "remove-subtree" | "delete" => RemoveSubtree::new(bid).apply(&mut doc),
        "promote" => Promote::new(bid).apply(&mut doc),
        "demote" => Demote::new(bid).apply(&mut doc),
        _ => return Err(format!("unknown command: {cmd}")),
    };
    result.map_err(|e| e.to_string())?;
    Ok(doc.source())
}

/// Standard base64 alphabet.
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64-encode bytes (no line breaks) — for inlining the wasm module
/// into a single HTML file. Hand-rolled to avoid a dependency.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64[(n >> 18 & 63) as usize] as char);
        out.push(B64[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Assemble a single self-contained HTML editor (X2b).
///
/// Combines the read-only export `base_page`, the wasm-bindgen `glue_js`,
/// the base64-inlined `wasm` module, and a harness that re-parses edits
/// in the browser via the exported [`reformat`] / [`headline_titles`].
/// No server, no network.
///
/// Pure string assembly — the produced page is verifiable without a
/// browser; the actual in-browser execution is display-bound.
#[must_use]
pub fn inline_wasm_editor(base_page: &str, glue_js: &str, wasm: &[u8]) -> String {
    let wasm_b64 = base64(wasm);
    format!(
        "{base_page}\n\
         <hr><h2>edit (client-side, offline)</h2>\n\
         <textarea id=\"org\" rows=\"12\" cols=\"80\"></textarea>\n\
         <pre id=\"titles\"></pre>\n\
         <script type=\"module\">\n\
         {glue_js}\n\
         const wasmB64 = \"{wasm_b64}\";\n\
         const bytes = Uint8Array.from(atob(wasmB64), c => c.charCodeAt(0));\n\
         await init(bytes);\n\
         const ta = document.getElementById('org');\n\
         const out = document.getElementById('titles');\n\
         ta.addEventListener('input', () => {{\n\
         \x20 try {{ ta.value = reformat(ta.value); out.textContent = headline_titles(ta.value); }}\n\
         \x20 catch (e) {{ out.textContent = String(e); }}\n\
         }});\n\
         </script>\n"
    )
}

/// wasm-bindgen JS exports (X2b inlines these into the HTML export).
#[cfg(feature = "wasm")]
mod js {
    use wasm_bindgen::prelude::wasm_bindgen;

    /// Round-trip + normalise org text; throws (via `Result`) on a parse
    /// error.
    #[wasm_bindgen]
    pub fn reformat(org: &str) -> Result<String, String> {
        super::reformat(org)
    }

    /// `\n`-joined headline titles of `org`.
    #[wasm_bindgen]
    #[must_use]
    pub fn headline_titles(org: &str) -> String {
        super::headline_titles_joined(org)
    }
}
