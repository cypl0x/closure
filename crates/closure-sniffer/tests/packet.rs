//! X3a hermetic core: the packet decoder turns a raw Ethernet frame into
//! a sniffer candidate. The live pcap capture is privileged/runtime; this
//! tests the parsing with synthetic frames, no network.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_sniffer::parse_candidate;

/// Ethernet(14) + IPv4(20) + L4 ports — a minimal frame to `dst:port proto`.
fn frame(proto: u8, dst: [u8; 4], dst_port: u16) -> Vec<u8> {
    let mut f = vec![0u8; 12]; // dst + src MAC
    f.extend_from_slice(&[0x08, 0x00]); // ethertype IPv4
    f.push(0x45); // version 4, IHL 5 (20 bytes)
    f.extend_from_slice(&[0x00, 0, 0, 0, 0, 0, 0, 0x40]); // tos..ttl
    f.push(proto);
    f.extend_from_slice(&[0, 0]); // checksum
    f.extend_from_slice(&[10, 0, 0, 1]); // src ip
    f.extend_from_slice(&dst); // dst ip
    f.extend_from_slice(&[0, 0]); // l4 src port
    f.extend_from_slice(&dst_port.to_be_bytes()); // l4 dst port
    f
}

#[test]
fn decodes_tcp_to_dst_host_port_proto() {
    let f = frame(6, [93, 184, 216, 34], 443);
    assert_eq!(parse_candidate(&f).as_deref(), Some("93.184.216.34:443 tcp"));
}

#[test]
fn decodes_udp() {
    let f = frame(17, [1, 1, 1, 1], 53);
    assert_eq!(parse_candidate(&f).as_deref(), Some("1.1.1.1:53 udp"));
}

#[test]
fn ignores_non_ipv4_and_other_protocols() {
    // ARP (ethertype 0x0806) — not IPv4.
    let mut arp = vec![0u8; 12];
    arp.extend_from_slice(&[0x08, 0x06]);
    arp.extend_from_slice(&[0u8; 30]);
    assert!(parse_candidate(&arp).is_none());
    // ICMP (proto 1) — not TCP/UDP.
    assert!(parse_candidate(&frame(1, [8, 8, 8, 8], 0)).is_none());
    // Truncated.
    assert!(parse_candidate(&[0u8; 10]).is_none());
}
