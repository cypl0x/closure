//! OS-clipboard bridge (D7).
//!
//! The kill ring (`Vault`) is the hub. A [`Clipboard`] adapter mirrors its
//! top entry *out* to an external clipboard and pulls external text *in*;
//! cut/paste never depend on a clipboard, so the bridge is additive. The
//! hermetic default is [`MemoryClipboard`]; a real system clipboard is
//! [`SystemClipboard`], compiled only under the `clipboard` feature
//! (it shells out to the platform tool at runtime).

/// A minimal clipboard surface: set the contents, read them back.
pub trait Clipboard {
    /// Replace the clipboard contents with `text`.
    fn set(&mut self, text: &str);
    /// The current clipboard contents, or `None` if empty/unavailable.
    fn get(&self) -> Option<String>;
}

/// Hermetic in-memory clipboard — the default. Holds one slot; no I/O, so
/// it works identically on every platform and in tests.
#[derive(Debug, Default, Clone)]
pub struct MemoryClipboard {
    slot: Option<String>,
}

impl Clipboard for MemoryClipboard {
    fn set(&mut self, text: &str) {
        self.slot = Some(text.to_owned());
    }

    fn get(&self) -> Option<String> {
        self.slot.clone()
    }
}

/// A real OS clipboard, behind the `clipboard` feature.
///
/// Shells out to the platform clipboard tool — no extra crate, so the
/// build stays hermetic (I10); only the *runtime* needs the tool present.
/// `copy_cmd` writes stdin to the clipboard (e.g. `wl-copy`, `xclip
/// -selection clipboard`, `pbcopy`); `paste_cmd` prints it to stdout
/// (`wl-paste`, `xclip -selection clipboard -o`, `pbpaste`).
#[cfg(feature = "clipboard")]
#[derive(Debug, Clone)]
pub struct SystemClipboard {
    copy_cmd: Vec<String>,
    paste_cmd: Vec<String>,
}

#[cfg(feature = "clipboard")]
impl SystemClipboard {
    /// A clipboard driven by explicit `copy`/`paste` argv vectors. Lets
    /// the caller pick `wl-copy`/`xclip`/`pbcopy` for their platform.
    #[must_use]
    pub const fn new(copy_cmd: Vec<String>, paste_cmd: Vec<String>) -> Self {
        Self {
            copy_cmd,
            paste_cmd,
        }
    }

    /// Wayland defaults: `wl-copy` / `wl-paste --no-newline`.
    #[must_use]
    pub fn wayland() -> Self {
        Self::new(
            vec!["wl-copy".to_owned()],
            vec!["wl-paste".to_owned(), "--no-newline".to_owned()],
        )
    }
}

#[cfg(feature = "clipboard")]
impl Clipboard for SystemClipboard {
    fn set(&mut self, text: &str) {
        use std::io::Write as _;
        let Some((prog, args)) = self.copy_cmd.split_first() else {
            return;
        };
        if let Ok(mut child) = std::process::Command::new(prog)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
    }

    fn get(&self) -> Option<String> {
        let (prog, args) = self.paste_cmd.split_first()?;
        let out = std::process::Command::new(prog).args(args).output().ok()?;
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            None
        }
    }
}
