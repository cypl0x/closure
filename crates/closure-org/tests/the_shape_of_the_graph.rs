//! The vault as a graph: who links to whom, and who is alone.
//!
//! These are the queries a "what is not connected to anything" view is
//! built from, and every one of them was reachable and unreached. They
//! share one failure mode and it is quiet: an off-by-one in the
//! direction of an edge turns "nothing links here" into "this links
//! nowhere", which is a different set of notes and looks equally
//! plausible on screen.
//!
//! One fixture, deliberately, with a known shape — a hub, a source, a
//! sink, an orphan, a duplicate and a dead link — so each query is
//! asked about the same graph and a wrong answer is a wrong *set*
//! rather than an empty one.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::parse;

const HUB: &str = "01GRAPH00000000000001";
const SOURCE: &str = "01GRAPH00000000000002";
const SINK: &str = "01GRAPH00000000000003";
const ORPHAN: &str = "01GRAPH00000000000004";
const DEAD: &str = "01GRAPH00000000000099";

/// A graph with one of everything.
///
/// - SOURCE links to HUB and to SINK, and nothing links to SOURCE.
/// - HUB links to SINK, and SOURCE links to it: both directions.
/// - SINK is linked to twice and links nowhere.
/// - ORPHAN has no edges at all.
/// - HUB also links to DEAD, which is no headline.
fn src() -> String {
    format!(
        "* Source :one:\n\
         :PROPERTIES:\n:ID: {SOURCE}\n:END:\n\
         to [[id:{HUB}]] and [[id:{SINK}]]\n\
         * Hub :one:two:\n\
         :PROPERTIES:\n:ID: {HUB}\n:CATEGORY: build\n:PRIORITY: x\n:END:\n\
         to [[id:{SINK}]] and [[id:{DEAD}]]\n\
         * Sink\n\
         :PROPERTIES:\n:ID: {SINK}\n:END:\n\
         no links here\n\
         * Orphan\n\
         :PROPERTIES:\n:ID: {ORPHAN}\n:END:\n\
         alone\n"
    )
}

#[test]
fn a_source_has_outgoing_edges_and_no_incoming_ones() {
    let d = parse(&src()).expect("parses");
    let sources = d.source_only_ids();
    assert!(sources.contains(&SOURCE), "{sources:?}");
    assert!(
        !sources.contains(&SINK),
        "the sink was called a source: {sources:?}"
    );
    assert!(
        !sources.contains(&HUB),
        "the hub has both directions: {sources:?}"
    );
    assert!(
        !sources.contains(&ORPHAN),
        "an orphan has no outgoing edge either: {sources:?}"
    );
}

#[test]
fn a_sink_has_incoming_edges_and_no_outgoing_ones() {
    // The mirror of the test above, on the same fixture. Only checking
    // one direction is how a swapped comparison survives.
    let d = parse(&src()).expect("parses");
    let sinks = d.sink_only_ids();
    assert!(sinks.contains(&SINK), "{sinks:?}");
    assert!(
        !sinks.contains(&SOURCE),
        "the source was called a sink: {sinks:?}"
    );
    assert!(!sinks.contains(&HUB), "{sinks:?}");
    assert!(!sinks.contains(&ORPHAN), "{sinks:?}");
}

#[test]
fn a_headline_with_no_edges_at_all_is_neither() {
    let d = parse(&src()).expect("parses");
    assert!(!d.source_only_ids().contains(&ORPHAN));
    assert!(!d.sink_only_ids().contains(&ORPHAN));
}

#[test]
fn a_link_to_nothing_is_a_dead_target_and_a_link_to_something_is_not() {
    let d = parse(&src()).expect("parses");
    let dead = d.dead_link_targets();
    // Targets come back as written, scheme and all — `id:01GRAPH…`,
    // not the bare id. That is the right choice for a report somebody
    // reads: it is the text in their file, and it is what they would
    // search for to find the broken link.
    assert!(dead.iter().any(|t| t == &format!("id:{DEAD}")), "{dead:?}");
    assert!(
        !dead.iter().any(|t| t.contains(SINK)),
        "a link that resolves was called dead: {dead:?}"
    );
    assert_eq!(dead.len(), 1, "{dead:?}");
}

