//! Slint shell (Q13): renders the shared `closure_shell_core`
//! `ViewTree` to a `.slint` UI document — the same declarative `Node`
//! tree every other shell renders, answering the vision's "Slint?"
//! question with working code on both sides: our own composable
//! `ViewTree` AND a Slint renderer of it.
//!
//! `slint_view` is hermetic and exhaustive over every [`Node`] kind
//! (a new kind is a compile error here, the G9 rule). Edits route
//! through the shared `Shell` (I8) — proven headlessly; a windowed
//! embedder loading the document with the real Slint runtime is a
//! display-bound, feature-gated follow-up (the gtk/qt recipe).

#![forbid(unsafe_code)]

use closure_shell_core::Node;

/// Render a [`Node`] tree as a `.slint` UI document.
#[must_use]
pub fn slint_view(node: &Node) -> String {
    let mut out = vec![
        "import { VerticalBox, HorizontalBox, ListView } from \"std-widgets.slint\";".to_owned(),
        "export component Main inherits Window {".to_owned(),
    ];
    push_element(node, 1, &mut out);
    out.push("}".to_owned());
    out.join("\n")
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// One flat arm per `Node` kind — exhaustive by design (V1c); splitting
// the match would only hide the one-to-one kind→Slint mapping. Same
// precedent as `closure_shell_qt::qml_item`.
#[allow(clippy::too_many_lines)]
fn push_element(node: &Node, depth: usize, out: &mut Vec<String>) {
    let pad = "    ".repeat(depth);
    match node {
        Node::Pane { title, children } => {
            out.push(format!("{pad}GroupBox {{ title: \"{}\";", esc(title)));
            out.push(format!("{pad}    VerticalBox {{"));
            for c in children {
                push_element(c, depth + 2, out);
            }
            out.push(format!("{pad}    }}"));
            out.push(format!("{pad}}}"));
        }
        Node::Rows { rows, selected } => {
            out.push(format!("{pad}ListView {{"));
            for (i, r) in rows.iter().enumerate() {
                let sel = if i == *selected { " // selected" } else { "" };
                let todo = r
                    .todo
                    .as_deref()
                    .map_or_else(String::new, |t| format!("{t} "));
                let icon = r
                    .icon
                    .as_deref()
                    .map_or_else(String::new, |g| format!("{g} "));
                let badges = if r.badges.is_empty() {
                    String::new()
                } else {
                    format!("  :{}:", r.badges.join(":"))
                };
                out.push(format!(
                    "{pad}    Text {{ text: \"{}\"; }}{sel}",
                    esc(&format!("{icon}{todo}{}{badges}", r.title))
                ));
            }
            out.push(format!("{pad}}}"));
        }
        Node::Detail { fields } => {
            out.push(format!("{pad}GridLayout {{"));
            for f in fields {
                let kbd = f
                    .action
                    .as_ref()
                    .map_or_else(String::new, |a| format!("  [{}]", a.chord()));
                out.push(format!(
                    "{pad}    Text {{ text: \"{}\"; }}",
                    esc(&format!("{}: {}{kbd}", f.label, f.value))
                ));
            }
            out.push(format!("{pad}}}"));
        }
        Node::Input { label, buffer } => {
            out.push(format!(
                "{pad}LineEdit {{ placeholder-text: \"{}\"; text: \"{}\"; }}",
                esc(label),
                esc(buffer)
            ));
        }
        Node::Palette { items, cursor } => {
            out.push(format!("{pad}ListView {{ // palette"));
            for (i, it) in items.iter().enumerate() {
                let sel = if i == *cursor { " // selected" } else { "" };
                out.push(format!(
                    "{pad}    Text {{ text: \"{}\"; }}{sel}",
                    esc(&format!("[{}] {}", it.action.chord(), it.label))
                ));
            }
            out.push(format!("{pad}}}"));
        }
        Node::Hints { line } => {
            out.push(format!("{pad}Text {{ text: \"{}\"; }} // hints", esc(line)));
        }
        Node::Widget { name, content } => {
            out.push(format!("{pad}GroupBox {{ title: \"{}\";", esc(name)));
            for l in content.lines() {
                out.push(format!("{pad}    Text {{ text: \"{}\"; }}", esc(l)));
            }
            out.push(format!("{pad}}}"));
        }
        Node::Text(t) => out.push(format!("{pad}Text {{ text: \"{}\"; }}", esc(t))),
        Node::Split { direction, panes } => {
            let container = match direction {
                closure_shell_core::SplitDir::Row => "HorizontalBox",
                closure_shell_core::SplitDir::Column => "VerticalBox",
            };
            out.push(format!("{pad}{container} {{"));
            for p in panes {
                push_element(p, depth + 1, out);
            }
            out.push(format!("{pad}}}"));
        }
        Node::Modal { title, body } => {
            out.push(format!("{pad}PopupWindow {{ // modal: {}", esc(title)));
            push_element(body, depth + 1, out);
            out.push(format!("{pad}}}"));
        }
        Node::Toast { level, text } => {
            out.push(format!(
                "{pad}Text {{ text: \"{}\"; }} // toast-{}",
                esc(text),
                level.as_str()
            ));
        }
    }
}
