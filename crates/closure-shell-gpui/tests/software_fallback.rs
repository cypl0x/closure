//! Adjacent polish, found while building the flatpak.
//!
//! The preflight asks "is there a Vulkan driver on this machine" and
//! runs if there is. Inside the flatpak there always is — the runtime
//! ships six ICDs — and the window still failed, because the *display*
//! could not present: Xvfb has no DRI3. So the flatpak had to be run
//! with `--env=VK_ICD_FILENAMES=…lvp_icd…` by hand, which is a caveat
//! nobody will remember.
//!
//! A driver existing and a surface working are different questions and
//! only one of them can be asked in advance. So the answer to the
//! second is the failure itself: if the window cannot make a surface,
//! look for a software rasteriser among the ICDs actually present and
//! start again on that — which is what the preflight already does when
//! it finds no driver at all.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::software_icd_in;

fn dir_with(files: &[&str]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for f in files {
        std::fs::write(d.path().join(f), "{}").unwrap();
    }
    d
}

#[test]
fn lavapipe_is_found_among_the_drivers_a_runtime_ships() {
    // The freedesktop runtime's own icd.d, verbatim.
    let d = dir_with(&[
        "intel_hasvk_icd.x86_64.json",
        "intel_icd.x86_64.json",
        "lvp_icd.x86_64.json",
        "nouveau_icd.x86_64.json",
        "radeon_icd.x86_64.json",
        "virtio_icd.x86_64.json",
    ]);
    let found = software_icd_in(&[d.path().to_path_buf()]).expect("lavapipe is in there");
    assert!(
        found.file_name().unwrap().to_string_lossy().contains("lvp"),
        "picked {found:?} rather than the software rasteriser"
    );
}

#[test]
fn a_machine_with_only_real_drivers_has_no_software_one() {
    // Nothing to fall back to is a different answer from "here is one".
    let d = dir_with(&["radeon_icd.x86_64.json", "intel_icd.x86_64.json"]);
    assert!(software_icd_in(&[d.path().to_path_buf()]).is_none());
}

#[test]
fn the_other_spelling_counts_too() {
    // Mesa has called it both.
    let d = dir_with(&["lavapipe_icd.x86_64.json"]);
    assert!(software_icd_in(&[d.path().to_path_buf()]).is_some());
}

#[test]
fn a_directory_that_is_not_there_is_not_an_error() {
    assert!(software_icd_in(&[std::path::PathBuf::from("/nonexistent/icd.d")]).is_none());
}
