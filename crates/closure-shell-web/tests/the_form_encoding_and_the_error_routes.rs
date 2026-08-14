//! Form decoding, and the routes that answer with a failure.
//!
//! `url_decode` is where a browser's encoding meets closure's text, and
//! its `%XX` branch had never been exercised — only the `+`-for-space
//! path. Everything a user types into the search or capture box that is
//! not a plain ASCII word arrives percent-encoded, so this is not an
//! edge: it is every accented letter, every space in a title typed on a
//! phone, and every `#` and `&`.
//!
//! The error routes are here for the same reason as everywhere else in
//! this session: a POST that fails and answers 200 tells the browser
//! the edit landed.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_shell_web::respond;
use closure_store::Vault;
use tempfile::TempDir;

fn vault() -> (TempDir, Vault) {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join("work.org"),
        "* TODO Ship parser :work:\n* DONE Write spec\n",
    )
    .expect("write");
    let v = Vault::open(dir.path()).expect("open");
    (dir, v)
}

#[test]
fn a_percent_encoded_search_term_is_decoded_before_it_is_searched() {
    // `%20` is a space. Without decoding, the query is the literal
    // three characters and matches nothing — a search box that
    // silently fails for every multi-word query.
    let (_d, mut v) = vault();
    let encoded = respond(&mut v, "GET", "/search?q=Ship%20parser", "");
    assert_eq!(encoded.status, 200);
    assert!(
        encoded.body.contains("Ship parser"),
        "a percent-encoded query found nothing: {}",
        encoded.body
    );
}

#[test]
fn a_plus_is_a_space_too() {
    // The other form encoding, and the one already covered — asserted
    // beside the first so the two cannot drift apart.
    let (_d, mut v) = vault();
    let r = respond(&mut v, "GET", "/search?q=Ship+parser", "");
    assert!(r.body.contains("Ship parser"), "{}", r.body);
}

#[test]
fn a_stray_percent_is_kept_rather_than_swallowing_what_follows() {
    // `100%` typed into a search box is not an escape sequence. A
    // decoder that consumed the next two characters regardless would
    // quietly eat part of the query.
    let (_d, mut v) = vault();
    let r = respond(&mut v, "GET", "/search?q=100%", "");
    assert_eq!(r.status, 200);

    // And a `%` followed by something that is not two hex digits.
    let r = respond(&mut v, "GET", "/search?q=50%zz", "");
    assert_eq!(r.status, 200);
}

#[test]
fn a_capture_title_arrives_decoded() {
    let (_d, mut v) = vault();
    let r = respond(&mut v, "POST", "/capture", "title=Buy%20milk%20and%20eggs");
    assert_eq!(r.status, 303, "capture did not redirect: {}", r.body);

    // And the vault has it, spelled the way it was typed.
    let listed = respond(&mut v, "GET", "/", "");
    assert!(
        listed.body.contains("Buy milk and eggs"),
        "the captured title is wrong or missing: {}",
        listed.body
    );
}

#[test]
fn a_capture_with_no_title_is_refused_with_400() {
    // The empty form submission — Enter on an untouched box, the same
    // case the Flutter shell and the C ABI both refuse.
    let (_d, mut v) = vault();
    for body in ["", "title=", "notatitle=x"] {
        let r = respond(&mut v, "POST", "/capture", body);
        assert_eq!(r.status, 400, "`{body}` was captured: {}", r.body);
    }
}

#[test]
fn a_command_the_registry_does_not_have_is_a_400_not_a_500() {
    // The distinction matters: 400 says the client asked for something
    // that is not a command, 500 says closure broke. Collapsing them
    // would make every client typo look like a server fault.
    let (_d, mut v) = vault();
    let r = respond(&mut v, "POST", "/command", "command=no-such-command");
    assert_eq!(r.status, 400, "{}", r.body);
    assert!(r.body.contains("unknown command"), "{}", r.body);
}

#[test]
fn a_command_that_fails_for_another_reason_is_a_500_and_says_what() {
    // An id that is not there is not an unknown command — the command
    // exists and could not do its job.
    let (_d, mut v) = vault();
    let r = respond(
        &mut v,
        "POST",
        "/command",
        "command=rename-headline&id=01NOSUCHIDATALL00000&title=x",
    );
    assert!(r.status >= 400, "a failing command answered {}", r.status);
    assert!(!r.body.is_empty(), "it failed without saying why");
}

#[test]
fn a_route_nobody_serves_is_404() {
    let (_d, mut v) = vault();
    for (method, target) in [
        ("GET", "/nope"),
        ("POST", "/"),
        ("DELETE", "/"),
        ("GET", "/search/extra"),
    ] {
        let r = respond(&mut v, method, target, "");
        assert_eq!(r.status, 404, "{method} {target} was served");
    }
}

#[test]
fn the_view_endpoint_answers_json_a_client_can_parse() {
    // The client-side renderer's data source. An endpoint that
    // answered HTML here would break the page rather than a request.
    let (_d, mut v) = vault();
    let r = respond(&mut v, "GET", "/view", "");
    assert_eq!(r.status, 200);
    assert!(
        r.content_type.contains("application/json"),
        "the view endpoint claims {}",
        r.content_type
    );
    assert!(r.body.starts_with('{'), "not a JSON object: {}", r.body);
    assert!(r.body.ends_with('}'), "truncated JSON: {}", r.body);
}

#[test]
fn a_search_with_no_query_at_all_still_answers() {
    // `/search` with no `?q=` is what a bookmarked link looks like.
    let (_d, mut v) = vault();
    assert_eq!(respond(&mut v, "GET", "/search", "").status, 200);
    assert_eq!(respond(&mut v, "GET", "/search?", "").status, 200);
    assert_eq!(respond(&mut v, "GET", "/search?other=1", "").status, 200);
}
