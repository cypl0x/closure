//! "add a button to the Assistant UI (g i) which calls assistant setup
//! if you press it."
//!
//! The assistant pane is where you find out the assistant is not
//! configured, so it is where the way to configure it belongs. The
//! button runs the same registry command the chord does (I8) rather
//! than reaching into the surface itself.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::ModalSurface;
use closure_shell_gpui::visual_window;

const VAULT: &str = "* Alpha\n:PROPERTIES:\n:ID: 01HQSETUP000000000000000A\n:END:\nbody\n";

#[gpui::test]
fn the_assistant_pane_offers_a_way_to_configure_it(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("llm", cx));
    vcx.run_until_parked();
    view.update(vcx, |v, _cx| {
        assert_eq!(v.surface(), ModalSurface::Llm, "the pane did not open");
    });
    assert!(
        vcx.debug_bounds("llm-setup").is_some(),
        "there is no setup button on the assistant pane"
    );
}

#[gpui::test]
fn pressing_it_opens_the_setup_screen(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("llm", cx));
    vcx.run_until_parked();

    let button = vcx
        .debug_bounds("llm-setup")
        .expect("the button is painted");
    vcx.simulate_click(button.center(), gpui::Modifiers::none());
    vcx.run_until_parked();

    view.update(vcx, |v, _cx| {
        assert_eq!(
            v.surface(),
            ModalSurface::Settings,
            "the button did not open the setup screen"
        );
    });
}
