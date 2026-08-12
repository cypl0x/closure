//! The ✕ beside a peer removes it, and removes the right one.
//!
//! One of the seventeen affordances the window painted and no test ever
//! looked at — and the one that deletes something. A paired machine is
//! written into `config.org`; forgetting the wrong row unpairs a laptop
//! somebody still uses, and nothing would have failed.
//!
//! `at` is an index into the peer list, captured per row when the row
//! is built. An off-by-one or a stale capture is invisible until it
//! deletes the wrong pairing, which is exactly the class of bug a
//! click-target test exists for.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const VAULT: &str = "\
* A note
:PROPERTIES:
:ID: 01PEERFORGET00000001
:END:
";

/// Three paired peers, added through the real ticket path rather than
/// by reaching into the peer list — a ticket is what a user pastes, so
/// a test that bypassed it would not prove pairing works.
fn paired(v: &mut closure_shell_gpui::GpuiView) {
    for (i, addr) in ["127.0.0.1:7001", "127.0.0.1:7002", "127.0.0.1:7003"]
        .iter()
        .enumerate()
    {
        // A distinct key per peer: `add_peer` ignores a ticket whose
        // key it already knows, so three tickets sharing one key would
        // silently produce a single peer and the test would pass on a
        // list of one.
        let key = closure_sync::SigningKey::from_bytes(&[u8::try_from(i).unwrap_or(0) + 1; 32])
            .verifying_key();
        let ticket = closure_sync::SyncTicket {
            addr: addr.parse().expect("addr"),
            pubkey: key,
        };
        v.app_mut()
            .sync_mut()
            .add_peer(&ticket.encode())
            .expect("paired");
    }
}

#[gpui::test]
fn the_button_is_painted_for_every_peer(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| {
        paired(v);
        v.run_command("sync", cx);
    });
    vcx.run_until_parked();
    for selector in ["peer-forget-0", "peer-forget-1", "peer-forget-2"] {
        assert!(
            vcx.debug_bounds(selector).is_some(),
            "no way to forget {selector}"
        );
    }
}

#[gpui::test]
fn forgetting_removes_that_peer_and_not_another(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| {
        paired(v);
        v.run_command("sync", cx);
    });
    vcx.run_until_parked();
    let before: Vec<String> = view.update(vcx, |v, _| {
        v.app()
            .sync()
            .expect("sync")
            .peers()
            .iter()
            .map(|p| p.addr.to_string())
            .collect()
    });
    assert_eq!(before.len(), 3);
    // The middle one, because an off-by-one that takes a neighbour is
    // the bug this is for and either end would hide it.
    view.update(vcx, |v, _cx| v.app_mut().sync_mut().forget_peer(1));
    vcx.run_until_parked();
    let after: Vec<String> = view.update(vcx, |v, _| {
        v.app()
            .sync()
            .expect("sync")
            .peers()
            .iter()
            .map(|p| p.addr.to_string())
            .collect()
    });
    assert_eq!(after, vec![before[0].clone(), before[2].clone()]);
}

#[gpui::test]
fn a_vault_with_no_peers_paints_no_forget_buttons(cx: &mut gpui::TestAppContext) {
    let (_dir, view, vcx) = visual_window(cx, VAULT);
    view.update(vcx, |v, cx| v.run_command("sync", cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("peer-forget-0").is_none());
}
