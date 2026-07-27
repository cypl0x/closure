//! Telling a vault where its sync socket lives.
//!
//! Pairing was loopback-only: the listener bound `127.0.0.1:7420` and
//! the ticket said so, which made a ticket handed to another machine a
//! lie — the peer dialled its *own* loopback and reached itself. Two
//! instances on one box paired; two people never could.
//!
//! Two settings fix that, and they are deliberately separate because
//! they answer different questions. `sync_bind` is which socket to
//! open (`0.0.0.0:7420` to accept from the network at all).
//! `sync_advertise` is which address a peer should dial — the one that
//! goes in the ticket. They differ whenever the machine has more than
//! one route to it: a Tailscale `100.x` address is reachable, the
//! `192.168.x` on the same host is not, and only the operator knows
//! which one the peer is on.
//!
//! Both are validated at load time (I9): a typo in an address is an
//! error when `config.org` is read, not a bind failure discovered the
//! moment someone tries to pair.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_config::{Config, ConfigError};

fn parse(body: &str) -> Result<Config, ConfigError> {
    Config::from_org_source(&format!("#+BEGIN_SRC closure-config\n{body}\n#+END_SRC\n"))
}

#[test]
fn both_sync_addresses_are_unset_by_default() {
    let cfg = Config::default();
    assert!(cfg.sync_bind.is_none(), "no socket is pinned by config");
    assert!(
        cfg.sync_advertise.is_none(),
        "and nothing is advertised until asked"
    );
}

#[test]
fn a_bind_address_is_parsed_as_a_socket_address() {
    let cfg = parse("sync_bind = 0.0.0.0:7420").expect("loads");
    assert_eq!(
        cfg.sync_bind.expect("set").to_string(),
        "0.0.0.0:7420",
        "host and port both survive"
    );
}

#[test]
fn an_advertise_address_is_parsed_as_a_bare_ip() {
    // The port a peer dials is the port we bound, so the ticket takes
    // it from the socket; what config supplies is *which of our
    // addresses* is the reachable one.
    let cfg = parse("sync_advertise = 100.101.102.103").expect("loads");
    assert_eq!(
        cfg.sync_advertise.expect("set").to_string(),
        "100.101.102.103"
    );
}

#[test]
fn an_ipv6_advertise_address_is_accepted() {
    let cfg = parse("sync_advertise = fd7a:115c:a1e0::1").expect("loads");
    assert_eq!(
        cfg.sync_advertise.expect("set").to_string(),
        "fd7a:115c:a1e0::1"
    );
}

#[test]
fn a_bind_address_without_a_port_is_a_load_error() {
    // `0.0.0.0` alone cannot be bound — refusing it here is the whole
    // point of typed config (I9).
    let err = parse("sync_bind = 0.0.0.0").expect_err("rejected");
    assert!(
        matches!(&err, ConfigError::BadValue { key, .. } if key == "sync_bind"),
        "{err}"
    );
}

#[test]
fn an_advertise_address_carrying_a_port_is_a_load_error() {
    // The port is not the operator's to choose here: it is whatever
    // the listener actually bound. Silently dropping it would produce
    // a ticket that disagrees with the config.
    let err = parse("sync_advertise = 100.101.102.103:7420").expect_err("rejected");
    assert!(
        matches!(&err, ConfigError::BadValue { key, .. } if key == "sync_advertise"),
        "{err}"
    );
}

#[test]
fn a_garbled_address_is_a_load_error() {
    let err = parse("sync_bind = not-an-address").expect_err("rejected");
    assert!(
        matches!(&err, ConfigError::BadValue { key, .. } if key == "sync_bind"),
        "{err}"
    );
}

#[test]
fn the_two_keys_coexist_with_the_rest_of_the_config() {
    let cfg = parse(
        "input_mode = doom\n\
         sync_bind = 0.0.0.0:7420\n\
         sync_advertise = 192.168.1.42\n\
         theme = doom-vibrant",
    )
    .expect("loads");
    assert_eq!(cfg.sync_bind.expect("bind").port(), 7420);
    assert_eq!(cfg.sync_advertise.expect("adv").to_string(), "192.168.1.42");
    assert_eq!(cfg.theme, "doom-vibrant");
}
