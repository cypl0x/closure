//! Golden conflict corpus: every case under `fixtures/conflicts/`
//! holds base/ours/theirs/merged org files; merging ours+theirs
//! against base and reconciling into the base document must
//! reproduce `merged.org` byte-exactly (I1 + I2).

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;
use std::path::PathBuf;

use closure_core::Document;
use closure_crdt::Replica;

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/conflicts")
}

#[test]
fn corpus_is_not_empty() {
    let cases: Vec<_> = fs::read_dir(corpus_dir()).expect("corpus dir").collect();
    assert!(!cases.is_empty(), "conflict corpus must have cases");
}

#[test]
fn every_conflict_case_merges_to_golden_output() {
    for entry in fs::read_dir(corpus_dir()).expect("corpus dir") {
        let case = entry.expect("entry").path();
        if !case.is_dir() {
            continue;
        }
        let name = case.file_name().unwrap().to_string_lossy().to_string();
        let read = |f: &str| fs::read_to_string(case.join(f)).expect("fixture file");
        let base_src = read("base.org");
        let ours_src = read("ours.org");
        let theirs_src = read("theirs.org");
        let golden = read("merged.org");

        let mut base = Document::load_str(&base_src).expect("base parses");
        let ours = Document::load_str(&ours_src).expect("ours parses");
        let theirs = Document::load_str(&theirs_src).expect("theirs parses");

        let r_base = Replica::snapshot(&base, 1, "base");
        let r_ours = Replica::snapshot_against(&r_base, &ours, 2, "ours");
        let r_theirs = Replica::snapshot_against(&r_base, &theirs, 3, "theirs");
        let mut merged = r_base;
        merged.merge(&r_ours);
        merged.merge(&r_theirs);
        merged.apply_to(&mut base).expect("apply");

        assert_eq!(base.source(), golden, "case `{name}` diverges from golden");

        // The merged output itself must roundtrip byte-exact (I1).
        let reparsed = Document::load_str(&golden).expect("golden parses");
        assert_eq!(reparsed.source(), golden, "case `{name}` golden roundtrip");
    }
}

#[test]
fn merge_order_does_not_change_golden_output() {
    for entry in fs::read_dir(corpus_dir()).expect("corpus dir") {
        let case = entry.expect("entry").path();
        if !case.is_dir() {
            continue;
        }
        let read = |f: &str| fs::read_to_string(case.join(f)).expect("fixture file");
        let mut base = Document::load_str(&read("base.org")).expect("parse");
        let ours = Document::load_str(&read("ours.org")).expect("parse");
        let theirs = Document::load_str(&read("theirs.org")).expect("parse");
        let r_base = Replica::snapshot(&base, 1, "base");
        let r_ours = Replica::snapshot_against(&r_base, &ours, 2, "ours");
        let r_theirs = Replica::snapshot_against(&r_base, &theirs, 3, "theirs");
        let mut other_order = r_base;
        other_order.merge(&r_theirs);
        other_order.merge(&r_ours);
        other_order.apply_to(&mut base).expect("apply");
        assert_eq!(base.source(), read("merged.org"));
    }
}
