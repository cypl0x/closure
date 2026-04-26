#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_shell_egui::{HeadlessAdapter, Shell, ShellAdapter};
use closure_store::Vault;

#[test]
fn headless_counts_frames_and_records_chords() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.org"), "* A\n").unwrap();
    let v = Vault::open(dir.path()).unwrap();
    let mut shell = Shell::new(v);
    let mut adapter = HeadlessAdapter::default();
    adapter.frame(&shell);
    adapter.frame(&shell);
    adapter.input(&mut shell, "C-x C-s");
    assert_eq!(adapter.frames, 2);
    assert_eq!(adapter.last_chord.as_deref(), Some("C-x C-s"));
}
