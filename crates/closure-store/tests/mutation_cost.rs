//! The microfreeze: a keystroke that costs the whole vault.
//!
//! Reported as "folding a level-1 headline stutters", and it is not
//! about folding. Every kernel mutation funnels through
//! `apply_to_block`, which opened with `refresh_all_stale()` — read
//! and hash *every file in the vault* — before touching the one
//! headline being changed. Measured on a generated vault: a fold costs
//! 1.8ms over 4 files, 14ms over 12, 25ms over 48. It scales with how
//! much you have written, which is the one thing a note-taker only
//! ever adds to.
//!
//! The read exists for a real reason (a whole-file write putting our
//! older copy back over an external change, which destroyed a capture
//! on 2026-08-02) — but that race is about *the file being written*.
//! Re-reading the other forty-seven protects nothing about this write.
//!
//! Budgets are in files read, not milliseconds: the same number on
//! every machine.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_core::BlockId;
use closure_store::Vault;

/// A vault of `n` files, two headlines each, with known ids.
fn vault_of(dir: &std::path::Path, n: usize) -> Vault {
    for f in 0..n {
        let src = format!(
            "* One in {f}\n:PROPERTIES:\n:ID: id-{f}-a\n:END:\nbody\n\
             * Two in {f}\n:PROPERTIES:\n:ID: id-{f}-b\n:END:\nbody\n"
        );
        std::fs::write(dir.join(format!("n{f:03}.org")), src).unwrap();
    }
    Vault::open(dir).unwrap()
}

#[test]
fn a_mutation_reads_only_the_file_it_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut vault = vault_of(tmp.path(), 40);
    let before = vault.disk_reads();
    vault
        .set_property(&BlockId::from_existing("id-7-a"), "VISIBILITY", "folded")
        .unwrap();
    let read = vault.disk_reads() - before;
    assert_eq!(
        read, 1,
        "one headline changed in one file, and {read} files were read"
    );
}

#[test]
fn the_cost_of_a_mutation_does_not_grow_with_the_vault() {
    // The shape of the complaint, stated directly: the same edit in
    // the same file must not get slower because there are more files
    // next to it.
    let small = tempfile::tempdir().unwrap();
    let large = tempfile::tempdir().unwrap();
    let mut a = vault_of(small.path(), 2);
    let mut b = vault_of(large.path(), 80);

    let (sa, sb) = (a.disk_reads(), b.disk_reads());
    a.set_property(&BlockId::from_existing("id-1-a"), "VISIBILITY", "folded")
        .unwrap();
    b.set_property(&BlockId::from_existing("id-1-a"), "VISIBILITY", "folded")
        .unwrap();
    assert_eq!(
        a.disk_reads() - sa,
        b.disk_reads() - sb,
        "a 80-file vault paid more than a 2-file vault for the same edit"
    );
}

#[test]
fn a_mutation_still_lands_on_top_of_an_external_change() {
    // The reason the read is there at all, and it has to survive:
    // change a file outside the app, then mutate a *different*
    // headline in that same file inside it. The write must not put
    // our older copy back.
    let tmp = tempfile::tempdir().unwrap();
    let mut vault = vault_of(tmp.path(), 6);
    let file = tmp.path().join("n003.org");

    std::fs::write(
        &file,
        "* One in 3\n:PROPERTIES:\n:ID: id-3-a\n:END:\nbody\n\
         * Two in 3\n:PROPERTIES:\n:ID: id-3-b\n:END:\nbody\n\
         * Written by Emacs\n:PROPERTIES:\n:ID: id-3-c\n:END:\nnew\n",
    )
    .unwrap();

    vault
        .set_property(&BlockId::from_existing("id-3-a"), "VISIBILITY", "folded")
        .unwrap();

    let on_disk = std::fs::read_to_string(&file).unwrap();
    assert!(
        on_disk.contains("Written by Emacs"),
        "the external headline was overwritten:\n{on_disk}"
    );
    assert!(on_disk.contains(":VISIBILITY: folded"), "{on_disk}");
}

