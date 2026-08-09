//! "Split closure-shell-core: 109 fields on one struct, 732-line
//! dispatch."
//!
//! A struct with 109 fields is not a design, it is a drawer. Every one
//! of them is in scope in every method, so nothing is private to the
//! part that owns it and any two of them can quietly get out of step —
//! which is exactly how this codebase's recurring bug shape (one fact,
//! two owners) keeps arriving.
//!
//! This is the ratchet rather than the fix: it counts, and it fails if
//! the number goes back up. The fields come out in clusters, and each
//! cluster is its own commit.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

/// How many fields `ModalApp` declares.
fn field_count() -> (usize, Vec<String>) {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"),
    )
    .unwrap();
    let start = src.find("pub struct ModalApp {").expect("the struct");
    let body = &src[start..];
    let end = body.find("\n}").expect("its end");
    let mut fields = Vec::new();
    for line in body[..end].lines() {
        // `    name: Type,` — a field, not a doc comment or an
        // attribute or a nested generic's line.
        let Some(name) = line.strip_prefix("    ") else {
            continue;
        };
        if name.starts_with("//") || name.starts_with('#') || name.starts_with(' ') {
            continue;
        }
        if let Some((name, _)) = name.split_once(':')
            && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            && !name.is_empty()
        {
            fields.push(name.to_owned());
        }
    }
    (fields.len(), fields)
}

/// The ceiling, lowered by each cluster that comes out.
///
/// The review counted 109; by the time this test was written it was
/// 118, which is the argument for having the test. The memo cluster
/// took it to 108, the capture bar's to 104, the `:` line's to 101, and the two
/// prompt pairs to 99.
/// Every cluster after them
/// lowers this line, and nothing is allowed to raise it — a field that
/// has nowhere to live but here is a cluster nobody has named yet.
const CEILING: usize = 99;

#[test]
fn the_drawer_does_not_refill() {
    let (n, fields) = field_count();
    assert!(
        n <= CEILING,
        "ModalApp has {n} fields, ceiling is {CEILING}. \
         New state belongs in a cluster of its own, not in the drawer:\n{}",
        fields.join(", ")
    );
}

#[test]
fn the_two_prompt_pairs_are_one_field_each() {
    // Small on purpose. `link_kind`/`link_dest` have an invariant
    // between them (a destination means nothing before a kind is
    // picked) and so do `field_target`/`field_buf` (text with nowhere
    // to go, or a headline nobody is editing). Two pairs, not one
    // cluster: `link_target` next to them is the Backlinks surface's
    // subject and has nothing to do with either.
    let (_, fields) = field_count();
    for gone in ["link_kind", "link_dest", "field_target", "field_buf"] {
        assert!(
            !fields.iter().any(|f| f == gone),
            "`{gone}` is still a field of its own"
        );
    }
    assert!(fields.iter().any(|f| f == "pending_link"), "{fields:?}");
    assert!(fields.iter().any(|f| f == "field"), "{fields:?}");
}

#[test]
fn the_ex_line_cluster_is_one_field() {
    let (_, fields) = field_count();
    for gone in ["ex_buf", "ex_return", "ex_cycle", "ex_stem"] {
        assert!(
            !fields.iter().any(|f| f == gone),
            "`{gone}` is still a field of its own"
        );
    }
    assert!(fields.iter().any(|f| f == "ex"), "{fields:?}");
}

#[test]
fn the_capture_cluster_is_one_field() {
    let (_, fields) = field_count();
    for gone in [
        "capture_buf",
        "capture_history",
        "capture_hist_at",
        "capture_crumb_pick",
        "capture_path_root",
    ] {
        assert!(
            !fields.iter().any(|f| f == gone),
            "`{gone}` is still a field of its own"
        );
    }
    assert!(fields.iter().any(|f| f == "capture"), "{fields:?}");
}

#[test]
fn the_memo_cluster_is_one_field() {
    // The first cluster out: eleven fields that were one idea —
    // "what did we compute last time, and how often did we have to".
    let (_, fields) = field_count();
    for gone in [
        "row_memo",
        "row_recomputes",
        "detail_memo",
        "detail_recomputes",
        "palette_memo",
        "palette_recomputes",
        "block_memo",
        "block_recomputes",
        "git_memo",
        "git_reads",
        "fringe_memo",
    ] {
        assert!(
            !fields.iter().any(|f| f == gone),
            "`{gone}` is still a field of its own"
        );
    }
    assert!(fields.iter().any(|f| f == "memos"), "{fields:?}");
}
