//! The frame decoder, given frames that are wrong.
//!
//! `parse_candidate` walks an Ethernet header into an IPv4 header into
//! a TCP/UDP header using `?` on every field, and the existing test
//! feeds it only well-formed frames. That is the wrong half to test
//! exclusively: this function's entire input comes off a wire, where
//! truncation, padding and protocols it does not handle are not edge
//! cases but the normal traffic on any interface.
//!
//! Every `?` here is a length check. A missed one is an out-of-bounds
//! read on attacker-adjacent data, so "returns None" is the whole
//! contract and each way of being too short deserves its own case.
//!
//! Also covered: the `match_rule` default on the trait, which exists so
//! a backend that cannot say which rule decided is not forced to
//! invent one.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_sniffer::{Action, CaptureBackend, log_capture_to_org, parse_candidate};

/// A well-formed Ethernet + IPv4 + L4 frame.
fn frame(proto: u8, dst: [u8; 4], dst_port: u16) -> Vec<u8> {
    let mut f = vec![0u8; 12];
    f.extend_from_slice(&[0x08, 0x00]); // ethertype IPv4
    f.push(0x45); // version 4, IHL 5
    f.extend_from_slice(&[0x00, 0, 0, 0, 0, 0, 0, 0x40]);
    f.push(proto);
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(&[10, 0, 0, 1]);
    f.extend_from_slice(&dst);
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(&dst_port.to_be_bytes());
    f
}

#[test]
fn a_good_frame_still_decodes() {
    // The baseline, so the cases below are known to be failing for the
    // reason claimed rather than because the fixture drifted.
    assert_eq!(
        parse_candidate(&frame(6, [93, 184, 216, 34], 443)).as_deref(),
        Some("93.184.216.34:443 tcp")
    );
}

#[test]
fn a_frame_too_short_for_an_ethertype_is_none() {
    // The first `?`. A runt frame is a real thing on a real interface.
    for len in 0..14 {
        let f = vec![0u8; len];
        assert_eq!(parse_candidate(&f), None, "a {len}-byte frame decoded");
    }
}

#[test]
fn a_frame_that_is_not_ipv4_is_none() {
    // ARP and IPv6 are the two most common things on any link, and
    // neither is what this decoder reads.
    for ethertype in [[0x08, 0x06], [0x86, 0xdd], [0x81, 0x00]] {
        let mut f = vec![0u8; 12];
        f.extend_from_slice(&ethertype);
        f.extend_from_slice(&[0u8; 40]);
        assert_eq!(parse_candidate(&f), None, "{ethertype:?} decoded as IPv4");
    }
}

#[test]
fn an_ethertype_of_ipv4_with_no_packet_after_it_is_none() {
    let mut f = vec![0u8; 12];
    f.extend_from_slice(&[0x08, 0x00]);
    // Nothing at all after the ethertype.
    assert_eq!(parse_candidate(&f), None);
}

#[test]
fn an_ip_version_that_is_not_four_is_none() {
    // The version nibble says 6 while the ethertype says IPv4 — a
    // malformed or deliberately confusing frame, and reading it as
    // IPv4 anyway would parse the wrong bytes as an address.
    let mut f = frame(6, [1, 2, 3, 4], 80);
    f[14] = 0x65; // version 6, IHL 5
    assert_eq!(parse_candidate(&f), None);
}

#[test]
fn a_header_length_below_the_minimum_is_none() {
    // IHL is in 32-bit words and an IPv4 header cannot be under 20
    // bytes. A smaller value would make the L4 offset point back into
    // the IP header.
    let mut f = frame(6, [1, 2, 3, 4], 80);
    f[14] = 0x44; // version 4, IHL 4 -> 16 bytes
    assert_eq!(parse_candidate(&f), None);
}

#[test]
fn a_protocol_that_is_not_tcp_or_udp_is_none() {
    // ICMP is the obvious one, and it has no ports to report.
    for proto in [1u8, 2, 47, 132] {
        assert_eq!(
            parse_candidate(&frame(proto, [1, 2, 3, 4], 80)),
            None,
            "protocol {proto} decoded"
        );
    }
}

#[test]
fn a_frame_truncated_inside_the_ip_header_is_none() {
    // Every field from the version byte to the destination address,
    // cut off one byte at a time. Each is a different `?`.
    let full = frame(6, [1, 2, 3, 4], 80);
    for len in 14..34 {
        assert_eq!(
            parse_candidate(&full[..len]),
            None,
            "a frame cut to {len} bytes decoded"
        );
    }
}

