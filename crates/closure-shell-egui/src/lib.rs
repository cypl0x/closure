//! Native desktop shell built on egui + eframe.
//!
//! I7: consumes [`closure_core`] / [`closure_store`] only. No direct
//! `closure_org` access. Spans never cross the shell boundary.
//!
//! M8 skeleton: an abstract [`ShellAdapter`] trait that an eframe
//! embedder implements. The kernel-side `Shell` holds the loaded
//! `Vault` and the active selection; the adapter renders. This keeps
//! egui / eframe out of the kernel build until the desktop shell ships.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use closure_store::Vault;

/// Selection state shared between the shell and its adapter.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// Currently-focused file, if any.
    pub file: Option<PathBuf>,
    /// Index into the headline list of the focused file.
    pub headline: usize,
}

/// Kernel-side shell state. Owns the [`Vault`] and the [`Selection`];
/// receives input events from a [`ShellAdapter`] and exposes them to
/// the command registry.
pub struct Shell {
    /// Loaded vault.
    pub vault: Vault,
    /// Current selection.
    pub selection: Selection,
}

impl Shell {
    /// Build a shell over an already-loaded vault.
    #[must_use]
    pub const fn new(vault: Vault) -> Self {
        Self {
            vault,
            selection: Selection {
                file: None,
                headline: 0,
            },
        }
    }

    /// Capture (org-capture style) into the vault (parity with TUI/CLI).
    /// Uses a default inbox + TODO prefix for the parity slice.
    pub fn capture(&mut self, title: &str) -> Result<(), closure_store::VaultError> {
        let template = closure_store::CaptureTemplate {
            target: std::path::PathBuf::from("inbox.org"),
            headline_prefix: "TODO ".to_owned(),
            body: String::new(),
        };
        self.vault.capture(&template, title).map(|_| ())
    }

    /// Basic browse support: select a file by path (updates selection for render parity).
    pub fn select_file(&mut self, path: Option<std::path::PathBuf>) {
        self.selection.file = path;
        self.selection.headline = 0;
    }

    /// Simple fuzzy search over headlines (re-uses query crate for parity).
    /// Returns (path, title) matches for the embedder to render.
    #[must_use]
    pub fn fuzzy_search(&self, q: &str) -> Vec<(std::path::PathBuf, String)> {
        let mut scored: Vec<(u32, std::path::PathBuf, String)> = vec![];
        for (p, doc) in self.vault.iter() {
            for h in doc.all_headlines() {
                if let Some(sc) = closure_query::fuzzy_score(q, h.title()) {
                    scored.push((sc, p.to_path_buf(), h.title().to_owned()));
                }
            }
        }
        scored.sort_by_key(|(sc, _, _)| std::cmp::Reverse(*sc));
        scored.into_iter().map(|(_, p, t)| (p, t)).take(20).collect()
    }
}

/// Adapter trait. An embedder (eframe / native window / wasm canvas)
/// implements this and drives the kernel-side [`Shell`].
pub trait ShellAdapter {
    /// Render one frame for the current shell state.
    fn frame(&mut self, shell: &Shell);
    /// Handle a single typed key chord.
    fn input(&mut self, shell: &mut Shell, chord: &str);
}

/// Headless adapter for tests: counts frames and forwards key chords
/// without rendering.
#[derive(Debug, Default)]
pub struct HeadlessAdapter {
    /// Number of `frame` calls.
    pub frames: u64,
    /// Last chord seen by `input`.
    pub last_chord: Option<String>,
}

impl ShellAdapter for HeadlessAdapter {
    fn frame(&mut self, _shell: &Shell) {
        self.frames += 1;
    }

    fn input(&mut self, _shell: &mut Shell, chord: &str) {
        self.last_chord = Some(chord.to_owned());
    }
}
