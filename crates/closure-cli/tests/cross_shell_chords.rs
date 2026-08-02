//! D6: input-mode conformance + cross-shell chord identity.
//!
//! Two invariants the vision demands ("every command shows its keybinding
//! in every UI element", consistently across all five modes — I4):
//!
//! 1. Every command resolves to a binding in all five modes. `Action::new`
//!    returns `None` when a command is unbound, so a present `Action` *is*
//!    the proof (the chord cannot be missing by construction).
//! 2. The chord shown for a given (mode, command) is *identical* across
//!    three independent shell render paths — the TUI (`render_snapshot`),
//!    the web shell (`render_view`), and the gpui shell
//!    (`App::palette_results`, the list its which-key paints). All three
//!    read the one keymap source (`chord_for_command`); this test fails if
//!    any shell ever hardcodes or diverges.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_config::InputMode;
use closure_input::{chord_for_command, mode_keymap};
use closure_shell_core::{Action, App, FieldView, Node, PaletteItemView};
use closure_shell_web::render_view;
use closure_tui::render_snapshot;

const MODES: [InputMode; 5] = [
    InputMode::Notion,
    InputMode::Emacs,
    InputMode::Vim,
    InputMode::Doom,
    InputMode::Helix,
];

/// The canonical command set (identical across modes; only chords differ).
fn command_set() -> Vec<&'static str> {
    mode_keymap(InputMode::Doom)
        .iter()
        .map(|(_, c)| *c)
        .collect()
}

#[test]
fn every_command_resolves_to_an_action_in_every_mode() {
    for mode in MODES {
        for cmd in command_set() {
            let action = Action::new(mode, cmd);
            assert!(
                action.is_some(),
                "{mode:?} has no binding for {cmd} — I4 violated"
            );
            // The type guarantees a chord exists; it must equal the source.
            let a = action.unwrap();
            assert_eq!(
                a.chord(),
                chord_for_command(mode, cmd).unwrap(),
                "{mode:?}/{cmd}: Action chord diverges from the keymap source"
            );
            assert!(!a.chord().is_empty(), "{mode:?}/{cmd}: empty chord");
        }
    }
}

/// Commands the gpui which-key (`palette_results`) lists *and* whose chords
/// are HTML-safe in every mode (no `<>&`), so one raw substring check works
/// for the TUI and web outputs alike.
const SHARED: [&str; 6] = [
    // Renamed from `capture-start` on 2026-08-02 with the rest of the
    // command vocabulary: verb first, and a bare noun opens a pane.
    "capture",
    "add-sibling",
    "rename",
    "delete",
    // Renamed again on 2026-08-02 for "cycle-mode is not a sound
    // name": which mode? The keymap, not the editor's vim mode, which
    // is the other thing "mode" means in the status bar three inches
    // away.
    "next-input-mode",
    "quit",
];

#[test]
fn chord_is_identical_across_tui_web_and_gpui() {
    for mode in MODES {
        // The gpui shell paints this list; build it once per mode.
        let mut app = App::new();
        app.set_mode(mode);
        let palette = app.palette_results();

        for cmd in SHARED {
            let src = chord_for_command(mode, cmd).unwrap();
            let action = Action::new(mode, cmd).unwrap();

            // The shared ViewTree the TUI + web shells both render.
            let node = Node::Pane {
                title: "t".to_owned(),
                children: vec![
                    Node::Palette {
                        items: vec![PaletteItemView {
                            label: cmd.to_owned(),
                            action: action.clone(),
                        }],
                        cursor: 0,
                    },
                    Node::Detail {
                        fields: vec![FieldView {
                            label: cmd.to_owned(),
                            value: "v".to_owned(),
                            action: Some(action),
                        }],
                    },
                ],
            };

            let tui = render_snapshot(&node);
            let web = render_view(&node);

            assert!(
                tui.contains(src),
                "TUI must show {mode:?}/{cmd} chord {src:?}: {tui}"
            );
            assert!(
                web.contains(src),
                "web must show {mode:?}/{cmd} chord {src:?}: {web}"
            );
            // gpui which-key shows the same chord from the same source.
            assert!(
                palette.iter().any(|(_, ch)| ch == src),
                "gpui palette must show {mode:?}/{cmd} chord {src:?}: {palette:?}"
            );
        }
    }
}
