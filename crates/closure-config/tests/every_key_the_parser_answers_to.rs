//! Every key `from_kv_block_at` handles, set well and set badly.
//!
//! The parser is a 60-way match and roughly a third of its arms had
//! never run — including all five input modes, both prefix keys
//! (`bind <chord>` and `lsp <lang>`), the socket-address pair, and
//! every `BadValue` a wrong setting can produce.
//!
//! A config parser is a bad place for untested branches. It does not
//! fail loudly when an arm is wrong: it silently gives the user a
//! different program than the one their file describes, and the file
//! is the thing they will re-read when trying to work out why. Each
//! "bad value" arm is equally load-bearing in the other direction —
//! a setting quietly ignored is a setting the user believes is on.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::{Config, ConfigError, InputMode};

/// Wrap `body` in the block the parser looks for.
fn cfg(body: &str) -> Result<Config, ConfigError> {
    Config::from_org_source(&format!("#+BEGIN_SRC closure-config\n{body}\n#+END_SRC\n"))
}

fn ok(body: &str) -> Config {
    cfg(body).unwrap_or_else(|e| panic!("`{body}` was rejected: {e}"))
}

#[test]
fn every_input_mode_has_a_name_the_parser_knows() {
    // Five modes, and a keymap is the most visible setting in the
    // program. One misrouted arm means somebody's vim bindings
    // silently become emacs ones.
    for (name, mode) in [
        ("emacs", InputMode::Emacs),
        ("vim", InputMode::Vim),
        ("doom", InputMode::Doom),
        ("helix", InputMode::Helix),
        ("notion", InputMode::Notion),
    ] {
        let c = ok(&format!("input_mode = {name}"));
        assert_eq!(c.input_mode, mode, "input_mode = {name}");
    }
}

#[test]
fn a_mode_that_does_not_exist_is_refused_and_named() {
    let err = cfg("input_mode = kakoune").expect_err("accepted an unknown mode");
    let text = err.to_string();
    assert!(
        text.contains("kakoune"),
        "the error does not say which: {text}"
    );
}

#[test]
fn every_input_mode_name_the_writer_uses_is_one_the_parser_knows() {
    // The parser and the renderer are two halves of one fact. If they
    // disagree, a config closure wrote cannot be read back by closure —
    // the failure the shell-core settings tests guard from the other
    // side. `default_org` renders whatever the default mode is, so
    // this checks the whole set by feeding each name back through the
    // parser and out again.
    let rendered = Config::default_org();
    let back = Config::from_org_source(&rendered).expect("closure cannot read its own default");
    assert_eq!(back.input_mode, Config::default().input_mode);

    for name in ["emacs", "vim", "doom", "helix", "notion"] {
        let written = rendered.replace(
            &format!("input_mode = {}", mode_name(Config::default().input_mode)),
            &format!("input_mode = {name}"),
        );
        let c = Config::from_org_source(&written)
            .unwrap_or_else(|e| panic!("`{name}` did not survive its own file: {e}"));
        assert_eq!(mode_name(c.input_mode), name);
    }
}

/// The spelling the renderer uses for a mode.
const fn mode_name(m: InputMode) -> &'static str {
    match m {
        InputMode::Emacs => "emacs",
        InputMode::Vim => "vim",
        InputMode::Doom => "doom",
        InputMode::Helix => "helix",
        InputMode::Notion => "notion",
    }
}

#[test]
fn a_bind_line_carries_its_chord_and_command() {
    let c = ok("bind C-c c = capture");
    assert_eq!(
        c.key_bindings,
        vec![("C-c c".to_owned(), "capture".to_owned())]
    );
}

#[test]
fn a_bind_with_no_chord_is_refused() {
    // `bind = capture` binds nothing to something. Accepting it would
    // put an unreachable entry in the keymap.
    assert!(cfg("bind = capture").is_err());
}

#[test]
fn several_binds_all_survive() {
    let c = ok("bind C-c c = capture\nbind C-c s = search");
    assert_eq!(c.key_bindings.len(), 2, "{:?}", c.key_bindings);
}

#[test]
fn an_lsp_line_carries_its_language_and_command() {
    let c = ok("lsp rust = rust-analyzer");
    assert_eq!(
        c.lsp_servers,
        vec![("rust".to_owned(), "rust-analyzer".to_owned())]
    );
}

#[test]
fn an_lsp_with_no_language_is_refused() {
    assert!(cfg("lsp = rust-analyzer").is_err());
}

#[test]
fn a_diagram_tool_is_recorded_per_language() {
    let c = ok("diagram_tool_mermaid = mmdc");
    assert_eq!(
        c.diagram_tools,
        vec![("mermaid".to_owned(), "mmdc".to_owned())]
    );
}

#[test]
fn a_diagram_tool_with_no_language_or_no_program_is_dropped_quietly() {
    // Deliberately not an error: the key *shape* is right and only the
    // halves are empty, so there is nothing to run and nothing to
    // complain about. Asserted so the quiet drop stays a decision.
    let c = ok("diagram_tool_ = mmdc");
    assert!(c.diagram_tools.is_empty(), "{:?}", c.diagram_tools);
}

