//! Pure request → response tests for the web shell: browse page,
//! fuzzy search, and capture form, no sockets involved.

#![allow(clippy::unwrap_used, clippy::expect_used)]

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
fn get_root_lists_files_and_headlines() {
    let (_td, mut v) = vault();
    let r = respond(&mut v, "GET", "/", "");
    assert_eq!(r.status, 200);
    assert!(r.content_type.starts_with("text/html"));
    assert!(r.body.contains("work.org"));
    assert!(r.body.contains("Ship parser"));
}

#[test]
fn get_unknown_path_is_404() {
    let (_td, mut v) = vault();
    let r = respond(&mut v, "GET", "/nope", "");
    assert_eq!(r.status, 404);
}

#[test]
fn get_search_without_query_shows_form() {
    let (_td, mut v) = vault();
    let r = respond(&mut v, "GET", "/search", "");
    assert_eq!(r.status, 200);
    assert!(r.body.contains("<form"), "search form present");
}

#[test]
fn get_search_finds_headlines_fuzzy() {
    let (_td, mut v) = vault();
    let r = respond(&mut v, "GET", "/search?q=shp", "");
    assert_eq!(r.status, 200);
    assert!(r.body.contains("Ship parser"));
    assert!(!r.body.contains("Write spec"), "non-match filtered");
}

/// `export_html` produces one self-contained file with the vault
/// content embedded plus client-side JS search — no server needed.
/// (Client-side wasm parsing is a separate, not-yet-built item; this
/// test only asserts what the export actually delivers.)
#[test]
fn export_html_is_self_contained_with_data_and_search() {
    let (_td, v) = vault();
    let html = closure_shell_web::export_html(&v);
    assert!(html.contains("<!doctype html") || html.contains("<html"));
    assert!(html.contains("work.org"), "vault data is embedded");
    assert!(html.contains("Ship parser"), "headlines embedded");
    assert!(html.contains("<script"), "client JS present");
    assert!(
        html.contains("addEventListener"),
        "search input is wired client-side"
    );
}

#[test]
fn search_results_are_html_escaped() {
    let td = tempfile::tempdir().expect("tempdir");
    fs::write(td.path().join("x.org"), "* a <script> b\n").expect("write");
    let mut v = Vault::open(td.path()).expect("open");
    let r = respond(&mut v, "GET", "/search?q=script", "");
    assert!(!r.body.contains("<script>"));
    assert!(r.body.contains("&lt;script&gt;"));
}

#[test]
fn post_capture_appends_entry_and_redirects() {
    let (_td, mut v) = vault();
    let r = respond(&mut v, "POST", "/capture", "title=Buy+milk%21");
    assert_eq!(r.status, 303);
    assert!(
        v.find_by_title("Buy milk!").is_some(),
        "captured into vault"
    );
}

#[test]
fn post_capture_empty_title_is_400() {
    let (_td, mut v) = vault();
    let r = respond(&mut v, "POST", "/capture", "title=");
    assert_eq!(r.status, 400);
}

#[test]
fn root_page_links_to_search_and_capture() {
    let (_td, mut v) = vault();
    let r = respond(&mut v, "GET", "/", "");
    assert!(r.body.contains("/search"));
    assert!(r.body.contains("/capture"));
}
