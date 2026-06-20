//! Native Qt6/QML desktop shell (X1c).
//!
//! A thin adapter: the vault's headlines become a Qt `ListView`. [`rows`]
//! and [`qml_document`] are the hermetic core (vault -> lines -> a QML
//! document) and are unit-tested in the default build; [`run`] (behind
//! the opt-in `qt` feature) loads that document in a real Qt window via
//! `qmetaobject`. The default build never pulls Qt (I10).
//!
//! Build/run with a Qt6 SDK on PATH (`qmake6`):
//! `cargo run -p closure-shell-qt --features qt -- <vault>`.

#![forbid(unsafe_code)]

use std::path::Path;

use closure_store::{Vault, VaultError};

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

/// Open a native Qt6 window listing the vault's headlines. Blocks on the
/// Qt event loop until the window is closed.
///
/// # Errors
///
/// [`VaultError`] if the vault cannot be opened.
#[cfg(feature = "qt")]
pub fn run(vault_path: &Path) -> Result<(), VaultError> {
    let qml = qml_document(&rows(vault_path)?);
    let mut engine = qmetaobject::QmlEngine::new();
    engine.load_data(qml.as_str().into());
    engine.exec();
    Ok(())
}
