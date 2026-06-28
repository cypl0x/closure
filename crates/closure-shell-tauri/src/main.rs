//! Thin launcher for the Tauri/webview shell: `closure-shell-tauri <vault>`.
//!
//! The window itself lives behind the `tauri` feature (so the default
//! build stays webview-free, I10); without it the binary prints how to
//! enable it.

#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    let Some(vault) = std::env::args().nth(1) else {
        eprintln!("usage: closure-shell-tauri <vault>");
        return std::process::ExitCode::FAILURE;
    };
    #[cfg(feature = "tauri")]
    {
        match closure_shell_tauri::run(std::path::Path::new(&vault)) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                std::process::ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(feature = "tauri"))]
    {
        let _ = vault;
        eprintln!("built without the `tauri` feature — rebuild with `--features tauri`");
        std::process::ExitCode::FAILURE
    }
}
