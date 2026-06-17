//! A *real* parser safety gate that runs on the pinned stable
//! toolchain under `cargo test` (no nightly / cargo-fuzz needed).
//!
//! Three layers, each a hard gate that fails CI on regression:
//!   1. every committed fixture round-trips byte-exact (I1 on the
//!      real corpus, not just random input);
//!   2. a deterministic full-byte-range fuzzer (seeded, reproducible)
//!      drives `parse` over tens of thousands of arbitrary inputs —
//!      no panic (I5), determinism (I6), and byte-exact roundtrip on
//!      valid UTF-8 (I1);
//!   3. hand-picked adversarial inputs that have historically broken
//!      hand-written org parsers.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};

fn workspace_fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn org_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            org_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "org") {
            out.push(p);
        }
    }
}

#[test]
fn every_committed_org_fixture_roundtrips_byte_exact() {
    let mut files = Vec::new();
    org_files(&workspace_fixtures(), &mut files);
    assert!(
        files.len() >= 20,
        "corpus shrank unexpectedly: {}",
        files.len()
    );
    for f in files {
        let src = std::fs::read_to_string(&f).expect("read fixture");
        let doc = closure_org::parse(&src).expect("fixture parses");
        assert_eq!(
            closure_org::print(&doc),
            src,
            "I1 byte-exact roundtrip failed for {}",
            f.display()
        );
    }
}

/// Tiny dependency-free xorshift64* PRNG — deterministic so a failure
/// reproduces from the printed seed.
struct Rng(u64);
impl Rng {
    const fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    const fn byte(&mut self) -> u8 {
        (self.next_u64() & 0xff) as u8
    }
}

#[test]
fn deterministic_byte_fuzz_never_panics_and_roundtrips() {
    const ITERATIONS: usize = 40_000;
    // Mix of structural org bytes and the full 0..=255 range, so we hit
    // both classification edge cases and invalid UTF-8.
    const STRUCTURAL: &[u8] = b"\n \t#+:*-_[]<>|aA1.";
    let mut rng = Rng(0x0BAD_C0DE_D15E_A5E5);
    for iter in 0..ITERATIONS {
        let len = usize::try_from(rng.next_u64() % 512).unwrap_or(0);
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..len {
            // 70% structural, 30% arbitrary byte.
            if rng.next_u64() % 10 < 7 {
                let idx = usize::try_from(rng.next_u64() % STRUCTURAL.len() as u64).unwrap_or(0);
                bytes.push(STRUCTURAL[idx]);
            } else {
                bytes.push(rng.byte());
            }
        }
        // parse takes &str; only valid UTF-8 is parseable. Both arms
        // must not panic.
        if let Ok(s) = std::str::from_utf8(&bytes) {
            let doc = closure_org::parse(s).expect("parse is infallible on valid utf8");
            assert_eq!(
                closure_org::print(&doc),
                s,
                "I1 roundtrip failed at iteration {iter} (seed 0x0BAD_C0DE_D15E_A5E5)"
            );
            // I6 determinism.
            let doc2 = closure_org::parse(s).expect("parse is infallible");
            assert_eq!(doc, doc2, "I6 determinism failed at iteration {iter}");
        }
    }
}

#[test]
fn adversarial_inputs_roundtrip() {
    let nasty: &[&str] = &[
        "",
        "*",
        "* ",
        "*\t",
        "\u{feff}* BOM heading\n",
        "* a\r\n** b\r\n",
        "* h\n:PROPERTIES:\n:ID: x\n",      // unterminated drawer
        "#+BEGIN_SRC rust\nfn main() {}\n", // unterminated block
        "#+begin_src\n#+end_src\n",
        ":PROPERTIES:\n:END:\n",
        "* \n\n\n* \n",
        "- a\n  - b\n    - c\n",
        "[[id:abc][label]] [[broken",
        "* TODO [#A] title :a:b:c:\n",
        "\n\n\n\n",
        "   \t  \t\n",
        "\r",                                                  // lone CR
        "* a\r\rb\r",                                          // bare CRs, no LF
        "* h\n:PROPERTIES:\n:A: 1\n:END:\n:LOGBOOK:\n:END:\n", // two drawers
        "#+BEGIN_SRC\n#+BEGIN_SRC\nnested?\n#+END_SRC\n",      // nested fences
        "| a | b |\n|---+---|\n| 1 | 2 |\n",                   // table
        "* h\nText with [fn:1] footnote.\n[fn:1] def\n",
        "[[https://x][a]] [[file:./y.org]] [[*heading]]", // link variety
        "* café ☕ naïve \u{0301}combining\n",            // unicode + combining
        "*** \n** \n* \n",                                // descending levels
        "#+TITLE: x\n#+AUTHOR: y\n#+OPTIONS: toc:nil\n",  // keywords
        "- [ ] todo\n- [X] done\n- [-] partial\n",        // checkbox list
        "* h\nSCHEDULED: <2026-06-15 Mon> DEADLINE: <2026-06-16>\n",
        "\u{feff}\u{feff}* double BOM\n",
        ": literal block line\n:another\n", // colon-prefixed lines
    ];
    let deep_stars = "*".repeat(1000);
    let owned: Vec<String> = vec![deep_stars, format!("{}\n", "*".repeat(500))];
    for input in nasty
        .iter()
        .copied()
        .chain(owned.iter().map(String::as_str))
    {
        let doc = closure_org::parse(input).expect("parse is infallible");
        assert_eq!(
            closure_org::print(&doc),
            input,
            "adversarial roundtrip failed for {input:?}"
        );
    }
}
