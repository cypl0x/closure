//! A blocked host does not get a prompt.
//!
//! The sniffer could say a connection should not have happened, after
//! it had. The one outbound request closure originates by itself is the
//! LLM call, so that is the one it can actually refuse — and refusing
//! it has to happen *before* the request, which is the difference
//! between enforcement and a log.
//!
//! The test asserts on the answer rather than on a socket, because a
//! test that needed the network to prove nothing went over the network
//! would be the wrong shape.

#![cfg(feature = "gpui-test")]
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_shell_gpui::visual_window;

const VAULT: &str = "\
* A note
:PROPERTIES:
:ID: 01LLMBLOCK0000000001
:END:
";

/// A vault whose config points the assistant at a host, and blocks it.
fn config(blocklist: &str) -> String {
    format!(
        "* Settings\n#+BEGIN_SRC closure-config\n\
         llm_provider = openai-compatible\n\
         llm_key_env = CLOSURE_TEST_KEY\n\
         llm_endpoint = http://api.example.invalid/v1\n\
         {blocklist}\n\
         #+END_SRC\n"
    )
}

#[gpui::test]
fn a_blocked_endpoint_is_refused_without_asking(cx: &mut gpui::TestAppContext) {
    let (dir, view, vcx) = visual_window(cx, VAULT);
    std::fs::write(
        dir.path().join(closure_config::CONFIG_FILE),
        config("sniffer_blocklist = api.example.invalid*"),
    )
    .unwrap();
    view.update(vcx, |v, cx| v.ask_llm("hello".to_owned(), cx));
    vcx.run_until_parked();
    let said = view.update(vcx, |v, _cx| v.chat_last());
    assert!(
        said.contains("blocked"),
        "a blocked host was not refused: {said:?}"
    );
    assert!(
        said.contains("api.example.invalid"),
        "the refusal does not say which host: {said:?}"
    );
}

#[gpui::test]
fn a_host_nobody_blocked_is_not_refused(cx: &mut gpui::TestAppContext) {
    // The control. If this said "blocked" too, the feature would be
    // "closure refuses to use a model" wearing a blocklist's name.
    let (dir, view, vcx) = visual_window(cx, VAULT);
    std::fs::write(
        dir.path().join(closure_config::CONFIG_FILE),
        config("sniffer_blocklist = somewhere.else.invalid*"),
    )
    .unwrap();
    view.update(vcx, |v, cx| v.ask_llm("hello".to_owned(), cx));
    vcx.run_until_parked();
    let said = view.update(vcx, |v, _cx| v.chat_last());
    assert!(
        !said.contains("blocked"),
        "refused a host nobody blocked: {said:?}"
    );
}
