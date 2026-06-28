//! Thin launcher for the Qt6/QML shell: `closure-shell-qt <vault>`.
//!
//! The window itself lives behind the `qt` feature (so the default build
//! stays Qt-free, I10); without it the binary prints how to enable it.

#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    let Some(vault) = std::env::args().nth(1) else {
        eprintln!("usage: closure-shell-qt <vault>");
        return std::process::ExitCode::FAILURE;
    };
    #[cfg(feature = "qt")]
    {
        match closure_shell_qt::run(std::path::Path::new(&vault)) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                std::process::ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(feature = "qt"))]
    {
        let _ = vault;
        eprintln!("built without the `qt` feature — rebuild with `--features qt`");
        std::process::ExitCode::FAILURE
    }
}
