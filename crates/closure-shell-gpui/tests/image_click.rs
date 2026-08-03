//! "Clicking a on image should show this popup with the image in full
//! size. Just like enter does."
//!
//! `RET` on an image link opens the viewer; the picture painted in the
//! preview was not a click target at all, so the obvious gesture did
//! nothing. Everything the keyboard can do the mouse should be able to
//! do — the picture runs the same `show_image` the key does.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::ModalSurface;
use closure_shell_gpui::visual_window;

/// A note whose body carries an image link, and an image to resolve it
/// to — a link with nothing behind it paints nothing, by design.
fn vault_with_image(dir: &std::path::Path) -> String {
    let assets = dir.join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    // A 1x1 PNG, enough to exist and decode.
    let png: [u8; 67] = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(assets.join("shot.png"), png).unwrap();
    "* Alpha\n:PROPERTIES:\n:ID: 01HQIMG0000000000000000AA\n:END:\n\
     [[file:assets/shot.png]]\n"
        .to_owned()
}

#[gpui::test]
fn clicking_a_picture_opens_it_full_size(cx: &mut gpui::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let org = vault_with_image(dir.path());
    std::fs::write(dir.path().join("notes.org"), &org).unwrap();

    let (_d, view, vcx) = visual_window(cx, &org);
    // The window under test owns its own vault; put the asset there too
    // so the link resolves for it.
    let root = view.update(vcx, |v, _cx| v.vault_root());
    let assets = root.join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::copy(dir.path().join("assets/shot.png"), assets.join("shot.png")).unwrap();
    // The pictures are painted between the buffer's lines, so the
    // buffer has to be open — and inline images on, which is their
    // default but is stated here so the test does not depend on it.
    view.update(vcx, |v, cx| v.run_command("edit-body", cx));
    vcx.run_until_parked();

    let bounds = vcx
        .debug_bounds("picture-shot.png")
        .expect("the picture is painted and clickable");
    vcx.simulate_click(bounds.center(), gpui::Modifiers::none());
    vcx.run_until_parked();

    view.update(vcx, |v, _cx| {
        assert_eq!(
            v.surface(),
            ModalSurface::ImageView,
            "clicking the picture did not open the viewer"
        );
    });
}
