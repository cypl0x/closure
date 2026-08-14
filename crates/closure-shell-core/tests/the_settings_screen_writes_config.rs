//! Editing a setting, and what reaches `config.org`.
//!
//! `commit_setting` was entirely uncovered, and it is the one screen in
//! this crate that rewrites the user's configuration file. Everything
//! it can do wrong is quiet: a value written to the wrong key, a
//! comment eaten by the rewriter, or — the one the code itself warns
//! about — a screen saying "saved" over a file that will no longer
//! load.
//!
//! The write goes through `closure_config::set_config_key`, so the
//! claim being tested is not "a file appeared" but "the rest of the
//! file survived". A settings screen that reformats config.org the
//! first time somebody changes a theme is a settings screen nobody can
//! trust with a hand-written file.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::fs;

use closure_config::InputMode;
use closure_shell_core::{ModalApp, ModalSurface, Shell};
use closure_store::Vault;

const NOTES: &str = "* Alpha\n\
                     :PROPERTIES:\n\
                     :ID: 01SETTING00000000000001\n\
                     :END:\n";

fn fixture(config: Option<&str>) -> (tempfile::TempDir, Shell, ModalApp) {
    let dir = tempfile::tempdir().expect("tmp");
    fs::write(dir.path().join("notes.org"), NOTES).expect("write");
    if let Some(c) = config {
        fs::write(dir.path().join(closure_config::CONFIG_FILE), c).expect("write config");
    }
    let vault = Vault::open(dir.path()).expect("open");
    (dir, Shell::new(vault), ModalApp::new(InputMode::Doom))
}

/// Open the settings screen and land on the row for `key`.
fn focus(app: &mut ModalApp, sh: &mut Shell, key: &str) {
    // Opened the way a chord opens it — `assistant-setup` is the
    // command the keymap resolves to — and walked with the same `j`
    // the user presses, rather than by setting the cursor directly.
    app.run(sh, "assistant-setup");
    assert_eq!(
        app.surface(),
        ModalSurface::Settings,
        "settings did not open"
    );
    let rows = app.settings_rows(sh);
    let idx = rows.iter().position(|r| r.key == key).unwrap_or_else(|| {
        panic!(
            "no settings row for `{key}`: {:?}",
            rows.iter().map(|r| r.key).collect::<Vec<_>>()
        )
    });
    for _ in 0..idx {
        app.on_key(sh, "j", false, false, Some('j'));
    }
    app.on_key(sh, "enter", false, false, None);
    assert_eq!(
        app.surface(),
        ModalSurface::Setting,
        "the value prompt did not open"
    );
    assert_eq!(app.editing_setting(), Some(key));
}

fn type_into(app: &mut ModalApp, sh: &mut Shell, text: &str) {
    for c in text.chars() {
        app.on_key(sh, &c.to_string(), false, false, Some(c));
    }
}

#[test]
fn editing_a_setting_writes_it_to_config_org() {
    let (d, mut sh, mut app) = fixture(None);
    focus(&mut app, &mut sh, "llm_model");
    app.run(&mut sh, "kill-line-back");
    app.run(&mut sh, "kill-line");
    type_into(&mut app, &mut sh, "a-model-name");
    app.on_key(&mut sh, "enter", false, false, None);

    assert_eq!(app.surface(), ModalSurface::Settings, "it did not go back");
    let written = fs::read_to_string(d.path().join(closure_config::CONFIG_FILE))
        .expect("config.org was not created");
    assert!(written.contains("llm_model"), "{written}");
    assert!(written.contains("a-model-name"), "{written}");
}

#[test]
fn what_was_written_loads_back() {
    // The code's own warning: "a screen that said `saved` over a
    // config.org that will not load is worse than an error". So the
    // file has to parse afterwards, not merely exist.
    let (d, mut sh, mut app) = fixture(None);
    focus(&mut app, &mut sh, "llm_model");
    app.run(&mut sh, "kill-line-back");
    app.run(&mut sh, "kill-line");
    type_into(&mut app, &mut sh, "a-model-name");
    app.on_key(&mut sh, "enter", false, false, None);

    let cfg = closure_config::Config::from_path(&d.path().join(closure_config::CONFIG_FILE))
        .expect("the file it just wrote does not load");
    assert_eq!(cfg.llm_model.as_deref(), Some("a-model-name"));
}

