//! A `config.org` written as an org document still configures.
//!
//! `from_org_source` searched `doc.preamble()` — the nodes before the
//! first headline — so a config block under `* Settings` was not found
//! at all. Not "the error was not shown": the whole file was ignored,
//! every setting silently reverted to its default, and `Missing` reads
//! to `load_reporting` as "this vault has no config", which is not a
//! complaint.
//!
//! That is the shape a person actually writes. `config.org` is an org
//! file in a vault of org files; giving it a heading is what everything
//! else in the vault does, and the one arrangement that worked was the
//! bare block at the top with nothing above it.
//!
//! Found by testing the shape the bug would show in rather than the
//! shape that was convenient: the first version of this test put the
//! block at the top of the file and passed.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::Config;

const UNDER_HEADLINE: &str = "\
* Settings
Notes about why these are what they are.

#+BEGIN_SRC closure-config
wrap = true
outline_width = 44
#+END_SRC
";

#[test]
fn a_block_under_a_headline_is_read() {
    let cfg = Config::from_org_source(UNDER_HEADLINE).expect("a config under a headline");
    assert!(cfg.wrap, "wrap did not take effect");
    assert_eq!(cfg.outline_width, Some(44));
}

#[test]
fn a_block_at_the_top_still_works() {
    let cfg = Config::from_org_source("#+BEGIN_SRC closure-config\nwrap = true\n#+END_SRC\n")
        .expect("a config at the top");
    assert!(cfg.wrap);
}

#[test]
fn a_bad_value_under_a_headline_is_still_an_error() {
    // The half that sent me looking: a mistake in a config nobody reads
    // is a mistake nobody hears about.
    let err = Config::from_org_source(
        "* Settings\n#+BEGIN_SRC closure-config\noutline_width = wide\n#+END_SRC\n",
    )
    .unwrap_err();
    assert!(format!("{err}").contains("outline_width"), "{err}");
}

#[test]
fn the_line_number_is_the_line_in_the_file() {
    // The reason `from_org_source` computes an offset at all. Under a
    // headline the block starts further down, and a line number that is
    // right about the block and wrong about the file is one you have to
    // check before you trust.
    let err = Config::from_org_source(
        "* Settings\nsome prose\n\n#+BEGIN_SRC closure-config\noutline_width = wide\n#+END_SRC\n",
    )
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("line 5"), "wrong line: {msg}");
}

#[test]
fn a_vault_note_that_merely_mentions_config_is_not_one() {
    // `config.org` is allowed to be a note. A block in some other
    // language, or prose about configuration, must not be parsed as
    // settings.
    let src = "* How I configure things\n#+BEGIN_SRC sh\necho wrap = true\n#+END_SRC\n";
    assert!(Config::from_org_source(src).is_err());
}

#[test]
fn the_first_config_block_wins() {
    // Two blocks is ambiguous and silently merging them would make the
    // file's meaning depend on order in a way nothing states.
    let src = "* One\n#+BEGIN_SRC closure-config\noutline_width = 10\n#+END_SRC\n\
               * Two\n#+BEGIN_SRC closure-config\noutline_width = 20\n#+END_SRC\n";
    let cfg = Config::from_org_source(src).expect("a config");
    assert_eq!(cfg.outline_width, Some(10));
}
