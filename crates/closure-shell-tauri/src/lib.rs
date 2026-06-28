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

/// The LIVE editor surface (P4): the web shell's served root page — the one
/// with the capture form + fuzzy search, whose `POST /capture` writes back
/// through the registry (I8).
///
/// This is what the interactive window hosts (via a local server), as
/// opposed to the read-only [`page`] export. Pure + hermetic: built from
/// the tested `closure_shell_web::respond`, so the interactive payload is
/// verifiable without a window or socket.
///
/// # Errors
///
/// [`VaultError`] if the vault cannot be opened.
pub fn interactive_page(vault_path: &Path) -> Result<String, VaultError> {
    let mut vault = Vault::open(vault_path)?;
    Ok(closure_shell_web::respond(&mut vault, "GET", "/", "").body)
}

/// The localhost address the interactive window serves the vault on (P4).
pub const SERVE_ADDR: &str = "127.0.0.1:8787";

/// Open a native window hosting the LIVE editor (P4): a local server
/// serves the vault on [`SERVE_ADDR`] (so `POST /capture` round-trips to
/// the registry, I8) and the webview loads its URL. Blocks on the event
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

    // Open once up front so a bad vault fails before the window opens.
    let _ = Vault::open(vault_path).map_err(|e| e.to_string())?;

    // Serve the interactive surface on localhost in a background thread.
    let serve_path = vault_path.to_path_buf();
    std::thread::spawn(move || {
        if let Ok(mut vault) = Vault::open(&serve_path) {
            let _ = closure_shell_web::serve(&mut vault, SERVE_ADDR);
        }
    });
    let url = format!("http://{SERVE_ADDR}/");

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("closure")
        .build(&event_loop)
        .map_err(|e| e.to_string())?;
    let _webview = WebViewBuilder::new(&window)
        .with_url(&url)
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
