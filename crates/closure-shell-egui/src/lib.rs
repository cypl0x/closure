//! Native desktop shell built on egui + eframe.
//!
//! I7: consumes [`closure_core`] / [`closure_store`] only. No direct
//! `closure_org` access. Spans never cross the shell boundary.
//!
//! The crate is a placeholder; eframe wiring salvages the prior
//! walking-skeleton in a later milestone.

#![forbid(unsafe_code)]

/// Shell handle.
#[derive(Debug, Default)]
pub struct Shell;
