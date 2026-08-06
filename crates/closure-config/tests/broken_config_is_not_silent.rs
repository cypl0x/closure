//! "configuration language … build time errors that normally would be
//! runtime errors."
//!
//! The validation was already there. What happened to it was not: every
//! caller in the app loads the file with `.unwrap_or_default()`, so one
//! mistyped key threw away the *whole* config — input mode, theme,
//! bindings, everything — and said nothing at all. The user's setting
//! stopped working and the reason was on a code path nobody runs.
//!
//! An error at load time that nobody is shown is not an error at load
//! time.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::Config;

const BROKEN: &str = "#+begin_src closure-config\ninput_mode = vim\nthemes = doom\n#+end_src\n";
const FINE: &str = "#+begin_src closure-config\ninput_mode = vim\n#+end_src\n";

fn write(src: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.org"), src).unwrap();
    dir
}

#[test]
fn a_broken_config_still_loads_but_says_what_is_wrong() {
    let dir = write(BROKEN);
    let (cfg, complaint) = Config::load_reporting(&dir.path().join("config.org"));
    let complaint = complaint.expect("a broken config that says nothing is the bug");
    assert!(complaint.contains("themes"), "{complaint}");
    assert!(complaint.contains("line 3"), "{complaint}");
    assert_eq!(
        cfg.input_mode,
        Config::default().input_mode,
        "a config that failed to parse must not be half-applied"
    );
}

#[test]
fn a_good_config_complains_about_nothing() {
    let dir = write(FINE);
    let (cfg, complaint) = Config::load_reporting(&dir.path().join("config.org"));
    assert!(complaint.is_none(), "{complaint:?}");
    assert_eq!(cfg.input_mode, closure_config::InputMode::Vim);
}

#[test]
fn no_config_at_all_is_not_a_complaint() {
    // Most vaults have no config.org, and that is not a mistake.
    let dir = tempfile::tempdir().unwrap();
    let (_, complaint) = Config::load_reporting(&dir.path().join("config.org"));
    assert!(complaint.is_none(), "{complaint:?}");
}

#[test]
fn a_file_with_no_config_block_is_not_a_complaint_either() {
    // `config.org` that is all prose and no block: the vault has notes
    // in a file of that name, which is a thing a vault is allowed to
    // have.
    let dir = write("* Some note\nnothing to configure here\n");
    let (_, complaint) = Config::load_reporting(&dir.path().join("config.org"));
    assert!(complaint.is_none(), "{complaint:?}");
}
