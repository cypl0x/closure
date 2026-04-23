//! Vault-level storage for closure: directory loader, file watcher, atomic
//! writes, headline-path → block-ID index, and backlink index. Implementation
//! lands in M4.

#![forbid(unsafe_code)]
