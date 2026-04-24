//! `CommonMark` + GFM parser (Layer 1).
//!
//! The architecture mirrors [`closure_org`]: a span-preserving
//! hand-written line-cursor classifier. Byte-exact roundtrip (I1) is
//! the contract. Structured editing is added per construct as later
//! milestones reach markdown parity.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Placeholder parsed markdown document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MdDoc {
    source: String,
}

impl MdDoc {
    /// Verbatim source bytes.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Parse failure.
#[derive(Debug, Error)]
pub enum ParseError {}

/// Parse markdown. Currently keeps the source verbatim (byte-exact
/// roundtrip by construction).
#[allow(clippy::missing_errors_doc, clippy::must_use_candidate)]
pub fn parse(src: &str) -> Result<MdDoc, ParseError> {
    Ok(MdDoc {
        source: src.to_owned(),
    })
}

/// Serialise back to markdown source.
#[must_use]
pub fn print(doc: &MdDoc) -> String {
    doc.source.clone()
}
