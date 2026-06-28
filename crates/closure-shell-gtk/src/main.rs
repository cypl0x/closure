//! Thin launcher for the GTK4 shell: `closure-shell-gtk <vault>`.
//!
//! The window itself lives behind the `gtk` feature (so the default build
//! stays GTK-free, I10); without it the binary prints how to enable it.

#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    let Some(vault) = std::env::args().nth(1) else {
        eprintln!("usage: closure-shell-gtk <vault>");
        return std::process::ExitCode::FAILURE;
    };
    #[cfg(feature = "gtk")]
    {
        match closure_shell_gtk::run(std::path::Path::new(&vault)) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                std::process::ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(feature = "gtk"))]
    {
        let _ = vault;
        eprintln!("built without the `gtk` feature — rebuild with `--features gtk`");
        std::process::ExitCode::FAILURE
    }
}
