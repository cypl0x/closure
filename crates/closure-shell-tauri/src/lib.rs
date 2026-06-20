//! Native webview desktop shell (X1a).
//!
//! A thin adapter that hosts closure's self-contained web UI
//! ([`closure_shell_web::export_html`]) inside a native window via
//! `wry`/`tao` — the same webview foundation Tauri is built on. The page
//! generation is the (already-tested) hermetic core; only the window is
//! display-bound, so it lives behind the opt-in `tauri` feature and the
//! default build never pulls the webview stack (I10).
//!
//! Build/run with the webview toolchain:
//! `nix develop .#webview -c cargo run -p closure-shell-tauri --features tauri -- <vault>`.

#![forbid(unsafe_code)]

use std::path::Path;

use closure_store::{Vault, VaultError};

/// Render the vault's self-contained HTML page (the webview content).
///
/// Pure + hermetic (reuses the web shell's tested `export_html`), so the
/// shell's payload is verifiable without a window.
///
/// # Errors
///
/// [`VaultError`] if the vault cannot be opened.
pub fn page(vault_path: &Path) -> Result<String, VaultError> {
    let vault = Vault::open(vault_path)?;
    Ok(closure_shell_web::export_html(&vault))
}

/// Open a native window showing the vault's web UI. Blocks on the event
/// loop until the window is closed.
///
/// # Errors
///
/// [`VaultError`] if the vault cannot be opened; a string on window /
/// webview construction failure.
#[cfg(feature = "tauri")]
pub fn run(vault_path: &Path) -> Result<(), String> {
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tao::window::WindowBuilder;
    use wry::WebViewBuilder;

    let html = page(vault_path).map_err(|e| e.to_string())?;

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("closure")
        .build(&event_loop)
        .map_err(|e| e.to_string())?;
    let _webview = WebViewBuilder::new(&window)
        .with_html(html)
        .build()
        .map_err(|e| e.to_string())?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}
