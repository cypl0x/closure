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

use closure_shell_core::{KeyEvent, Node};
use closure_store::{Vault, VaultError};

// Re-export the shared state core so the gtk shell drives the SAME tested
// App/Shell as every other shell (editor parity, not a private model).
pub use closure_shell_core::{App, Shell, browse_view};

/// Map a shared theme to a GTK4 CSS string (P5).
///
/// Window/label colours from the palette, plus selection + severity
/// classes (`.selected`, `.toast-error`, …). Hermetic; the windowed
/// [`run`] feeds it to a `CssProvider`.
#[must_use]
pub fn theme_css(theme: &closure_shell_core::Theme) -> String {
    use closure_shell_core::ColorRole::{Accent, Bg, Error, Fg, Selection, Success, Warning};
    let c = |role| theme.color(role).hex();
    format!(
        "window {{ background-color: {bg}; color: {fg}; }}\n\
         label {{ color: {fg}; }}\n\
         .selected {{ background-color: {sel}; }}\n\
         .accent {{ color: {accent}; }}\n\
         .toast-error {{ color: {error}; }}\n\
         .toast-warning {{ color: {warning}; }}\n\
         .toast-success {{ color: {success}; }}\n",
        bg = c(Bg),
        fg = c(Fg),
        sel = c(Selection),
        accent = c(Accent),
        error = c(Error),
        warning = c(Warning),
        success = c(Success),
    )
}

/// Apply one key event and return the GTK widget descriptor to repaint (P2).
///
/// Dispatches through the shared core (P1), then maps the fresh `ViewTree`
/// to [`widget_tree`]. The exact step the windowed [`run`] calls per
/// keypress — so the interactive loop is hermetically tested without a
/// display.
#[must_use]
pub fn next_frame(app: &mut App, shell: &mut Shell, event: &KeyEvent) -> String {
    widget_tree(&app.dispatch(shell, event))
}

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
                let icon = r.icon.as_deref().map_or_else(String::new, |g| format!("{g} "));
                let badges = if r.badges.is_empty() {
                    String::new()
                } else {
                    format!("  :{}:", r.badges.join(":"))
                };
                out.push(format!(
                    "{pad}  GtkLabel xalign=0{sel} \"{icon}{todo}{}{badges}\"",
                    r.title
                ));
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

/// Repaint a `ListBox` from a widget descriptor: one label per line.
#[cfg(feature = "gtk")]
fn populate(list: &gtk4::ListBox, descriptor: &str) {
    use gtk4::prelude::*;
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    for line in descriptor.lines() {
        list.append(&gtk4::Label::builder().label(line).xalign(0.0).build());
    }
}

/// Translate a GDK key + Ctrl modifier into a shared [`KeyEvent`] (P2).
/// Named keys map to their canonical names; printable keys carry the char.
#[cfg(feature = "gtk")]
fn gdk_key_to_event(keyval: gtk4::gdk::Key, ctrl: bool) -> Option<KeyEvent> {
    use gtk4::gdk::Key;
    match keyval {
        Key::Return | Key::KP_Enter => Some(KeyEvent::new("enter", ctrl, None)),
        Key::Escape => Some(KeyEvent::new("escape", ctrl, None)),
        Key::BackSpace => Some(KeyEvent::new("backspace", ctrl, None)),
        Key::Up => Some(KeyEvent::new("up", ctrl, None)),
        Key::Down => Some(KeyEvent::new("down", ctrl, None)),
        Key::Tab => Some(KeyEvent::new("tab", ctrl, None)),
        other => {
            let c = other.to_unicode()?;
            if ctrl {
                Some(KeyEvent::new(c.to_string(), true, None))
            } else {
                Some(KeyEvent::char(c))
            }
        }
    }
}

/// Open a native GTK4 window: an interactive editor over the shared
/// [`Shell`] (P2). Each keypress is translated to a [`KeyEvent`] and run
/// through [`next_frame`] (P1 dispatch + [`widget_tree`]); the list
/// repaints from the result. Blocks on the GTK main loop until closed.
///
/// # Errors
///
/// [`VaultError`] if the vault cannot be opened.
#[cfg(feature = "gtk")]
pub fn run(vault_path: &Path) -> Result<(), VaultError> {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gtk4::prelude::*;
    use gtk4::{
        Application, ApplicationWindow, EventControllerKey, ListBox, ScrolledWindow, glib,
    };

    let shell = Shell::new(Vault::open(vault_path)?);
    let state = Rc::new(RefCell::new((App::new(), shell)));
    let app = Application::builder()
        .application_id("net.closure.gtk")
        .build();
    app.connect_activate(move |app| {
        // Apply the shared theme tokens to the display (P5).
        if let Some(display) = gtk4::gdk::Display::default() {
            let provider = gtk4::CssProvider::new();
            provider.load_from_data(&theme_css(&closure_shell_core::Theme::dark()));
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let list = ListBox::new();
        // Initial paint of the browse ViewTree.
        {
            let st = state.borrow();
            populate(&list, &widget_tree(&st.0.view(&st.1)));
        }
        let keys = EventControllerKey::new();
        let state_k = Rc::clone(&state);
        let list_k = list.clone();
        keys.connect_key_pressed(move |_, keyval, _code, modifier| {
            let ctrl = modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
            if let Some(ev) = gdk_key_to_event(keyval, ctrl) {
                let mut st = state_k.borrow_mut();
                let (app_state, shell_state) = &mut *st;
                let frame = next_frame(app_state, shell_state, &ev);
                let quit = app_state.should_quit();
                drop(st);
                populate(&list_k, &frame);
                if quit {
                    std::process::exit(0);
                }
            }
            glib::Propagation::Proceed
        });
        let scrolled = ScrolledWindow::builder().child(&list).build();
        let window = ApplicationWindow::builder()
            .application(app)
            .title("closure")
            .default_width(700)
            .default_height(850)
            .child(&scrolled)
            .build();
        window.add_controller(keys);
        window.present();
    });
    app.run();
    Ok(())
}
