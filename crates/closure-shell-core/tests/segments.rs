//! P3: split a headline body into prose vs. `#+BEGIN_SRC` code segments
//! (carrying the fence language) so the GUI styles each. Pure + hermetic.

#![allow(clippy::unwrap_used)]

use closure_shell_core::{BodySegment, segment_body};

#[test]
fn plain_body_is_one_prose_segment() {
    let segs = segment_body("just some\nprose lines\n");
    assert_eq!(segs.len(), 1);
    match &segs[0] {
        BodySegment::Prose(t) => assert_eq!(t.trim_end(), "just some\nprose lines"),
        BodySegment::Code { .. } => panic!("expected prose, got code"),
    }
}

#[test]
fn empty_body_has_no_segments() {
    assert!(segment_body("").is_empty());
}

#[test]
fn src_block_becomes_code_segment_with_language() {
    let body = "#+BEGIN_SRC rust\nfn main() {}\n#+END_SRC\n";
    let segs = segment_body(body);
    assert_eq!(segs.len(), 1);
    match &segs[0] {
        BodySegment::Code { lang, text } => {
            assert_eq!(lang, "rust");
            assert_eq!(text.trim_end(), "fn main() {}");
        }
        BodySegment::Prose(_) => panic!("expected code, got prose"),
    }
}

#[test]
fn prose_around_code_yields_three_segments() {
    let body = "intro line\n#+BEGIN_SRC python\nprint(1)\n#+END_SRC\noutro line\n";
    let segs = segment_body(body);
    assert_eq!(segs.len(), 3, "{segs:?}");
    assert!(matches!(&segs[0], BodySegment::Prose(t) if t.contains("intro")));
    assert!(matches!(&segs[1], BodySegment::Code { lang, .. } if lang == "python"));
    assert!(matches!(&segs[2], BodySegment::Prose(t) if t.contains("outro")));
}

#[test]
fn fence_is_case_insensitive_and_langless_defaults_plain() {
    let body = "#+begin_src\nraw\n#+end_src\n";
    let segs = segment_body(body);
    assert_eq!(segs.len(), 1);
    assert!(matches!(&segs[0], BodySegment::Code { lang, .. } if lang == "plain"));
}

#[test]
fn unterminated_block_captures_to_end_as_code() {
    let body = "#+BEGIN_SRC sh\necho hi\nno end fence";
    let segs = segment_body(body);
    assert_eq!(segs.len(), 1);
    match &segs[0] {
        BodySegment::Code { lang, text } => {
            assert_eq!(lang, "shell");
            assert!(text.contains("echo hi") && text.contains("no end fence"));
        }
        BodySegment::Prose(_) => panic!("expected code, got prose"),
    }
}