#[test]
fn a_headline_moved_to_another_file_is_still_found() {
    // The one case the whole-vault read genuinely covered: the id
    // index says one file and the headline is now in another, because
    // someone moved it with an editor. Narrowing the refresh must not
    // turn that into "unknown id" — it falls back to the full sweep,
    // which is the rare path and is allowed to cost what it costs.
    let tmp = tempfile::tempdir().unwrap();
    let mut vault = vault_of(tmp.path(), 5);

    std::fs::write(
        tmp.path().join("n002.org"),
        "* One in 2\n:PROPERTIES:\n:ID: id-2-a\n:END:\nbody\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("n004.org"),
        "* One in 4\n:PROPERTIES:\n:ID: id-4-a\n:END:\nbody\n\
         * Two in 4\n:PROPERTIES:\n:ID: id-4-b\n:END:\nbody\n\
         * Two in 2\n:PROPERTIES:\n:ID: id-2-b\n:END:\nmoved here\n",
    )
    .unwrap();

    vault
        .set_property(&BlockId::from_existing("id-2-b"), "VISIBILITY", "folded")
        .expect("the moved headline is still reachable");

    let moved = std::fs::read_to_string(tmp.path().join("n004.org")).unwrap();
    assert!(moved.contains(":VISIBILITY: folded"), "{moved}");
}

#[test]
fn no_interactive_mutation_reads_the_whole_vault() {
    // `apply_to_block` was one of twelve entry points that opened with
    // the whole-vault sweep. Fixing the one the trace happened to
    // catch would have left capture, refile, archive, undo, redo and
    // eval each stuttering on their own, and "why is it slow" would
    // have come back per command. The budget is a property of writing
    // a file, so it is asserted over the whole set at once.
    //
    // The count itself is not the point — refile honestly touches two
    // files — so each operation runs over a small vault and a large one
    // and the two must agree. That is the property: what a command
    // costs is what it writes, not what sits next to it.
    let small = tempfile::tempdir().unwrap();
    let large = tempfile::tempdir().unwrap();
    let mut four = vault_of(small.path(), 4);
    let mut eighty = vault_of(large.path(), 80);

    let check = |label: &str, small: u64, large: u64| {
        assert_eq!(
            small, large,
            "{label} read {small} files over 4 and {large} over 80"
        );
    };

    let mut both = |op: &mut dyn FnMut(&mut Vault)| -> (u64, u64) {
        let (before_four, before_eighty) = (four.disk_reads(), eighty.disk_reads());
        op(&mut four);
        op(&mut eighty);
        (
            four.disk_reads() - before_four,
            eighty.disk_reads() - before_eighty,
        )
    };

    let (small_reads, large_reads) = both(&mut |v| {
        v.capture(
            &closure_store::CaptureTemplate {
                target: "inbox.org".into(),
                headline_prefix: "TODO ".into(),
                body: String::new(),
            },
            "a new thing",
        )
        .unwrap();
    });
    check("capture", small_reads, large_reads);

    let (small_reads, large_reads) = both(&mut |v| {
        let p = v.root().join("n002.org");
        v.set_source(&p, "* Rewritten\n").unwrap();
    });
    check("set_source", small_reads, large_reads);

    let (small_reads, large_reads) = both(&mut |v| {
        let p = v.root().join("n002.org");
        v.undo_in(&p).ok();
    });
    check("undo_in", small_reads, large_reads);

    let (small_reads, large_reads) = both(&mut |v| {
        let p = v.root().join("n002.org");
        v.redo_in(&p).ok();
    });
    check("redo_in", small_reads, large_reads);

    let (small_reads, large_reads) = both(&mut |v| {
        v.create_file(std::path::Path::new("fresh.org"), "* New\n")
            .unwrap();
    });
    check("create_file", small_reads, large_reads);

    let (small_reads, large_reads) = both(&mut |v| {
        v.refile(
            &BlockId::from_existing("id-3-b"),
            &BlockId::from_existing("id-1-a"),
        )
        .unwrap();
    });
    check("refile", small_reads, large_reads);

    let (small_reads, large_reads) = both(&mut |v| {
        v.archive_subtree(&BlockId::from_existing("id-0-a"), "2026-08-03")
            .unwrap();
    });
    check("archive_subtree", small_reads, large_reads);
}

#[test]
fn an_unknown_id_is_still_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let mut vault = vault_of(tmp.path(), 3);
    assert!(
        vault
            .set_property(&BlockId::from_existing("no-such-id"), "K", "v")
            .is_err(),
        "a missing id has to stay an error, fallback sweep or not"
    );
}
