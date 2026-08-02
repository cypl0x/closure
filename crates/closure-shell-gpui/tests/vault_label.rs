//! "[#B] Show vault (path somewhere) in the top left".
//!
//! Already there: the header's third element is the vault, clickable,
//! and it shortens `$HOME` the way a shell prompt does — a vault at
//! `~/vault` reads `~/vault`.
//!
//! What it did badly is the case where the path is long. The label was
//! clipped by the pane, which cuts the *tail*, so a vault at
//! `/tmp/…/scratchpad/dvault` showed
//! `/tmp/claude-1000/-home-cypl0x-dev-clo…` — the part you already
//! know, with the part that distinguishes one vault from another cut
//! off. The function's own comment says the last component is what
//! matters; the rendering threw exactly that away.
//!
//! So a long path loses its middle instead of its end.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::path::Path;

use closure_shell_gpui::vault_label;

#[test]
fn a_vault_under_home_reads_like_a_shell_prompt() {
    // Unchanged, and the case that matters most: this is what the
    // user's own header shows.
    assert_eq!(
        vault_label(Path::new("/home/wap/vault"), Some(Path::new("/home/wap"))),
        "~/vault"
    );
}

#[test]
fn home_itself_is_a_tilde() {
    assert_eq!(
        vault_label(Path::new("/home/wap"), Some(Path::new("/home/wap"))),
        "~"
    );
}

#[test]
fn a_short_path_outside_home_is_untouched() {
    assert_eq!(
        vault_label(Path::new("/srv/notes"), Some(Path::new("/home/wap"))),
        "/srv/notes"
    );
}

#[test]
fn a_long_path_keeps_the_name_of_the_vault() {
    // The reported shape. Whatever else is dropped, the last component
    // survives — it is the only part that tells two vaults apart.
    let long =
        Path::new("/tmp/claude-1000/-home-cypl0x-dev-closure/dedd5b7c-0fdf-4a10/scratchpad/dvault");
    let label = vault_label(long, Some(Path::new("/home/wap")));
    assert!(
        label.ends_with("dvault"),
        "the vault's own name was cut off: {label}"
    );
}

#[test]
fn a_long_path_says_it_dropped_something() {
    let long =
        Path::new("/tmp/claude-1000/-home-cypl0x-dev-closure/dedd5b7c-0fdf-4a10/scratchpad/dvault");
    let label = vault_label(long, Some(Path::new("/home/wap")));
    assert!(label.contains('…'), "silently shortened: {label}");
    assert!(
        label.starts_with('/'),
        "and it still says where it is rooted: {label}"
    );
}

#[test]
fn a_long_path_under_home_keeps_the_tilde_and_the_name() {
    let long = Path::new("/home/wap/some/deeply/nested/place/for/notes/second-vault");
    let label = vault_label(long, Some(Path::new("/home/wap")));
    assert!(label.starts_with('~'), "{label}");
    assert!(label.ends_with("second-vault"), "{label}");
}

#[test]
fn the_label_is_short_enough_to_sit_in_a_header() {
    // A header row has buttons on it. The whole point of shortening is
    // that the label cannot push them off.
    let long =
        Path::new("/tmp/claude-1000/-home-cypl0x-dev-closure/dedd5b7c-0fdf-4a10/scratchpad/dvault");
    let label = vault_label(long, Some(Path::new("/home/wap")));
    assert!(
        label.chars().count() <= 40,
        "{} chars: {label}",
        label.chars().count()
    );
}
