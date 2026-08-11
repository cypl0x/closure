//! Tangling resolves noweb references.
//!
//! `<<name>>` is what lets a literate document explain a program in
//! the order a reader needs and assemble it in the order the compiler
//! needs — which is the entire argument for writing one. Tangling
//! wrote the reference out verbatim, so the assembled file had
//! `<<setup>>` in it where the setup should have been, and the
//! literate half of literate programming did not work.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_store::Vault;

const DOC: &str = "\
* Program
:PROPERTIES:
:ID: 01TANGLE00000000000001A
:END:
#+NAME: setup
#+BEGIN_SRC sh
echo setting up
#+END_SRC

#+BEGIN_SRC sh :tangle out.sh
<<setup>>
echo main
#+END_SRC
";

#[test]
fn a_noweb_reference_is_resolved_on_the_way_out() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lit.org"), DOC).unwrap();
    let v = Vault::open(dir.path()).unwrap();
    let written = v.tangle(&dir.path().join("lit.org")).unwrap();
    assert_eq!(written.len(), 1, "{written:?}");
    let out = std::fs::read_to_string(&written[0]).unwrap();
    assert!(out.contains("echo setting up"), "{out}");
    assert!(out.contains("echo main"), "{out}");
    assert!(!out.contains("<<setup>>"), "the reference survived: {out}");
}

#[test]
fn the_org_file_is_not_rewritten() {
    // I12 again: tangling produces a file, it does not edit the one it
    // was written from.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lit.org"), DOC).unwrap();
    let v = Vault::open(dir.path()).unwrap();
    let _ = v.tangle(&dir.path().join("lit.org")).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("lit.org")).unwrap(),
        DOC
    );
}

#[test]
fn a_reference_to_nothing_fails_the_tangle_rather_than_writing_it() {
    // Writing `<<nosuch>>` into a shell script produces a file that
    // fails later, somewhere else, with a worse message.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("lit.org"),
        "* P\n:PROPERTIES:\n:ID: 01TANGLE00000000000002A\n:END:\n\
         #+BEGIN_SRC sh :tangle out.sh\n<<nosuch>>\n#+END_SRC\n",
    )
    .unwrap();
    let v = Vault::open(dir.path()).unwrap();
    assert!(v.tangle(&dir.path().join("lit.org")).is_err());
    assert!(
        !dir.path().join("out.sh").exists(),
        "a broken file was written"
    );
}
