//! Native Qt6/QML desktop shell (X1c / GUI-UX G4).
//!
//! Editor parity with the TUI/egui/gpui/gtk shells: the Qt window renders
//! the shared [`closure_shell_core`] `ViewTree` (the same `Node` tree the
//! others render) via [`qml_view`] and edits through the shared [`Shell`]
//! (I8). [`qml_view`] is the hermetic QML mapping — golden-testable with no
//! display, and the document [`run`] (behind the opt-in `qt` feature) loads
//! for real. The legacy [`qml_document`] read-only list is retained. The
//! default build never pulls Qt (I10).
//!
//! Build/run with a Qt6 SDK on PATH (`qmake6`):
//! `cargo run -p closure-shell-qt --features qt -- <vault>`.

#![forbid(unsafe_code)]

use std::path::Path;

use closure_shell_core::{KeyEvent, Node};
use closure_store::{Vault, VaultError};

// Re-export the shared state core so the qt shell drives the SAME tested
// App/Shell as every other shell (editor parity, not a private model).
pub use closure_shell_core::{App, Shell, browse_view};

/// Map a shared theme to QML colour properties (P5).
///
/// `readonly property color` declarations (window bg, fg, accent) the
/// interactive window injects into its host document. Hermetic.
#[must_use]
pub fn theme_qml(theme: &closure_shell_core::Theme) -> String {
    use closure_shell_core::ColorRole::{Accent, Bg, Fg};
    format!(
        "readonly property color bgColor: \"{}\"\n  \
         readonly property color fgColor: \"{}\"\n  \
         readonly property color accentColor: \"{}\"",
        theme.color(Bg).hex(),
        theme.color(Fg).hex(),
        theme.color(Accent).hex(),
    )
}

/// Apply one key event and return the QML document to reload (P3).
///
/// Dispatches through the shared core (P1), then maps the fresh `ViewTree`
/// to [`qml_view`]. The exact step the windowed [`run`] (via its `QObject`
/// bridge) calls per keypress — so the interactive loop is hermetically
/// tested without Qt.
#[must_use]
pub fn next_frame(app: &mut App, shell: &mut Shell, event: &KeyEvent) -> String {
    qml_view(&app.dispatch(shell, event))
}

/// The display lines for a vault: every headline as `indent + TODO +
/// title`, in document order. Pure + hermetic.
///
/// # Errors
///
/// [`VaultError`] if the vault cannot be opened.
pub fn rows(vault_path: &Path) -> Result<Vec<String>, VaultError> {
    let vault = Vault::open(vault_path)?;
    let mut out = Vec::new();
    for (_, doc) in vault.iter() {
        for h in doc.all_headlines() {
            let indent = "  ".repeat(h.level().saturating_sub(1) as usize);
            let todo = h.todo().map(|t| format!("{t} ")).unwrap_or_default();
            out.push(format!("{indent}{todo}{}", h.title()));
        }
    }
    Ok(out)
}