#[test]
fn a_frame_with_no_room_for_the_ports_is_none() {
    // The header is complete and the payload is not. This is what a
    // capture with a small snaplen produces, so it is the truncation
    // most likely to actually arrive.
    let full = frame(6, [1, 2, 3, 4], 80);
    for len in 34..full.len() {
        assert_eq!(
            parse_candidate(&full[..len]),
            None,
            "a frame cut to {len} bytes decoded"
        );
    }
}

#[test]
fn a_header_longer_than_the_frame_is_none() {
    // IHL claims options that are not there, so the L4 slice starts
    // past the end.
    let mut f = frame(6, [1, 2, 3, 4], 80);
    f[14] = 0x4f; // IHL 15 -> 60 bytes of header
    assert_eq!(parse_candidate(&f), None);
}

#[test]
fn udp_decodes_as_udp() {
    assert_eq!(
        parse_candidate(&frame(17, [8, 8, 8, 8], 53)).as_deref(),
        Some("8.8.8.8:53 udp")
    );
}

/// A backend that knows an action and not which rule gave it.
struct ActionOnly;

impl CaptureBackend for ActionOnly {
    fn match_action(&self, _candidate: &str) -> Option<Action> {
        Some(Action::Block)
    }
}

#[test]
fn a_backend_that_cannot_name_the_rule_says_so_rather_than_inventing_one() {
    // The trait's default. It exists precisely so a backend built on
    // something other than a rule list — tcpdump, ss — is not forced to
    // make one up, and nothing had ever used it.
    let b = ActionOnly;
    assert_eq!(b.match_action("example.com:443 tcp"), Some(Action::Block));
    assert_eq!(b.match_rule("example.com:443 tcp"), None);
}

#[test]
fn a_capture_is_appended_as_org_and_the_file_keeps_growing() {
    // Appending, not rewriting: a log that replaced its file would lose
    // every earlier capture, and nothing had ever called this twice.
    let d = tempfile::tempdir().expect("tempdir");
    let path = d.path().join("network.org");

    log_capture_to_org(&path, "example.com", "tcp", "2026-08-14 Fri 12:00").expect("first");
    log_capture_to_org(&path, "other.test", "udp", "2026-08-14 Fri 12:01").expect("second");

    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("host=example.com"), "{text}");
    assert!(text.contains("host=other.test"), "{text}");
    assert_eq!(
        text.lines().count(),
        2,
        "the second write replaced the first: {text}"
    );
    assert!(text.starts_with("* <"), "not an org headline: {text}");
}

#[test]
fn logging_to_a_path_that_cannot_be_opened_is_an_error_not_a_panic() {
    let d = tempfile::tempdir().expect("tempdir");
    let nested = d.path().join("no-such-dir").join("network.org");
    assert!(log_capture_to_org(&nested, "h", "tcp", "ts").is_err());
}

#[test]
fn the_mock_backend_names_the_rule_that_decided() {
    // `MockBackend` overrides the default precisely because it *can*
    // say which rule matched, and that override had never been called.
    // "Blocked by what" is the question anyone has in front of a
    // blocked request, and a backend that answers the action but not
    // the rule is the one that leaves them guessing.
    use closure_sniffer::{MockBackend, Rule};

    let rules = vec![
        Rule {
            id: "no-trackers".to_owned(),
            pattern: "*.tracker.test:*".to_owned(),
            action: Action::Block,
        },
        Rule {
            id: "https-is-fine".to_owned(),
            pattern: "*:443 tcp".to_owned(),
            action: Action::Allow,
        },
    ];
    let b = MockBackend::new(rules);

    let hit = b
        .match_rule("ads.tracker.test:80 tcp")
        .expect("a rule matched");
    assert_eq!(hit.id, "no-trackers", "the wrong rule was named");
    assert_eq!(hit.pattern, "*.tracker.test:*");
    assert_eq!(hit.action, Action::Block);

    // The first matching rule wins, and the reported rule must be that
    // same one — an action from rule one and a name from rule two
    // would be worse than no name at all.
    assert_eq!(
        b.match_action("ads.tracker.test:80 tcp"),
        Some(Action::Block)
    );

    assert_eq!(b.match_rule("nothing.matches.test:22 tcp"), None);
}
