//! Golden-corpus roundtrip test — the single most important guarantee of this
//! crate (spec invariant I1). For every `*.org` file under `fixtures/org/`,
//! `print(parse(s)?) == s` byte-for-byte.
//!
//! Adding a file to `fixtures/org/` automatically expands the corpus checked
//! here; no test code change is needed.

use std::{fs, path::PathBuf};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| panic!("workspace root above crates/closure-org"))
        .join("fixtures")
        .join("org")
}

fn org_files() -> Vec<PathBuf> {
    let dir = corpus_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("org"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no *.org fixtures found under {}",
        dir.display()
    );
    files
}

#[test]
fn roundtrip_exact_on_golden_corpus() {
    let mut failures: Vec<String> = Vec::new();
    for path in org_files() {
        let src =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let parsed = match closure_org::parse(&src) {
            Ok(d) => d,
            Err(e) => {
                failures.push(format!("{}: parse error: {e}", path.display()));
                continue;
            }
        };
        let printed = closure_org::print(&parsed);
        if printed != src {
            failures.push(format!(
                "{}: roundtrip mismatch\n  expected ({} bytes): {:?}\n  got      ({} bytes): {:?}",
                path.display(),
                src.len(),
                src,
                printed.len(),
                printed,
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} file(s) failed roundtrip:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
