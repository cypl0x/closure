//! What the egui shell does in the build everybody has.
//!
//! `run` behind `#[cfg(not(feature = "egui"))]` is the whole crate in
//! the default configuration, and it had never been called. The default
//! build is deliberately egui-free (I10: the hermetic build must not
//! need system GL/X11), so this fallback *is* the shipped behaviour for
//! anyone who has not opted in.
//!
//! One function, one string, and the string is the only thing standing
//! between a user and a program that does nothing without saying why.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

#[cfg(not(feature = "egui"))]
#[test]
fn the_default_build_refuses_and_says_how_to_enable_it() {
    let err = closure_shell_egui::run(std::path::Path::new("/some/vault"))
        .expect_err("an egui-free build claimed to open a window");

    assert!(err.contains("egui"), "{err}");
    assert!(
        err.contains("--features"),
        "the message does not say how to fix it: {err}"
    );
}

#[cfg(not(feature = "egui"))]
#[test]
fn it_refuses_the_same_way_whatever_path_it_is_given() {
    // The path is irrelevant when there is no window to open. A
    // refusal that depended on the vault would be checking the wrong
    // thing first and reporting the wrong problem.
    for p in ["/nonexistent", "", "/tmp"] {
        assert!(
            closure_shell_egui::run(std::path::Path::new(p)).is_err(),
            "accepted {p:?}"
        );
    }
}

#[test]
fn the_shell_names_itself_for_the_capability_matrix() {
    // `closure shells` reads this constant. A column heading that
    // disagreed with the crate would be a matrix describing a shell
    // nobody can name.
    assert_eq!(closure_shell_egui::EGUI_SHELL, "egui");
}
