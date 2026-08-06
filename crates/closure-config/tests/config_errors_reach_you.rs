//! "configuration language … This is not something like CUE, idris or
//! whatever that gives us build time errors that normally would be
//! runtime errors."
//!
//! closure's config *does* validate at load: unknown keys and bad
//! values are refused there rather than at first use of the feature.
//! What it did with the refusal was the problem. Every caller in the
//! app reads the file with `.unwrap_or_default()`, so one mistyped key
//! threw away the *whole* config — your input mode, your theme, your
//! bindings — and said nothing at all. A load-time error you never see
//! is worse than a runtime one you do.
//!
//! So: the error names the line and offers the key you meant, and
//! there is one way to ask for it that the app itself uses.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

fn err(src: &str) -> String {
    format!(
        "{}",
        closure_config::Config::from_org_source(src).expect_err("this config is broken")
    )
}

#[test]
fn a_mistyped_key_says_which_one_you_meant() {
    let e = err("#+begin_src closure-config\ninput_mode = vim\nthemes = doom\n#+end_src\n");
    assert!(e.contains("themes"), "{e}");
    assert!(
        e.contains("theme") && e.contains("did you mean"),
        "no suggestion for a one-letter typo: {e}"
    );
}

#[test]
fn a_key_that_is_nothing_like_one_of_ours_gets_no_invented_suggestion() {
    let e = err("#+begin_src closure-config\nrefrigerator = cold\n#+end_src\n");
    assert!(e.contains("refrigerator"), "{e}");
    assert!(
        !e.contains("did you mean"),
        "a suggestion was invented for a key that resembles nothing: {e}"
    );
}

#[test]
fn the_error_says_which_line() {
    let e =
        err("#+begin_src closure-config\n# a comment\ntheme = doom\nthemes = doom\n#+end_src\n");
    assert!(e.contains("line 4"), "the line number is missing: {e}");
}

#[test]
fn every_key_the_parser_takes_is_in_the_sample_config() {
    // The suggestion list is read from the generated sample, so a key
    // the sample does not mention is a key nobody can be pointed at.
    // Asserted the only way round it can be: every key the sample
    // offers must parse.
    for key in closure_config::Config::known_keys() {
        let src = format!("#+begin_src closure-config\n{key} = x\n#+end_src\n");
        let got = closure_config::Config::from_org_source(&src);
        assert!(
            !matches!(got, Err(closure_config::ConfigError::UnknownKey { .. })),
            "the sample config offers `{key}`, which the parser rejects as unknown"
        );
    }
}
