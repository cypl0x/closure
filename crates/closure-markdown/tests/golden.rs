//! Golden roundtrip corpus for closure-markdown (I1 for markdown).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/md")
}

#[test]
fn every_md_fixture_roundtrips_byte_exact() {
    let dir = corpus();
    let mut count = 0;
    for entry in fs::read_dir(&dir).expect("md corpus dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("read");
        let doc = closure_markdown::parse(&src).expect("parse");
        assert_eq!(
            closure_markdown::print(&doc),
            src,
            "fixture {} does not roundtrip",
            path.display()
        );
        count += 1;
    }
    assert!(count > 0, "md corpus must have fixtures");
}