/// Escape a string for embedding inside a QML/JS double-quoted literal.
fn qml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Build the QML document that renders `rows` as a scrollable list.
/// Pure — the window's content is verifiable without Qt.
#[must_use]
pub fn qml_document(rows: &[String]) -> String {
    let items = rows
        .iter()
        .map(|r| format!("\"{}\"", qml_escape(r)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "import QtQuick 2.15\n\
         import QtQuick.Window 2.15\n\
         Window {{\n\
         \x20 visible: true; width: 700; height: 850; title: \"closure\"\n\
         \x20 ListView {{\n\
         \x20   anchors.fill: parent; model: [{items}]\n\
         \x20   delegate: Text {{ text: modelData; padding: 4 }}\n\
         \x20 }}\n\
         }}\n"
    )
}

/// Render a [`Node`] `ViewTree` to a QML document (G4).
///
/// The full declarative tree as `QtQuick.Controls`/`Layouts` items —
/// exhaustive over every `Node` kind (V1c: a new kind fails to compile
/// here until handled), so the Qt shell renders the whole G1 vocabulary,
/// not just a flat list. Hermetic — the document is verifiable without Qt.
#[must_use]
pub fn qml_view(node: &Node) -> String {
    format!(
        "import QtQuick 2.15\n\
         import QtQuick.Controls 2.15\n\
         import QtQuick.Layouts 1.15\n\
         ApplicationWindow {{\n\
         \x20 visible: true; width: 700; height: 850; title: \"closure\"\n\
         {}\n\
         }}\n",
        qml_item(node, 1)
    )
}

// One flat arm per `Node` kind — exhaustive by design (V1c); splitting
// the match would only hide the one-to-one kind→QML mapping.
#[allow(clippy::too_many_lines)]
fn qml_item(node: &Node, depth: usize) -> String {
    let pad = "  ".repeat(depth);
    let kids = |nodes: &[Node]| -> String {
        nodes
            .iter()
            .map(|n| qml_item(n, depth + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    match node {
        Node::Pane { title, children } => format!(
            "{pad}GroupBox {{ title: \"{}\"\n{pad}  ColumnLayout {{\n{}\n{pad}  }} }}",
            qml_escape(title),
            kids(children)
        ),
        Node::Rows { rows, selected } => {
            let items = rows
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let sel = if i == *selected { "; font.bold: true" } else { "" };
                    let todo = r
                        .todo
                        .as_deref()
                        .map_or_else(String::new, |t| format!("{t} "));
                    let icon = r.icon.as_deref().map_or_else(String::new, |g| format!("{g} "));
                    let badges = if r.badges.is_empty() {
                        String::new()
                    } else {
                        format!("  :{}:", r.badges.join(":"))
                    };
                    format!(
                        "{pad}  Text {{ text: \"{}{}{}{}\"{sel} }}",
                        qml_escape(&icon),
                        qml_escape(&todo),
                        qml_escape(&r.title),
                        qml_escape(&badges)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{pad}ColumnLayout {{\n{items}\n{pad}}}")
        }
        Node::Detail { fields } => {
            let items = fields
                .iter()
                .map(|f| {
                    let kbd = f
                        .action
                        .as_ref()
                        .map_or_else(String::new, |a| format!("  [{}]", a.chord()));
                    format!(
                        "{pad}  Text {{ text: \"{}: {}{}\" }}",
                        qml_escape(&f.label),
                        qml_escape(&f.value),
                        qml_escape(&kbd)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{pad}ColumnLayout {{\n{items}\n{pad}}}")
        }
        Node::Input { label, buffer } => format!(
            "{pad}TextField {{ placeholderText: \"{}\"; text: \"{}\" }}",
            qml_escape(label),
            qml_escape(buffer)
        ),
        Node::Palette { items, cursor } => {
            let lis = items
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    let sel = if i == *cursor { "; font.bold: true" } else { "" };
                    format!(
                        "{pad}  Text {{ text: \"[{}] {}\"{sel} }}",
                        qml_escape(it.action.chord()),
                        qml_escape(&it.label)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{pad}ColumnLayout {{ objectName: \"palette\"\n{lis}\n{pad}}}")
        }
        Node::Hints { line } => format!(
            "{pad}Label {{ objectName: \"hints\"; text: \"{}\" }}",
            qml_escape(line)
        ),
        Node::Widget { name, content } => format!(
            "{pad}GroupBox {{ title: \"{}\"\n{pad}  Text {{ text: \"{}\" }} }}",
            qml_escape(name),
            qml_escape(content)
        ),
        Node::Text(t) => format!("{pad}Text {{ text: \"{}\" }}", qml_escape(t)),
        Node::Split { direction, panes } => {
            let layout = match direction {
                closure_shell_core::SplitDir::Row => "RowLayout",
                closure_shell_core::SplitDir::Column => "ColumnLayout",
            };
            format!("{pad}{layout} {{\n{}\n{pad}}}", kids(panes))
        }
        Node::Modal { title, body } => format!(
            "{pad}Dialog {{ title: \"{}\"; visible: true\n{}\n{pad}}}",
            qml_escape(title),
            qml_item(body, depth + 1)
        ),
        Node::Toast { level, text } => format!(
            "{pad}Label {{ objectName: \"toast-{}\"; text: \"{}\" }}",
            level.as_str(),
            qml_escape(text)
        ),
    }
}

/// Open a native Qt6 window: an interactive editor over the shared
/// [`Shell`] (P3). QML key events route through a `Bridge` `QObject` to
/// [`next_frame`] (P1 dispatch + [`qml_view`]); the displayed `frame`
/// republishes on every key. Blocks on the Qt event loop until closed.
///
/// # Errors
///
/// [`VaultError`] if the vault cannot be opened.
#[cfg(feature = "qt")]
pub fn run(vault_path: &Path) -> Result<(), VaultError> {
    qt_window::run(vault_path)
}

/// The interactive Qt window — isolated so `use qmetaobject::*` (needed by
/// the QObject derive macros) stays out of the default build.
#[cfg(feature = "qt")]
mod qt_window {
    // The qmetaobject QObject macros require the glob + an owned-`QString`
    // method signature + post-Default field assignment — all forced by the
    // crate's API, not style choices.
    #![allow(
        clippy::wildcard_imports,
        clippy::field_reassign_with_default,
        clippy::needless_pass_by_value
    )]
    use std::path::Path;

    use qmetaobject::*;

    use super::{App, KeyEvent, Shell, next_frame, qml_view};
    use closure_store::{Vault, VaultError};

    fn event_from_qml(text: &str, name: &str, ctrl: bool) -> KeyEvent {
        if !name.is_empty() {
            KeyEvent::new(name, ctrl, None)
        } else if let Some(c) = text.chars().next() {
            if ctrl {
                KeyEvent::new(c.to_string(), true, None)
            } else {
                KeyEvent::char(c)
            }
        } else {
            KeyEvent::new(String::new(), ctrl, None)
        }
    }

    /// Build the host QML, injecting the shared theme colour props (P5).
    fn host_qml(theme_props: &str) -> String {
        format!(
            "import QtQuick 2.15\n\
import QtQuick.Controls 2.15\n\
ApplicationWindow {{\n\
  visible: true; width: 700; height: 850; title: \"closure\"\n\
  {theme_props}\n\
  color: bgColor\n\
  Flickable {{\n\
    anchors.fill: parent; focus: true\n\
    Keys.onPressed: (event) => {{\n\
      var ctrl = (event.modifiers & Qt.ControlModifier) != 0;\n\
      if (event.key === Qt.Key_Return) bridge.on_key(\"\", \"enter\", ctrl);\n\
      else if (event.key === Qt.Key_Escape) bridge.on_key(\"\", \"escape\", ctrl);\n\
      else if (event.key === Qt.Key_Backspace) bridge.on_key(\"\", \"backspace\", ctrl);\n\
      else if (event.key === Qt.Key_Up) bridge.on_key(\"\", \"up\", ctrl);\n\
      else if (event.key === Qt.Key_Down) bridge.on_key(\"\", \"down\", ctrl);\n\
      else if (event.text.length > 0) bridge.on_key(event.text, \"\", ctrl);\n\
    }}\n\
    Text {{ text: bridge.frame; color: fgColor; font.family: \"monospace\" }}\n\
  }}\n\
}}\n"
        )
    }

    #[derive(QObject, Default)]
    struct Bridge {
        base: qt_base_class!(trait QObject),
        frame: qt_property!(QString; NOTIFY frame_changed),
        frame_changed: qt_signal!(),
        on_key: qt_method!(
            fn on_key(&mut self, text: QString, name: QString, ctrl: bool) {
                let ev = event_from_qml(&text.to_string(), &name.to_string(), ctrl);
                let mut produced = None;
                let mut quit = false;
                if let Some(shell) = self.shell.as_mut() {
                    produced = Some(next_frame(&mut self.app, shell, &ev));
                    quit = self.app.should_quit();
                }
                if let Some(f) = produced {
                    self.frame = QString::from(f.as_str());
                    self.frame_changed();
                    if quit {
                        std::process::exit(0);
                    }
                }
            }
        ),
        app: App,
        shell: Option<Shell>,
    }

    pub fn run(vault_path: &Path) -> Result<(), VaultError> {
        let shell = Shell::new(Vault::open(vault_path)?);
        let app = App::new();
        let initial = qml_view(&app.view(&shell));
        let mut bridge = Bridge::default();
        bridge.app = app;
        bridge.shell = Some(shell);
        bridge.frame = QString::from(initial.as_str());
        let bridge_box = QObjectBox::new(bridge);
        let mut engine = QmlEngine::new();
        engine.set_object_property("bridge".into(), bridge_box.pinned());
        let theme = super::theme_qml(&closure_shell_core::Theme::dark());
        engine.load_data(host_qml(&theme).into());
        engine.exec();
        Ok(())
    }
}