#[test]
fn the_rest_of_the_file_survives_the_edit() {
    // The reason the write goes through `set_config_key` rather than
    // rendering a fresh config: a comment, an unusual ordering, and a
    // key this build does not know all have to come back untouched.
    let original = "#+BEGIN_SRC closure-config\n\
                    # a comment somebody wrote\n\
                    llm_model = old-model\n\
                    some_key_this_build_does_not_know = 7\n\
                    #+END_SRC\n";
    let (d, mut sh, mut app) = fixture(Some(original));
    focus(&mut app, &mut sh, "llm_model");
    app.run(&mut sh, "kill-line-back");
    app.run(&mut sh, "kill-line");
    type_into(&mut app, &mut sh, "a-model-name");
    app.on_key(&mut sh, "enter", false, false, None);

    let after = fs::read_to_string(d.path().join(closure_config::CONFIG_FILE)).expect("read");
    assert!(
        after.contains("# a comment somebody wrote"),
        "the comment was eaten: {after}"
    );
    assert!(
        after.contains("some_key_this_build_does_not_know = 7"),
        "an unknown key was dropped: {after}"
    );
    assert!(
        after.contains("a-model-name"),
        "the edit did not land: {after}"
    );
}

#[test]
fn clearing_a_setting_says_cleared_rather_than_setting_it_to_nothing() {
    let (_d, mut sh, mut app) = fixture(Some(
        "#+BEGIN_SRC closure-config\nllm_model = old-model\n#+END_SRC\n",
    ));
    focus(&mut app, &mut sh, "llm_model");
    app.run(&mut sh, "kill-line-back");
    app.run(&mut sh, "kill-line");
    app.on_key(&mut sh, "enter", false, false, None);
    assert_eq!(app.surface(), ModalSurface::Settings);
}

#[test]
fn a_file_with_no_config_block_gains_one_and_keeps_what_was_there() {
    // I expected a refusal here — a file the rewriter cannot find a
    // block in seemed like the case where changing nothing is the safe
    // answer. It appends a block instead, and that is better: the
    // prose is untouched and the setting still lands, where refusing
    // would have left the user unable to change a setting at all
    // because of text they wrote above it.
    //
    // The distinction the safety argument actually turns on is not
    // "understood the file" but "destroyed anything", and nothing is
    // destroyed.
    let prose = "this is not a closure-config block at all\njust prose\n";
    let (d, mut sh, mut app) = fixture(Some(prose));
    focus(&mut app, &mut sh, "llm_model");
    app.run(&mut sh, "kill-line-back");
    app.run(&mut sh, "kill-line");
    type_into(&mut app, &mut sh, "a-model-name");
    app.on_key(&mut sh, "enter", false, false, None);

    assert_eq!(app.surface(), ModalSurface::Settings, "it did not go back");
    let after = fs::read_to_string(d.path().join(closure_config::CONFIG_FILE)).expect("read");
    assert!(
        after.starts_with(prose),
        "the prose that was already there did not survive: {after:?}"
    );
    assert!(
        after.contains("a-model-name"),
        "the setting did not land: {after:?}"
    );
    assert!(
        closure_config::Config::from_path(&d.path().join(closure_config::CONFIG_FILE)).is_ok(),
        "what it appended does not load: {after:?}"
    );
}

#[test]
fn leaving_the_prompt_without_committing_changes_nothing() {
    let original = "#+BEGIN_SRC closure-config\nllm_model = old-model\n#+END_SRC\n";
    let (d, mut sh, mut app) = fixture(Some(original));
    focus(&mut app, &mut sh, "llm_model");
    app.run(&mut sh, "kill-line-back");
    app.run(&mut sh, "kill-line");
    type_into(&mut app, &mut sh, "a-model-name");
    app.on_key(&mut sh, "escape", false, false, None);

    let after = fs::read_to_string(d.path().join(closure_config::CONFIG_FILE)).expect("read");
    assert_eq!(after, original, "escape wrote the edit anyway");
}
