//! Draining a task queue: every task processed exactly once.
//!
//! Rewritten 2026-08-12. The version shipped before asserted the
//! property it names with `prop_assert!(true)` after an empty `for`
//! loop, above a comment saying the cross-check was not done. The only
//! real assertion was `drained == unique.len()`, and `drained` is the
//! implementation counting its own inserts — so the test could not have
//! failed for the reason it exists.
//!
//! What "exactly once" has to mean is a claim about the *vault*: after
//! the drain, every task carries the mark, no task carries it twice,
//! and no task was missed. That is what this checks now.
//!
//! It also stops calling the thing a swarm. `swarm_drain` takes a
//! worker count it ignores — the drain is one sequential loop — and
//! `closure-a2a` is frozen, so making concurrency real is not this
//! item's business. Saying what the function does is.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::fmt::Write as _;

use closure_a2a::swarm_drain;
use closure_core::BlockId;
use closure_store::Vault;
use proptest::prelude::*;
use tempfile::TempDir;

fn make_queue_with_tasks(tasks: &[String]) -> (TempDir, Vault, Vec<BlockId>) {
    let dir = tempfile::tempdir().expect("tmp");
    let mut content = String::new();
    let mut ids = vec![];
    for t in tasks {
        let id = BlockId::fresh();
        ids.push(id.clone());
        let _ = write!(content, "* TODO {t}\n:PROPERTIES:\n:ID: {id}\n:END:\n\n");
    }
    std::fs::write(dir.path().join("queue.org"), &content).expect("write queue");
    let v = Vault::open(dir.path()).expect("open vault with queue");
    (dir, v, ids)
}

/// How many headlines in the vault carry `:SWARM-STATE: done` for `id`.
///
/// The mark the drain leaves. Counted rather than tested for presence,
/// because "exactly once" is a statement about the count.
fn marks_for(vault: &Vault, id: &BlockId) -> usize {
    vault
        .iter()
        .flat_map(|(_, doc)| doc.all_headlines())
        .filter(|h| h.id() == id)
        .filter(|h| {
            h.properties()
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("swarm-state") && v == "done")
        })
        .count()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn every_task_is_marked_exactly_once(tasks in prop::collection::vec("[a-z]{1,16}", 0..24)) {
        let unique: Vec<String> = tasks.into_iter().collect::<HashSet<_>>().into_iter().collect();
        if unique.is_empty() { return Ok(()); }

        let (_td, mut v, ids) = make_queue_with_tasks(&unique);
        let drained = swarm_drain(&mut v, 3, &ids);
        prop_assert_eq!(drained, unique.len(), "all tasks must be drained");

        // The part the old test skipped: ask the vault, not the counter.
        for id in &ids {
            prop_assert_eq!(
                marks_for(&v, id),
                1,
                "task {} is not marked exactly once",
                id
            );
        }
    }
}

#[test]
fn a_task_offered_twice_is_still_done_once() {
    // The claim set is the whole point of the function. A queue that
    // lists the same id twice — two producers, one task — must not be
    // processed twice, and the count must not double.
    let tasks = vec!["t1".to_owned(), "t2".to_owned()];
    let (_td, mut v, ids) = make_queue_with_tasks(&tasks);
    let doubled: Vec<BlockId> = ids.iter().chain(ids.iter()).cloned().collect();
    let drained = swarm_drain(&mut v, 2, &doubled);
    assert_eq!(drained, 2, "a repeated id was drained twice");
    for id in &ids {
        assert_eq!(marks_for(&v, id), 1, "{id} marked more than once");
    }
}

#[test]
fn the_worker_count_is_not_used() {
    // Stated as a test rather than left in a doc comment, because the
    // signature says otherwise and a reader is entitled to know. One
    // worker and a hundred produce identical vaults; when the drain
    // becomes concurrent this test is the one that has to change, and
    // it will be obvious why.
    let tasks: Vec<String> = (0..6).map(|i| format!("t{i}")).collect();
    let (_a, mut one, ids_a) = make_queue_with_tasks(&tasks);
    let (_b, mut many, ids_b) = make_queue_with_tasks(&tasks);
    assert_eq!(swarm_drain(&mut one, 1, &ids_a), 6);
    assert_eq!(swarm_drain(&mut many, 100, &ids_b), 6);
    for (a, b) in ids_a.iter().zip(ids_b.iter()) {
        assert_eq!(marks_for(&one, a), marks_for(&many, b));
    }
}