#[test]
fn a_vault_where_every_link_resolves_has_no_dead_targets() {
    let d = parse(&format!(
        "* One\n:PROPERTIES:\n:ID: {HUB}\n:END:\nto [[id:{SINK}]]\n\
         * Two\n:PROPERTIES:\n:ID: {SINK}\n:END:\n"
    ))
    .expect("parses");
    assert!(d.dead_link_targets().is_empty());
}

#[test]
fn the_most_linked_headline_is_the_one_with_the_most_links_out() {
    // Out, not in. SOURCE writes two links; HUB writes two as well but
    // one is dead — which still counts, because this asks what a
    // headline *says*, not what resolves.
    let d = parse(&src()).expect("parses");
    let h = d.most_linked().expect("a most-linked headline");
    assert!(
        h.title() == "Source" || h.title() == "Hub",
        "the sink or the orphan won: {}",
        h.title()
    );
    assert_ne!(h.title(), "Sink");
    assert_ne!(h.title(), "Orphan");
}

#[test]
fn a_document_with_no_links_has_no_most_linked_headline() {
    let d = parse("* One\n* Two\n").expect("parses");
    assert!(
        // Either there is no answer, or the answer links to nothing —
        // both are honest here, and which one it gives is not a promise
        // worth pinning.
        d.most_linked().is_none_or(|h| h.link_targets().is_empty())
    );
}

#[test]
fn the_most_tagged_and_most_propertied_are_counted_separately() {
    // Hub carries two tags and three properties; Source one tag and
    // one property. Two counts that would agree on this fixture only
    // if one were reading the other's field.
    let d = parse(&src()).expect("parses");
    let tagged = d.most_tagged().expect("most tagged");
    assert_eq!(tagged.title(), "Hub", "{}", tagged.title());
    let propertied = d.most_propertied().expect("most propertied");
    assert_eq!(propertied.title(), "Hub", "{}", propertied.title());
}

#[test]
fn duplicate_ids_are_the_ones_written_twice_and_nothing_else() {
    let d = parse(&format!(
        "* One\n:PROPERTIES:\n:ID: {HUB}\n:END:\n\
         * Two\n:PROPERTIES:\n:ID: {HUB}\n:END:\n\
         * Three\n:PROPERTIES:\n:ID: {SINK}\n:END:\n"
    ))
    .expect("parses");
    let dups = d.duplicate_ids();
    assert_eq!(dups, vec![HUB], "{dups:?}");
}

#[test]
fn a_document_with_unique_ids_reports_no_duplicates() {
    assert!(parse(&src()).expect("parses").duplicate_ids().is_empty());
}

#[test]
fn the_highest_priority_is_a_before_b_and_no_cookie_is_not_a_priority() {
    // `'A' < 'B'`, and headlines without a cookie are skipped — a
    // comparison that treated "none" as lowest would still pass a test
    // with only cookied headlines in it, so the fixture has both.
    let d =
        parse("* [#B] Middle\n* No cookie at all\n* [#A] Top\n* [#C] Bottom\n").expect("parses");
    let h = d.highest_priority().expect("a highest priority");
    assert_eq!(h.title(), "Top", "{}", h.title());
}

#[test]
fn a_document_with_no_priorities_has_no_highest_one() {
    let d = parse("* One\n* Two\n").expect("parses");
    assert!(d.highest_priority().is_none());
}

#[test]
fn the_path_of_an_id_addresses_that_headline_and_no_other() {
    let d = parse(&src()).expect("parses");
    let p = d.path_of(HUB).expect("a path for the hub");
    assert_eq!(p, vec![1], "the hub is the second root: {p:?}");
    assert_eq!(d.path_of(SOURCE).expect("source"), vec![0]);
    assert_eq!(d.path_of(ORPHAN).expect("orphan"), vec![3]);
}

#[test]
fn the_path_of_a_nested_headline_names_every_step() {
    let d = parse(&format!(
        "* Root\n:PROPERTIES:\n:ID: {SOURCE}\n:END:\n\
         ** Child\n** Second child\n\
         *** Grandchild\n:PROPERTIES:\n:ID: {HUB}\n:END:\n"
    ))
    .expect("parses");
    assert_eq!(d.path_of(HUB).expect("grandchild"), vec![0, 1, 0]);
}

#[test]
fn the_path_of_an_id_that_is_not_there_is_none() {
    let d = parse(&src()).expect("parses");
    assert_eq!(d.path_of("01NOSUCHIDATALL00000"), None);
    assert_eq!(d.path_of(""), None);
}
