//! Every command the wasm bridge offers, applied to a real document.
//!
//! `dispatch_command` is the whole editing surface of the browser
//! shell: seven commands in one match, each constructing a kernel
//! `Command` and applying it. Only two arms had ever been exercised.
//!
//! The arms are not interchangeable. Each builds a different type with
//! different arguments, and the two that translate an empty argument
//! into something else — `set-todo` clearing the keyword, `add-sibling`
//! defaulting to "untitled" — are places where a wrong branch produces
//! a document rather than an error. That is the failure worth catching
//! here: the bridge cannot panic (I5), so everything it gets wrong, it
//! gets wrong quietly and hands back as org.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_wasm::{dispatch_command, headline_titles, headline_titles_joined, reformat};

const ID: &str = "01WASMCMD00000000001";
const CHILD: &str = "01WASMCMD00000000002";

fn doc() -> String {
    format!(
        "* TODO Parent headline\n\
         :PROPERTIES:\n:ID: {ID}\n:END:\n\
         the parent body\n\
         ** Child headline\n\
         :PROPERTIES:\n:ID: {CHILD}\n:END:\n\
         the child body\n"
    )
}

#[test]
fn rename_changes_the_title_and_keeps_the_rest() {
    let out = dispatch_command(&doc(), "rename", ID, "A new title").expect("rename");
    assert!(out.contains("A new title"), "{out}");
    assert!(!out.contains("Parent headline"), "{out}");
    // The body and the child are not the rename's business.
    assert!(out.contains("the parent body"), "{out}");
    assert!(out.contains("Child headline"), "{out}");
}

#[test]
fn set_todo_sets_the_keyword_and_an_empty_argument_clears_it() {
    let set = dispatch_command(&doc(), "set-todo", ID, "DONE").expect("set");
    assert!(set.contains("* DONE Parent headline"), "{set}");

    // The empty-argument branch: `None` rather than a keyword called
    // "". A shell sending an empty string means "no keyword", and the
    // other reading would write `* Parent` with a stray space or a
    // literal empty TODO.
    let cleared = dispatch_command(&set, "set-todo", ID, "").expect("clear");
    assert!(!cleared.contains("DONE"), "{cleared}");
    assert!(cleared.contains("Parent headline"), "{cleared}");
}

#[test]
fn set_body_replaces_the_body_only() {
    let out = dispatch_command(&doc(), "set-body", ID, "a replaced body").expect("set-body");
    assert!(out.contains("a replaced body"), "{out}");
    assert!(!out.contains("the parent body"), "{out}");
    assert!(out.contains("Parent headline"), "{out}");
    assert!(
        out.contains("the child body"),
        "the child's body was taken too: {out}"
    );
}

#[test]
fn add_sibling_adds_one_and_an_empty_argument_becomes_untitled() {
    let named = dispatch_command(&doc(), "add-sibling", ID, "A sibling").expect("add");
    assert!(named.contains("A sibling"), "{named}");
    assert_eq!(
        headline_titles(&named).len(),
        headline_titles(&doc()).len() + 1
    );

    // The other empty-argument branch. A headline with no title at all
    // is not addressable in an outline, so the bridge names it.
    let blank = dispatch_command(&doc(), "add-sibling", ID, "").expect("add blank");
    assert!(blank.contains("untitled"), "{blank}");
}

#[test]
fn remove_subtree_takes_the_children_with_it() {
    // And `delete` is the same arm under another name — a shell using
    // the alias must not get different behaviour.
    for cmd in ["remove-subtree", "delete"] {
        let out = dispatch_command(&doc(), cmd, ID, "").unwrap_or_else(|e| panic!("{cmd}: {e}"));
        assert!(!out.contains("Parent headline"), "{cmd}: {out}");
        assert!(
            !out.contains("Child headline"),
            "{cmd}: the child outlived its parent: {out}"
        );
    }
}

#[test]
fn promote_and_demote_move_a_headline_a_level() {
    let promoted = dispatch_command(&doc(), "promote", CHILD, "").expect("promote");
    assert!(
        promoted.contains("* Child headline"),
        "the child did not reach level 1: {promoted}"
    );

    let demoted = dispatch_command(&promoted, "demote", CHILD, "").expect("demote");
    assert!(demoted.contains("** Child headline"), "{demoted}");
}

#[test]
fn a_command_the_bridge_does_not_have_is_named_in_the_error() {
    let err = dispatch_command(&doc(), "teleport", ID, "").expect_err("accepted a made-up command");
    assert!(
        err.contains("teleport"),
        "the error does not say which: {err}"
    );
}

#[test]
fn an_id_that_is_not_there_is_an_error_rather_than_a_silent_no_op() {
    // Every arm looks an id up. A bridge that returned the document
    // unchanged would let a browser think an edit landed.
    for cmd in ["rename", "set-todo", "set-body", "promote", "demote"] {
        let r = dispatch_command(&doc(), cmd, "01NOSUCHIDATALL00000", "x");
        assert!(r.is_err(), "`{cmd}` accepted an id that is not there");
    }
}

#[test]
fn source_that_is_not_org_is_an_error_and_never_a_panic() {
    // I5. The browser can hand this anything at all.
    for src in ["\u{0}\u{1}\u{2}", "* [[[", "not org"] {
        // Not org enough to hold the id, so this must fail rather than
        // crash the wasm instance — a panic there takes the page down.
        let _ = dispatch_command(src, "rename", ID, "x");
    }
}

#[test]
fn reformat_round_trips_a_document_it_can_read() {
    let out = reformat(&doc()).expect("reformat");
    assert!(out.contains("Parent headline"), "{out}");
    assert!(out.contains("Child headline"), "{out}");
}

#[test]
fn the_titles_come_back_in_order_and_joined_the_same_way() {
    let titles = headline_titles(&doc());
    assert_eq!(titles, vec!["Parent headline", "Child headline"]);
    // The joined form is what crosses the wasm boundary, since a
    // Vec<String> does not. It has to describe the same list.
    let joined = headline_titles_joined(&doc());
    for t in &titles {
        assert!(joined.contains(t.as_str()), "{joined}");
    }
}

#[test]
fn an_empty_document_has_no_titles_and_does_not_fail() {
    assert!(headline_titles("").is_empty());
    assert!(headline_titles_joined("").is_empty());
}
