//! Located defect: a peer's *new* headline never arrives.
//!
//! The merge walked every block in the replica and did this:
//!
//! ```ignore
//! let Some((headline, _)) = vault.find_by_id(&id) else { continue };
//! ```
//!
//! — so an id the local vault has never seen is skipped in silence.
//! Edits to headlines both sides already share were applied; anything
//! *created* on the peer was dropped on the floor, which is the half
//! of syncing people notice first. The status line then reported the
//! edits it did apply, so it looked like it had worked.
//!
//! An id we do not have is not an error and not a conflict: it is a
//! note somebody else wrote. It gets written into the capture file,
//! the same place a thought with no home already goes.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_shell_core::Shell;
use closure_store::Vault;
use tempfile::TempDir;

const NOTES: &str = "\
* Shared
:PROPERTIES:
:ID: 01HQSYNC00000000000001
:END:
the local body
";

fn fixture() -> (TempDir, Shell) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault))
}

/// Every headline id the vault knows.
fn ids(shell: &Shell) -> Vec<String> {
    shell
        .vault
        .iter()
        .flat_map(|(_, doc)| {
            doc.all_headlines()
                .map(|h| h.id().as_str().to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn a_headline_the_vault_already_has_is_updated() {
    // The half that worked, kept as cover.
    let (_d, mut shell) = fixture();
    let applied = shell.apply_peer_block(
        "01HQSYNC00000000000001",
        Some("Renamed by a peer"),
        Some("the peer's body\n"),
    );
    assert!(applied > 0);
    let (_, doc) = shell.vault.iter().next().expect("a document");
    assert!(
        doc.all_headlines()
            .any(|h| h.title() == "Renamed by a peer"),
        "the rename did not land"
    );
}

#[test]
fn a_headline_only_the_peer_has_is_created() {
    // The defect. This used to be skipped, silently.
    let (_d, mut shell) = fixture();
    let applied = shell.apply_peer_block(
        "01HQSYNC00000000000002",
        Some("Written on the other machine"),
        Some("its body\n"),
    );
    assert!(applied > 0, "nothing was applied");
    assert!(
        ids(&shell).iter().any(|i| i == "01HQSYNC00000000000002"),
        "the peer's headline never arrived: {:?}",
        ids(&shell)
    );
}

#[test]
fn the_created_headline_keeps_the_peers_id() {
    // The id *is* the identity: minting a fresh one would make the
    // next sync think it is a different note and create it again.
    let (_d, mut shell) = fixture();
    shell.apply_peer_block("01HQSYNC00000000000003", Some("Theirs"), Some("body\n"));
    shell.apply_peer_block("01HQSYNC00000000000003", Some("Theirs"), Some("body\n"));
    assert_eq!(
        ids(&shell)
            .iter()
            .filter(|i| *i == "01HQSYNC00000000000003")
            .count(),
        1,
        "the same peer block arrived twice as two headlines"
    );
}

#[test]
fn the_created_headline_carries_its_title_and_body() {
    let (_d, mut shell) = fixture();
    shell.apply_peer_block(
        "01HQSYNC00000000000004",
        Some("Their title"),
        Some("their body\n"),
    );
    let found = shell
        .vault
        .find_by_id(&closure_core::BlockId::from_existing(
            "01HQSYNC00000000000004",
        ))
        .expect("the new headline");
    assert_eq!(found.0.title(), "Their title");
    assert!(found.0.body_text().contains("their body"));
}

#[test]
fn a_block_with_nothing_to_say_is_not_invented() {
    // A replica entry with neither title nor body is not a note.
    let (_d, mut shell) = fixture();
    let before = ids(&shell).len();
    let applied = shell.apply_peer_block("01HQSYNC00000000000005", None, None);
    assert_eq!(applied, 0);
    assert_eq!(ids(&shell).len(), before);
}

#[test]
fn an_unchanged_block_costs_nothing() {
    // Applying what is already true must not rewrite the file: a sync
    // that dirties every note on every round is a sync that fights
    // git and the file watcher.
    let (_d, mut shell) = fixture();
    let applied = shell.apply_peer_block(
        "01HQSYNC00000000000001",
        Some("Shared"),
        Some("the local body\n"),
    );
    assert_eq!(applied, 0, "an identical block was written back");
}

// === what the assistant is told before it starts ===
//
// The tool loop used to be handed a bare task: no idea which vault, how
// big it is, or what is selected. So a model's first turn was always
// spent asking, which costs a round trip and an API call to learn
// something the process already knew.

#[test]
fn the_vault_context_names_the_vault() {
    let (dir, shell) = fixture();
    let ctx = shell.assistant_context();
    let name = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert!(ctx.contains(&name), "{ctx}");
}

#[test]
fn it_says_how_much_is_there() {
    let (_d, shell) = fixture();
    let ctx = shell.assistant_context();
    assert!(ctx.contains('1'), "one file, one headline: {ctx}");
}

#[test]
fn it_is_short_enough_to_prepend_to_every_task() {
    // Context that costs more than it saves is not context, it is a
    // tax on every prompt.
    let (_d, shell) = fixture();
    assert!(
        shell.assistant_context().len() < 400,
        "{}",
        shell.assistant_context()
    );
}

#[test]
fn it_holds_no_note_contents() {
    // The assistant reads notes through tools, which are gated. A
    // context block that quietly pasted body text in would route
    // around that gate entirely.
    let (_d, shell) = fixture();
    assert!(
        !shell.assistant_context().contains("the local body"),
        "body text leaked into the preamble: {}",
        shell.assistant_context()
    );
}
