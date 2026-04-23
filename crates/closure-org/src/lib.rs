//! Lossless Emacs org-mode parser and printer.
//!
//! Invariant I1 (byte-exact roundtrip on the golden corpus) is the core
//! guarantee of this crate: for every `s` in `fixtures/org/`,
//! `print(parse(s)?) == s` byte-for-byte.
//!
//! This stub establishes the public surface (`OrgDoc`, [`parse`], [`print`],
//! [`ParseError`]). The span-preserving recursive-descent implementation
//! lands in M1 alongside its golden-corpus and property-test harness.

#![forbid(unsafe_code)]

use thiserror::Error;

/// A parsed org document. Structured fields land in M1.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OrgDoc;

/// Parse failure. The M1 parser is total; the `Result` return keeps the
/// public signature stable as richer parsing lands.
#[derive(Debug, Error)]
pub enum ParseError {}

/// Parse an org document. M0 stub — always returns [`OrgDoc::default`].
///
/// `#[allow]` attributes cover nursery lints (`missing_const_for_fn`,
/// `must_use_candidate`) that only fire because this stub is trivial. The
/// M1 span-preserving parser allocates and returns a populated document;
/// both lints go silent then and the `#[allow]` is removed.
#[allow(clippy::missing_const_for_fn, clippy::must_use_candidate)]
pub fn parse(_src: &str) -> Result<OrgDoc, ParseError> {
    Ok(OrgDoc)
}

/// Serialise an org document back to its source text. M0 stub — always
/// returns the empty string. See [`parse`] for the `#[allow]` rationale.
#[must_use]
#[allow(clippy::missing_const_for_fn)]
pub fn print(_doc: &OrgDoc) -> String {
    String::new()
}
