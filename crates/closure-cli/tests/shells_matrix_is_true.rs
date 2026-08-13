//! The capability matrix is a claim, so it needs a test.
//!
//! `closure shells` prints it and calls itself "the single source of
//! truth", above a line reading "Every shell should be a superset of
//! CORE (I7)". The only test behind any of it asserted that three rows
//! contain `Browse`, and admitted as much in its own comment: "for now
//! the presence + browse satisfies the matrix row".
//!
//! A published claim with nothing holding it is the shape the org
//! conformance matrix had before it grew a second number — and the
//! shape that let seven org constructs count as supported with no
//! caller anywhere.
//!
//! Tested through the printed table rather than the constants, because
//! `closure-cli` is a binary with no lib target and, more to the point,
//! the table is what a reader actually sees. Same reason "offered"
//! means a shell shows it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_closure");

/// Where each column's implementation lives. A column with no file is a
/// column claiming capabilities that nothing provides.
const IMPLEMENTATIONS: &[(&str, &str)] = &[
    ("TUI", "crates/closure-tui/src/lib.rs"),
    ("CLI", "crates/closure-cli/src/main.rs"),
    ("WEB", "crates/closure-shell-web/src/lib.rs"),
    ("GTK", "crates/closure-shell-gtk/src/lib.rs"),
    ("QT", "crates/closure-shell-qt/src/lib.rs"),
    ("EGUI", "crates/closure-shell-egui/src/lib.rs"),
    ("TAURI", "crates/closure-shell-tauri/src/lib.rs"),
    ("GPUI", "crates/closure-shell-gpui/src/lib.rs"),
    ("FLUTTER", "flutter/lib/main.dart"),
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repo root")
}

/// `{shell: {capability: claimed}}`, read off the printed table.
fn matrix() -> BTreeMap<String, BTreeMap<String, bool>> {
    let out = Command::new(BIN).arg("shells").output().expect("spawn");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut lines = text.lines().skip_while(|l| !l.starts_with("Capability"));
    let header = lines.next().expect("a header row");
    let shells: Vec<String> = header
        .split('|')
        .skip(1)
        .map(|s| s.trim().to_owned())
        .collect();
    let mut out_map: BTreeMap<String, BTreeMap<String, bool>> = BTreeMap::new();
    for line in lines.skip(1) {
        let mut cells = line.split('|');
        let Some(cap) = cells.next().map(str::trim) else {
            continue;
        };
        if cap.is_empty() {
            continue;
        }
        for (i, cell) in cells.enumerate() {
            let Some(shell) = shells.get(i) else { continue };
            out_map
                .entry(shell.clone())
                .or_default()
                .insert(cap.to_owned(), cell.trim() == "X");
        }
    }
    out_map
}

#[test]
fn the_table_parses_at_all() {
    let m = matrix();
    assert!(
        m.len() >= 9,
        "only {} columns parsed: {:?}",
        m.len(),
        m.keys()
    );
    assert!(
        m.get("GPUI")
            .is_some_and(|c| c.get("Browse") == Some(&true)),
        "the parse is wrong, not the matrix"
    );
}

#[test]
fn every_column_names_an_implementation_that_exists() {
    // The sharpest one: FLUTTER claims Browse, Capture and Search and
    // there is no Flutter shell. A matrix describing a shell that does
    // not exist is worse than one omitting it — somebody reads it and
    // picks closure for the row.
    let m = matrix();
    let missing: Vec<&str> = IMPLEMENTATIONS
        .iter()
        .filter(|(name, path)| {
            let claims_anything = m.get(*name).is_some_and(|c| c.values().any(|v| *v));
            claims_anything && !root().join(path).exists()
        })
        .map(|(name, _)| *name)
        .collect();
    assert!(
        missing.is_empty(),
        "these columns claim capabilities and have no implementation: {missing:?}"
    );
}

#[test]
fn every_shell_is_a_superset_of_core() {
    // The line printed directly above the table. Advice until now.
    let m = matrix();
    let core = m.get("CORE").expect("a CORE column").clone();
    let mut broken: Vec<String> = Vec::new();
    for (shell, caps) in &m {
        if shell == "CORE" {
            continue;
        }
        // A column with no implementation is not a shell yet, so it is
        // not held to the floor — `every_column_names_an_implementation`
        // is what stops it claiming things instead. The two together
        // mean a row either has code and meets the floor, or claims
        // nothing at all.
        let built = IMPLEMENTATIONS
            .iter()
            .find(|(name, _)| name == shell)
            .is_some_and(|(_, path)| root().join(path).exists());
        if !built {
            continue;
        }
        let short: Vec<&str> = core
            .iter()
            .filter(|(cap, in_core)| **in_core && caps.get(*cap) != Some(&true))
            .map(|(cap, _)| cap.as_str())
            .collect();
        if !short.is_empty() {
            broken.push(format!("{shell} missing {short:?}"));
        }
    }
    assert!(
        broken.is_empty(),
        "the table promises every shell is a superset of CORE: {broken:?}"
    );
}

#[test]
fn a_shell_that_claims_edit_also_claims_browse() {
    // You cannot edit what you cannot see, and the matrix is
    // hand-maintained, so this is the transcription error to catch.
    let m = matrix();
    for (shell, caps) in &m {
        if caps.get("Edit") == Some(&true) {
            assert_eq!(
                caps.get("Browse"),
                Some(&true),
                "`{shell}` claims Edit without Browse"
            );
        }
    }
}