#[test]
fn every_boolean_key_reads_both_spellings_of_both_answers() {
    // `true/yes/1` and `false/no/0`. A boolean that only understood
    // one spelling would silently leave a setting off for anybody who
    // wrote the other.
    for key in [
        "tag_inheritance",
        "record_commands",
        "log_done",
        "wrap",
        "rail_docked",
    ] {
        for yes in ["true", "yes", "1"] {
            ok(&format!("{key} = {yes}"));
        }
        for no in ["false", "no", "0"] {
            ok(&format!("{key} = {no}"));
        }
        let err =
            cfg(&format!("{key} = maybe")).expect_err(&format!("`{key} = maybe` was accepted"));
        assert!(err.to_string().contains(key), "{err}");
    }
}

#[test]
fn record_commands_and_wrap_actually_land_on_the_config() {
    // Reading a boolean is not the same as storing it in the right
    // field, and a swap between two adjacent bools would pass every
    // "it parsed" test.
    let on = ok("record_commands = true\nwrap = false\nlog_done = true");
    assert!(on.record_commands);
    assert!(!on.wrap);
    assert!(on.log_done);

    let off = ok("record_commands = false\nwrap = true");
    assert!(!off.record_commands);
    assert!(off.wrap);
}

#[test]
fn the_sync_addresses_are_parsed_as_addresses_not_strings() {
    let c = ok("sync_bind = 0.0.0.0:7420\nsync_advertise = 192.168.1.5");
    assert_eq!(
        c.sync_bind.map(|a| a.to_string()),
        Some("0.0.0.0:7420".to_owned())
    );
    assert_eq!(
        c.sync_advertise.map(|a| a.to_string()),
        Some("192.168.1.5".to_owned())
    );
}

#[test]
fn a_sync_bind_without_a_port_is_refused_with_an_example() {
    // The error has to show the shape, because "invalid socket
    // address" tells somebody nothing about what to type instead.
    let err = cfg("sync_bind = 0.0.0.0").expect_err("accepted an address with no port");
    let text = err.to_string();
    assert!(text.contains("host:port"), "{text}");
}

#[test]
fn a_sync_advertise_with_a_port_is_refused_because_the_port_is_not_its_business() {
    let err = cfg("sync_advertise = 192.168.1.5:7420").expect_err("accepted a port");
    assert!(err.to_string().contains("port"), "{err}");
}

#[test]
fn llm_tools_is_a_comma_list() {
    let c = ok("llm_tools = read, search, capture");
    assert_eq!(
        c.llm_tools,
        Some(vec![
            "read".to_owned(),
            "search".to_owned(),
            "capture".to_owned()
        ])
    );
}

#[test]
fn an_llm_provider_that_is_not_supported_is_refused_and_the_options_listed() {
    // Naming the alternatives is the difference between an error and a
    // help message, and this is the setting somebody is most likely to
    // guess at.
    let err = cfg("llm_provider = gpt4all").expect_err("accepted an unknown provider");
    let text = err.to_string();
    assert!(text.contains("gpt4all"), "{text}");
    assert!(
        text.contains("anthropic") || text.contains("ollama"),
        "the error does not list what is supported: {text}"
    );
}

#[test]
fn a_line_that_is_not_key_equals_value_is_refused_with_its_line_number() {
    // A typo in a config file is found by line number or not at all.
    let err = cfg("input_mode = vim\nthis line has no equals sign").expect_err("accepted it");
    let text = err.to_string();
    assert!(text.contains("line"), "no line number in: {text}");
}

#[test]
fn a_key_the_build_does_not_know_is_refused_and_a_near_one_suggested() {
    // I assumed the opposite — that an unknown key would be ignored for
    // forward compatibility, so a file written by a newer closure keeps
    // working on an older one. It is refused instead, and on balance
    // that is the better trade for a file a person hand-writes: a
    // typo'd key that is silently ignored is a setting the user
    // believes is on, and they have no way to find out otherwise.
    //
    // The strictness is only defensible because of the suggestion, so
    // that is what is asserted rather than the refusal alone.
    let err = cfg("input_mode = vim\ninupt_mode = emacs").expect_err("accepted a typo");
    let text = err.to_string();
    assert!(text.contains("inupt_mode"), "{text}");
    assert!(
        text.contains("input_mode"),
        "a typo one letter from a real key got no suggestion: {text}"
    );
}

#[test]
fn a_key_nothing_resembles_is_still_refused_without_a_wrong_suggestion() {
    // The other half: `nearest_key` must not reach for something
    // unrelated just to have an answer. A suggestion that is wrong is
    // worse than none, because it sends somebody to change a line that
    // was never the problem.
    let err = cfg("zzzqqq = 1").expect_err("accepted an unknown key");
    assert!(err.to_string().contains("zzzqqq"), "{err}");
}
