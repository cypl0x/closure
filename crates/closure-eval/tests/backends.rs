//! Backend registry + cache tests. Hermetic: only the shell backend
//! is actually executed (/bin/sh); python/node/ruby are checked for
//! identity + language only (no interpreter required).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_eval::{EvalCache, ShellBackend, backend_for, known_languages};

#[test]
fn backend_for_maps_aliases_to_languages() {
    for (alias, lang) in [
        ("sh", "shell"),
        ("bash", "shell"),
        ("shell", "shell"),
        ("py", "python"),
        ("python", "python"),
        ("js", "javascript"),
        ("node", "javascript"),
        ("rb", "ruby"),
        ("ruby", "ruby"),
    ] {
        let b = backend_for(alias).unwrap_or_else(|| panic!("no backend for {alias}"));
        assert_eq!(b.language(), lang, "{alias} -> {lang}");
    }
}

#[test]
fn backend_for_is_case_insensitive() {
    assert!(backend_for("PYTHON").is_some());
    assert!(backend_for("Sh").is_some());
}

#[test]
fn backend_for_unknown_is_none() {
    assert!(backend_for("brainfuck").is_none());
    assert!(backend_for("").is_none());
}

#[test]
fn known_languages_all_resolve() {
    for lang in known_languages() {
        assert!(backend_for(lang).is_some(), "{lang} should resolve");
    }
}

#[test]
fn cache_starts_empty() {
    let c = EvalCache::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn cache_stores_and_reuses_shell_results() {
    let mut c = EvalCache::new();
    let out1 = c.eval_cached(&ShellBackend, "echo hi").expect("eval");
    assert_eq!(out1.stdout.trim(), "hi");
    assert_eq!(c.len(), 1);
    // Same src -> served from cache, no growth.
    let out2 = c.eval_cached(&ShellBackend, "echo hi").expect("cached");
    assert_eq!(out2.stdout.trim(), "hi");
    assert_eq!(c.len(), 1);
    // Different src -> new entry.
    let _ = c.eval_cached(&ShellBackend, "echo bye").expect("eval");
    assert_eq!(c.len(), 2);
    assert!(!c.is_empty());
}

#[test]
fn cache_reflects_exit_code() {
    let mut c = EvalCache::new();
    let out = c.eval_cached(&ShellBackend, "exit 2").expect("eval");
    assert_eq!(out.exit, 2);
}
