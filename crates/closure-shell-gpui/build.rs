//! Copy the bundled font faces into `OUT_DIR`, when there are any.
//!
//! Maple Mono NF is OFL-1.1 and so may be redistributed, but a face is
//! two and a half megabytes and five of them do not belong in a git
//! history. The flake points `CLOSURE_FONT_DIR` at the nixpkgs
//! derivation instead, so the bytes are pinned by the lockfile like
//! every other input and the binary still ships self-contained.
//!
//! Without that variable the build produces no embed at all and the
//! shell falls back to the system font stack — which is what it did
//! before, so a plain `cargo build` keeps working.

use std::{env, fs, path::PathBuf};

/// The faces worth their bytes: prose, emphasis, and headings.
const FACES: [&str; 5] = ["Regular", "Italic", "Bold", "BoldItalic", "SemiBold"];

fn main() {
    println!("cargo::rerun-if-env-changed=CLOSURE_FONT_DIR");
    println!("cargo::rustc-check-cfg=cfg(bundled_fonts)");
    let Ok(dir) = env::var("CLOSURE_FONT_DIR") else {
        return;
    };
    let Ok(out) = env::var("OUT_DIR").map(PathBuf::from) else {
        panic!("cargo did not set OUT_DIR");
    };
    let src = PathBuf::from(&dir);
    for face in FACES {
        let from = src.join(format!("MapleMono-NF-{face}.ttf"));
        println!("cargo::rerun-if-changed={}", from.display());
        // A missing face is a broken bundle, not a quiet degradation:
        // the shell would ask for a weight that is not there and look
        // exactly as it did before the bundling.
        let bytes = fs::read(&from)
            .unwrap_or_else(|e| panic!("CLOSURE_FONT_DIR has no {}: {e}", from.display()));
        if let Err(e) = fs::write(out.join(format!("{face}.ttf")), bytes) {
            panic!("could not stage {face}: {e}");
        }
    }
    println!("cargo::rustc-cfg=bundled_fonts");
}
