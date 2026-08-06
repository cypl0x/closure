//! "you said there more than 17 literal `config.org` uses. Please
//! point all of them to the variable you have created for this."
//!
//! There were 35 in `src/` when this was written, and one constant
//! that two of them used. A name spelled out in thirty-five places is
//! not a name, it is thirty-five chances to disagree — and one of them
//! was already the reason a vault could hold a `config.org` that
//! nothing read.
//!
//! This is the assertion that keeps it at one: the source of every
//! crate in the workspace, checked for the literal.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

/// Every `src/**/*.rs` in the workspace.
fn sources() -> Vec<std::path::PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|x| x == "rs") {
                out.push(path);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut out = Vec::new();
    for crate_dir in std::fs::read_dir(&root).unwrap().flatten() {
        walk(&crate_dir.path().join("src"), &mut out);
    }
    out
}

#[test]
fn the_file_is_named_once() {
    let this_file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let mut offenders = Vec::new();
    for path in sources() {
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        for (i, line) in src.lines().enumerate() {
            if !line.contains("\"config.org\"") {
                continue;
            }
            // The definition itself, and prose about it.
            if path == this_file || line.trim_start().starts_with("//") {
                continue;
            }
            offenders.push(format!("{}:{}", path.display(), i + 1));
        }
    }
    assert!(
        offenders.is_empty(),
        "the config file is named literally in {} place(s) instead of \
         `closure_config::CONFIG_FILE`:\n{}",
        offenders.len(),
        offenders.join("\n")
    );
}

#[test]
fn the_constant_is_what_it_says() {
    assert_eq!(closure_config::CONFIG_FILE, "config.org");
}

#[test]
fn the_capture_file_is_named_once_too() {
    // Worse than config.org when this was written: two constants —
    // `closure_shell_core::CAPTURE_FILE` and `closure_tui::
    // CAPTURE_TARGET` — and four literals besides. One fact with two
    // owners is the bug that keeps coming back; here it would be two
    // shells filing your captures in different files.
    let mut offenders = Vec::new();
    for path in sources() {
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        for (i, line) in src.lines().enumerate() {
            if !line.contains("\"inbox.org\"") || line.trim_start().starts_with("//") {
                continue;
            }
            // The definition itself.
            if line.contains("pub const CAPTURE_FILE") {
                continue;
            }
            offenders.push(format!("{}:{}", path.display(), i + 1));
        }
    }
    assert!(
        offenders.is_empty(),
        "the capture file is named literally in {} place(s) instead of \
         `closure_store::CAPTURE_FILE`:\n{}",
        offenders.len(),
        offenders.join("\n")
    );
}
