//! Native GTK4 desktop shell (X1b).
//!
//! A thin adapter: the vault's headlines become a scrollable GTK4 list.
//! [`rows`] (the hermetic core) turns a vault into the display lines and
//! is unit-tested in the default build; [`run`] (behind the opt-in `gtk`
//! feature) draws them in a real window. The default build never pulls
//! GTK (I10).
//!
//! Build/run with the GUI toolchain:
//! `nix develop .#webview -c cargo run -p closure-shell-gtk --features gtk -- <vault>`.

#![forbid(unsafe_code)]

use std::path::Path;

use closure_store::{Vault, VaultError};

/// The display lines for a vault: every headline as `indent + TODO +
/// title`, in document order. Pure + hermetic — the shell's content is
/// verifiable without a window.
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

/// Open a native GTK4 window listing the vault's headlines. Blocks on the
/// GTK main loop until the window is closed.
///
/// # Errors
///
/// [`VaultError`] if the vault cannot be opened.
#[cfg(feature = "gtk")]
pub fn run(vault_path: &Path) -> Result<(), VaultError> {
    use gtk4::prelude::*;
    use gtk4::{Application, ApplicationWindow, Label, ListBox, ScrolledWindow};

    let lines = rows(vault_path)?;
    let app = Application::builder()
        .application_id("net.closure.gtk")
        .build();
    app.connect_activate(move |app| {
        let list = ListBox::new();
        for line in &lines {
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
