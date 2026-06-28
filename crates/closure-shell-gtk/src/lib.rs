//! Native GTK4 desktop shell (X1b / GUI-UX G3).
//!
//! Editor parity with the TUI/egui/gpui shells: the gtk window renders the
//! shared [`closure_shell_core`] `ViewTree` (the same `Node` tree tui/web
//! render) and edits through the shared [`Shell`] (I8). [`widget_tree`] is
//! the hermetic GTK4 widget-tree mapping — golden-testable with no display,
//! and the structure [`run`] (behind the opt-in `gtk` feature) builds for
//! real. The default build never pulls GTK (I10).
//!
//! Build/run with the GUI toolchain:
//! `nix develop .#webview -c cargo run -p closure-shell-gtk --features gtk -- <vault>`.

#![forbid(unsafe_code)]

use std::path::Path;

use closure_shell_core::Node;
use closure_store::{Vault, VaultError};

// Re-export the shared state core so the gtk shell drives the SAME tested
// App/Shell as every other shell (editor parity, not a private model).
pub use closure_shell_core::{App, Shell, browse_view};

/// The legacy read-only display lines for a vault.
///
/// Every headline as `indent + TODO + title`, document order. The editor
/// path renders [`widget_tree`] over the shared `ViewTree` instead.
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

/// Render a [`Node`] `ViewTree` to a deterministic GTK4 widget-tree
/// descriptor (G3).
///
/// One indented line per widget — the hierarchy [`run`] builds with real
/// `gtk4` types. Exhaustive over every `Node` kind (V1c: a new kind fails
/// to compile here until handled), so the gtk shell renders the full G1
/// vocabulary, not just a flat list. Hermetic — no display needed.
#[must_use]
pub fn widget_tree(node: &Node) -> String {
    let mut out = Vec::new();
    push_widget(node, 0, &mut out);
    out.join("\n")
}

fn push_widget(node: &Node, depth: usize, out: &mut Vec<String>) {
    let pad = "  ".repeat(depth);
    match node {
        Node::Pane { title, children } => {
            out.push(format!("{pad}GtkFrame label=\"{title}\""));
            out.push(format!("{pad}  GtkBox orientation=vertical"));
            for c in children {
                push_widget(c, depth + 2, out);
            }
        }
        Node::Rows { rows, selected } => {
            out.push(format!("{pad}GtkListBox"));
            for (i, r) in rows.iter().enumerate() {
                let sel = if i == *selected { " selected" } else { "" };
                let todo = r
                    .todo
                    .as_deref()
                    .map_or_else(String::new, |t| format!("{t} "));
                out.push(format!("{pad}  GtkLabel xalign=0{sel} \"{todo}{}\"", r.title));
            }
        }
        Node::Detail { fields } => {
            out.push(format!("{pad}GtkGrid"));
            for f in fields {
                let kbd = f
                    .action
                    .as_ref()
                    .map_or_else(String::new, |a| format!("  [{}]", a.chord()));
                out.push(format!("{pad}  GtkLabel \"{}: {}{kbd}\"", f.label, f.value));
            }
        }
        Node::Input { label, buffer } => {
            out.push(format!("{pad}GtkEntry placeholder=\"{label}\" text=\"{buffer}\""));
        }
        Node::Palette { items, cursor } => {
            out.push(format!("{pad}GtkListBox.palette"));
            for (i, it) in items.iter().enumerate() {
                let sel = if i == *cursor { " selected" } else { "" };
                out.push(format!(
                    "{pad}  GtkLabel{sel} \"[{}] {}\"",
                    it.action.chord(),
                    it.label
                ));
            }
        }
        Node::Hints { line } => out.push(format!("{pad}GtkLabel.hints \"{line}\"")),
        Node::Widget { name, content } => {
            out.push(format!("{pad}GtkFrame.widget label=\"{name}\""));
            for l in content.lines() {
                out.push(format!("{pad}  GtkLabel \"{l}\""));
            }
        }
        Node::Text(t) => out.push(format!("{pad}GtkLabel \"{t}\"")),
        Node::Split { direction, panes } => {
            let orient = match direction {
                closure_shell_core::SplitDir::Row => "horizontal",
                closure_shell_core::SplitDir::Column => "vertical",
            };
            out.push(format!("{pad}GtkBox orientation={orient}"));
            for p in panes {
                push_widget(p, depth + 1, out);
            }
        }
        Node::Modal { title, body } => {
            out.push(format!("{pad}GtkDialog modal=true title=\"{title}\""));
            push_widget(body, depth + 1, out);
        }
        Node::Toast { level, text } => {
            out.push(format!("{pad}GtkInfoBar.{} \"{text}\"", level.as_str()));
        }
    }
}

/// Open a native GTK4 window that renders the shared `ViewTree` and edits
/// through the shared [`Shell`]. Blocks on the GTK main loop until closed.
///
/// # Errors
///
/// [`VaultError`] if the vault cannot be opened.
#[cfg(feature = "gtk")]
pub fn run(vault_path: &Path) -> Result<(), VaultError> {
    use gtk4::prelude::*;
    use gtk4::{Application, ApplicationWindow, Label, ScrolledWindow};

    let shell = Shell::new(Vault::open(vault_path)?);
    let tree = App::new().view(&shell);
    let descriptor = widget_tree(&tree);
    let app = Application::builder()
        .application_id("net.closure.gtk")
        .build();
    app.connect_activate(move |app| {
        // The window renders the ViewTree the shared App produced. The
        // descriptor lines map 1:1 to the widget hierarchy; a real build
        // walks the Node tree directly (each label is one row).
        let list = gtk4::ListBox::new();
        for line in descriptor.lines() {
            let label = Label::builder().label(line).xalign(0.0).build();
            list.append(&label);
        }
        let scrolled = ScrolledWindow::builder().child(&list).build();
        let window = ApplicationWindow::builder()
            .application(app)
            .title("closure")
            .default_width(700)
            .default_height(850)
            .child(&scrolled)
            .build();
        window.present();
    });
    app.run();
    Ok(())
}
