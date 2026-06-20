//! V5b: pluggable content-address providers + sync. A `BlockProvider`
//! (get/has/put/cids by CID) has an in-memory and a filesystem impl;
//! `sync_providers` exchanges missing blobs so two stores converge to the
//! union — the content-address layer beneath IPFS/iroh.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use closure_sync::{BlockProvider, BlockStore, Cid, FsBlockStore, sync_providers};

fn cids<P: BlockProvider>(p: &P) -> BTreeSet<Cid> {
    p.cids().into_iter().collect()
}

#[test]
fn in_memory_providers_converge_to_the_union() {
    let mut store_a = BlockStore::new();
    let cx = store_a.put(b"x");
    let cy = store_a.put(b"y");
    let mut store_b = BlockStore::new();
    store_b.put(b"y");
    let cz = store_b.put(b"z");

    let moved = sync_providers(&mut store_a, &mut store_b);
    assert!(moved >= 2, "x→b and z→a transferred: {moved}");
    let union: BTreeSet<Cid> = [cx, cy, cz].into_iter().collect();
    assert_eq!(cids(&store_a), union, "a has the union");
    assert_eq!(cids(&store_b), union, "b has the union");
}

#[test]
fn filesystem_provider_round_trips() {
    let dir = tempfile::tempdir().expect("tmp");
    let mut fs = FsBlockStore::new(dir.path());
    let cid = fs.put(b"persisted");
    assert!(fs.has(&cid));
    assert_eq!(fs.get(&cid).unwrap(), b"persisted");
    assert_eq!(fs.cids(), vec![cid]);
}

#[test]
fn sync_between_memory_and_filesystem_converges() {
    let dir = tempfile::tempdir().expect("tmp");
    let mut mem = BlockStore::new();
    mem.put(b"from-mem");
    let mut fs = FsBlockStore::new(dir.path());
    fs.put(b"from-fs");

    sync_providers(&mut mem, &mut fs);
    assert_eq!(cids(&mem), cids(&fs), "heterogeneous providers converge");
    // Transferred blobs are content-addressed → they verify.
    let from_mem = Cid::of(b"from-mem");
    assert_eq!(fs.get(&from_mem).unwrap(), b"from-mem");
}

#[test]
fn nothing_to_do_when_already_in_sync() {
    let mut a = BlockStore::new();
    a.put(b"same");
    let mut b = BlockStore::new();
    b.put(b"same");
    assert_eq!(sync_providers(&mut a, &mut b), 0, "no transfers");
}
