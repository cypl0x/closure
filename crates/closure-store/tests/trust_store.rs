//! "The eval policy lives inside the data it protects. … Since
//! `closure sync` is `git pull --rebase` and the CRDT layer merges
//! vault content, cloning or syncing an untrusted vault is arbitrary
//! code execution. The attacker ships the policy file that authorizes
//! their own code block. … The trust store needs to be in
//! `$XDG_CONFIG_HOME`, keyed by vault path or by a hash, and set by
//! the user — never read from the vault root."
//!
//! Reported 2026-08-04 in the Opus 5 review, with the exploit path run
//! end to end: `eval_trust` empty → block refused; edit `config.org`
//! to `eval_trust = shell` → `echo hi` executes. Every byte of that
//! decision arrived over the same channel as the code it permits.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_store::{Vault, grant_eval_trust_at, trusted_languages_at};

/// A vault whose own `config.org` says every language is trusted, and
/// a user config directory that says nothing.
fn fixture() -> (tempfile::TempDir, tempfile::TempDir, Vault) {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        "#+TITLE: config\n\n#+BEGIN_SRC closure-config\neval_trust = shell, python\n#+END_SRC\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* One\n:PROPERTIES:\n:ID: 01TRUSTAAAAAAAAAAAAAAAAAA\n:END:\n",
    )
    .unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    (home, dir, vault)
}

#[test]
fn a_vault_cannot_grant_itself_permission_to_run_code() {
    // The whole finding, as one assertion: a store that has never
    // heard of this vault trusts nothing in it, whatever the vault
    // says about itself.
    let (home, dir, _vault) = fixture();
    let store = home.path().join("trust.org");
    assert!(
        trusted_languages_at(&store, dir.path()).is_empty(),
        "the vault's own config.org still authorises execution"
    );
}

#[test]
fn the_users_own_config_is_what_grants_it() {
    let (home, dir, _vault) = fixture();
    let store = home.path().join("trust.org");
    std::fs::write(&store, format!("{} = shell\n", dir.path().display())).unwrap();
    assert_eq!(
        trusted_languages_at(&store, dir.path()),
        vec!["shell".to_owned()]
    );
}

#[test]
fn a_grant_is_for_one_vault_only() {
    // Keyed by path: trusting the vault you wrote must not trust the
    // one you cloned this morning.
    let (home, dir, _vault) = fixture();
    let other = tempfile::tempdir().unwrap();
    let store = home.path().join("trust.org");
    std::fs::write(&store, format!("{} = shell\n", other.path().display())).unwrap();
    assert!(
        trusted_languages_at(&store, dir.path()).is_empty(),
        "a grant for one vault leaked to another"
    );
    assert_eq!(
        trusted_languages_at(&store, other.path()),
        vec!["shell".to_owned()]
    );
}

#[test]
fn no_store_at_all_means_no_execution() {
    let (home, dir, _vault) = fixture();
    let store = home.path().join("nothing-here.org");
    assert!(trusted_languages_at(&store, dir.path()).is_empty());
}

#[test]
fn granting_is_something_the_code_can_do_for_the_user() {
    // A refusal that tells you to hand-write a file in a directory you
    // have to guess is a refusal people work around by turning the
    // check off. So there is one call that writes the grant.
    let (home, dir, _vault) = fixture();
    let store = home.path().join("trust.org");
    grant_eval_trust_at(&store, dir.path(), "shell").unwrap();
    assert_eq!(
        trusted_languages_at(&store, dir.path()),
        vec!["shell".to_owned()]
    );

    // And it accumulates rather than replacing.
    grant_eval_trust_at(&store, dir.path(), "python").unwrap();
    let trust = trusted_languages_at(&store, dir.path());
    assert!(trust.contains(&"shell".to_owned()), "{trust:?}");
    assert!(trust.contains(&"python".to_owned()), "{trust:?}");

    // Twice is once: a grant is a set, not a log.
    grant_eval_trust_at(&store, dir.path(), "shell").unwrap();
    assert_eq!(trusted_languages_at(&store, dir.path()).len(), 2);
}

#[test]
fn a_grant_for_another_vault_survives_writing_one_for_this_one() {
    let (home, dir, _vault) = fixture();
    let other = tempfile::tempdir().unwrap();
    let store = home.path().join("trust.org");
    grant_eval_trust_at(&store, other.path(), "python").unwrap();
    grant_eval_trust_at(&store, dir.path(), "shell").unwrap();
    assert_eq!(
        trusted_languages_at(&store, other.path()),
        vec!["python".to_owned()],
        "the other vault's grant was clobbered"
    );
}

#[test]
fn a_junk_store_refuses_rather_than_crashing() {
    // The one file that decides whether code runs must fail closed on
    // anything it cannot read.
    let (home, dir, _vault) = fixture();
    let store = home.path().join("trust.org");
    std::fs::write(&store, "this is not\nа = = = grant\n\0\n").unwrap();
    assert!(trusted_languages_at(&store, dir.path()).is_empty());
}

#[test]
fn the_vault_reads_the_store_and_not_its_own_config() {
    // Through `Vault::eval_trust`, which is what the shells call.
    let (_home, _dir, vault) = fixture();
    assert!(
        vault.eval_trust().is_empty(),
        "Vault::eval_trust is still reading the vault: {:?}",
        vault.eval_trust()
    );
}

#[test]
fn a_vault_that_still_has_the_old_key_is_worth_saying_so() {
    // The upgrade path. Someone with `eval_trust = shell` in their
    // vault's config.org, which used to work, now gets a refusal — and
    // the honest thing is to say that the line they are looking at is
    // the line that stopped counting, rather than let them edit it
    // again and wonder.
    let (_home, dir, _vault) = fixture();
    assert!(
        closure_store::vault_claims_trust(dir.path()),
        "the leftover key was not noticed"
    );
}

#[test]
fn a_vault_with_no_such_line_says_nothing() {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let _ = home;
    std::fs::write(dir.path().join("config.org"), "#+TITLE: config\n").unwrap();
    assert!(!closure_store::vault_claims_trust(dir.path()));
}

#[test]
fn an_empty_eval_trust_is_not_a_claim() {
    // The generated default config carries `eval_trust = ` with
    // nothing after it. That is not somebody's setting going quiet; it
    // is the file saying the same thing the new store says.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        "#+BEGIN_SRC closure-config\neval_trust = \n#+END_SRC\n",
    )
    .unwrap();
    assert!(!closure_store::vault_claims_trust(dir.path()));
}
