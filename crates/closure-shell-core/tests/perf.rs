//! Q12: performance is a number, not a claim. A seeded vault
//! generator (B1) + the shared-input-seam latency harness (B3): p99
//! over `ModalApp::on_key` on a big vault must stay under a budget
//! generous enough for debug builds — `just bench` prints the real
//! release numbers.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_core::{ModalApp, Shell, gen_vault};
use closure_store::Vault;

#[test]
fn gen_vault_is_deterministic_and_sized() {
    let a = gen_vault(5, 20, 42);
    let b = gen_vault(5, 20, 42);
    assert_eq!(a, b, "same seed, same vault (I6)");
    assert_eq!(a.len(), 5, "file count");
    let heads = a[0].1.matches("\n* ").count() + usize::from(a[0].1.starts_with("* "));
    assert_eq!(
        heads,
        20,
        "headlines per file: {}",
        &a[0].1[..120.min(a[0].1.len())]
    );
    let c = gen_vault(5, 20, 43);
    assert_ne!(a, c, "different seed, different content");
}

#[test]
fn generated_vault_parses_and_roundtrips() {
    for (name, content) in gen_vault(3, 10, 7) {
        let doc = closure_core::Document::load_str(&content)
            .unwrap_or_else(|e| panic!("{name} parses: {e}"));
        assert_eq!(doc.source(), content, "byte-exact (I1) for {name}");
    }
}

#[test]
fn dispatch_p99_stays_under_budget_on_a_big_vault() {
    let dir = tempfile::tempdir().expect("tmp");
    for (name, content) in gen_vault(50, 40, 1) {
        std::fs::write(dir.path().join(name), content).expect("write");
    }
    let vault = Vault::open(dir.path()).expect("open");
    let mut sh = Shell::new(vault);
    let mut app = ModalApp::new(closure_config::InputMode::Vim);
    // 1000 navigation keystrokes over 2000 headlines through the one
    // shared input seam.
    let mut samples: Vec<u128> = Vec::with_capacity(1000);
    for i in 0..1000u32 {
        let key = if i % 3 == 0 {
            "j"
        } else if i % 3 == 1 {
            "k"
        } else {
            "j"
        };
        let t = std::time::Instant::now();
        app.on_key(&mut sh, key, false, false, key.chars().next());
        samples.push(t.elapsed().as_micros());
    }
    samples.sort_unstable();
    let p99 = samples[989];
    eprintln!("dispatch p99 = {p99} µs over 2000 headlines (debug build)");
    // Debug-safe budget; `just bench` prints release numbers. The
    // vision's "almost no input lag" starts being enforced here.
    assert!(p99 < 50_000, "p99 {p99} µs blew the 50 ms debug budget");
}
