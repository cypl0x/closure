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
