//! Kernel document model: stable block IDs, command registry, keybinding trie,
//! event bus. Sits on top of [`closure_org`] and is UI-agnostic. Implementation
//! lands in M2.

#![forbid(unsafe_code)]
