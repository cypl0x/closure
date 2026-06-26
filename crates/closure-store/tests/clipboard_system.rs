//! D7: the real `SystemClipboard` (behind the `clipboard` feature). It
//! shells out to platform argv, so here we back it with a temp file via
//! `sh -c` — a genuine process round trip, hermetic (coreutils only), no
//! wl-copy/xclip needed in CI.

#![cfg(feature = "clipboard")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_store::{Clipboard, SystemClipboard};

#[test]
fn system_clipboard_round_trips_via_a_process() {
    let dir = tempfile::tempdir().expect("tmp");
    let file = dir.path().join("clip");
    let f = file.display().to_string();

    // copy: read stdin into the file; paste: print the file.
    let mut clip = SystemClipboard::new(
        vec!["sh".into(), "-c".into(), format!("cat > {f}")],
        vec!["sh".into(), "-c".into(), format!("cat {f}")],
    );

    clip.set("ring contents");
    assert_eq!(
        clip.get().as_deref(),
        Some("ring contents"),
        "value survived the copy/paste process round trip"
    );
}
