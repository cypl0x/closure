//! Q4-M3: markdown backlink queries (path/slug identity).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

// === Q4-M3: markdown backlinks — path/slug identity, read-only. ===

#[test]
fn md_backlinks_find_files_linking_to_a_target() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::write(
        dir.path().join("a.md"),
        "points at [notes](notes.md) here\n",
    )
    .expect("a");
    std::fs::write(dir.path().join("b.md"), "wiki link [[notes]] too\n").expect("b");
    std::fs::write(dir.path().join("c.md"), "unrelated [x](other.md)\n").expect("c");
    let mut hits = closure_query::md_backlinks(dir.path(), "notes.md");
    hits.sort();
    let names: Vec<String> = hits
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    assert_eq!(
        names,
        vec!["a.md".to_owned(), "b.md".to_owned()],
        "slug and path both match"
    );
}

#[test]
fn md_backlinks_on_a_missing_dir_is_empty_never_a_panic() {
    assert!(
        closure_query::md_backlinks(std::path::Path::new("/nonexistent-xyz"), "n.md").is_empty()
    );
}
