//! "[#C] A2A protocol … We should think about the use case of
//! connecting an Agent to closure."
//!
//! The card is the thing another agent reads to decide whether talking
//! to closure is worth it. Ours said:
//!
//!     {"name":"closure","version":"0.0.0","skills":["task/delegate"]}
//!
//! which names the *transport* as the skill and says nothing about what
//! closure can actually do — so an agent discovering it learns that it
//! can delegate a task, and not one thing about what a task may be.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_store::Vault;

fn vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* Alpha\n:PROPERTIES:\n:ID: 01A2AAAAAAAAAAAAAAAAAAAAAA\n:END:\nbody\n",
    )
    .unwrap();
    let v = Vault::open(dir.path()).unwrap();
    (dir, v)
}

fn card(v: &mut Vault) -> String {
    closure_a2a::handle_message(v, r#"{"jsonrpc":"2.0","id":1,"method":"agent/card"}"#)
        .expect("a card")
}

#[test]
fn the_card_says_what_closure_is_for() {
    let (_d, mut v) = vault();
    let c = card(&mut v);
    assert!(c.contains("\"description\""), "no description: {c}");
    assert!(
        c.to_lowercase().contains("org"),
        "it does not say what kind of thing it is: {c}"
    );
}

#[test]
fn the_skills_are_the_tools_not_the_transport() {
    // An agent asks "what can you do", not "how do I ask".
    let (_d, mut v) = vault();
    let c = card(&mut v);
    for tool in ["search", "read", "capture"] {
        assert!(c.contains(tool), "`{tool}` is not advertised: {c}");
    }
}

#[test]
fn every_advertised_skill_actually_runs() {
    // The A2A version of the dead-chord rule: a skill on the card that
    // the delegate path cannot run is a promise to another machine.
    let (_d, mut v) = vault();
    let c = card(&mut v);
    for skill in closure_a2a::SKILLS {
        assert!(
            c.contains(skill.id),
            "{} is missing from the card",
            skill.id
        );
        // A plausible call for each, because "does the vault know this
        // tool" and "did I give it what it needs" are two questions.
        let args = match skill.id {
            "read" => " notes.org",
            "search" => " Alpha",
            "capture" => " a captured note",
            "rename" => " 01A2AAAAAAAAAAAAAAAAAAAAAA Renamed",
            "set-property" => " 01A2AAAAAAAAAAAAAAAAAAAAAA KEY value",
            _ => "",
        };
        let msg = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"task/delegate","params":{{"task":"{}{args}"}}}}"#,
            skill.id
        );
        let out = closure_a2a::handle_message(&mut v, &msg).expect("a reply");
        assert!(
            !out.contains("unknown tool"),
            "`{}` is advertised and the vault does not know it: {out}",
            skill.id
        );
    }
}

#[test]
fn a_skill_carries_a_sentence_a_person_could_read() {
    for skill in closure_a2a::SKILLS {
        assert!(!skill.name.is_empty(), "{}", skill.id);
        assert!(
            skill.description.len() > 10,
            "{} has no description worth reading",
            skill.id
        );
    }
}
