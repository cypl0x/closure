//! Branching undo-tree. Edits form a DAG keyed by [`closure_core`] block IDs
//! and are persisted per-vault. Implementation lands in M3.

#![forbid(unsafe_code)]
