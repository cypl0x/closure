//! "Consume MCP Servers (?)" — end to end, over a real process.
//!
//! The other tests script the wire. This one spawns an actual MCP
//! server, shakes hands with it, lists its tools and calls one. The
//! server it spawns is closure's own `closure mcp`, which is the only
//! MCP server guaranteed to exist on the machine running the tests —
//! and consuming what you serve is the honest end of the loop.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

fn vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.org"),
        "* Hello from the other vault\n:PROPERTIES:\n:ID: 01MCPCLIENT00000000000001\n:END:\n",
    )
    .unwrap();
    dir
}

#[test]
fn closure_can_be_a_client_of_closure() {
    let dir = vault();
    let command = format!(
        "{} mcp {}",
        env!("CARGO_BIN_EXE_closure"),
        dir.path().display()
    );
    let mut server = closure_mcp::Server::spawn("notes", &command).expect("spawn");

    let tools = server.tools().expect("tools/list");
    assert!(
        tools.iter().any(|t| t.name == "notes/search"),
        "the server's tools are not qualified by its name: {tools:?}"
    );
    assert!(
        tools.iter().any(|t| t.schema.contains("args")),
        "the schema the caller has to fill in did not come back: {tools:?}"
    );

    let out = server
        .call("notes/search", r#"{"args":"Hello"}"#)
        .expect("tools/call");
    assert!(
        out.contains("Hello from the other vault"),
        "the call returned {out:?}"
    );
}

#[test]
fn a_command_that_is_not_there_is_reported_not_panicked() {
    let got = closure_mcp::Server::spawn("nope", "closure-no-such-binary --serve");
    assert!(got.is_err(), "a missing server looked like a working one");
}

#[test]
fn a_command_that_is_not_a_server_is_reported_too() {
    // `true` exits immediately: a process that starts and says nothing
    // is the failure mode of pointing this at the wrong binary.
    let got = closure_mcp::Server::spawn("silent", "true");
    assert!(got.is_err(), "a silent process looked like a working one");
}

#[test]
fn a_server_that_will_not_start_does_not_take_the_others_with_it() {
    let dir = vault();
    let ok = format!(
        "{} mcp {}",
        env!("CARGO_BIN_EXE_closure"),
        dir.path().display()
    );
    let mut servers = closure_mcp::Servers::start(&[
        ("broken".to_owned(), "closure-no-such-binary".to_owned()),
        ("notes".to_owned(), ok),
    ]);
    assert_eq!(servers.failures().len(), 1, "{:?}", servers.failures());
    assert!(
        servers.failures()[0].1.contains("broken"),
        "the failure does not say which server: {:?}",
        servers.failures()
    );
    assert!(
        servers.menu().contains("notes/search"),
        "the working server was lost with the broken one: {}",
        servers.menu()
    );
}

#[test]
fn a_local_tool_line_is_left_alone() {
    let mut servers = closure_mcp::Servers::start(&[]);
    assert!(
        servers.call_line("search Hello").is_none(),
        "a vault tool was routed to a server that has never heard of it"
    );
}

#[test]
fn a_remote_call_needs_the_arguments_its_schema_asks_for() {
    let dir = vault();
    let command = format!(
        "{} mcp {}",
        env!("CARGO_BIN_EXE_closure"),
        dir.path().display()
    );
    let mut servers = closure_mcp::Servers::start(&[("notes".to_owned(), command)]);
    let out = servers
        .call_line(r#"notes/search {"args":"Hello"}"#)
        .expect("routed to the server");
    assert!(out.contains("Hello from the other vault"), "{out}");

    let out = servers
        .call_line("notes/search Hello")
        .expect("routed to the server");
    assert!(
        out.starts_with("ERROR") && out.contains("notes/search") && out.contains("JSON object"),
        "a bare word was passed off as an arguments object: {out}"
    );
}

#[test]
fn the_assistant_is_told_which_remote_tools_it_has() {
    // End of the wire: `closure ask` against a vault whose config.org
    // names an MCP server has to hand the model that server's tools.
    // The echo provider returns the prompt unchanged, so what comes
    // back on stdout *is* what the model would have been given.
    let other = vault();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.org"), "* Local\n").unwrap();
    std::fs::write(
        dir.path().join("config.org"),
        format!(
            "#+begin_src closure-config\nllm_provider = echo\nmcp notes = {} mcp {}\n#+end_src\n",
            env!("CARGO_BIN_EXE_closure"),
            other.path().display()
        ),
    )
    .unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_closure"))
        .args(["ask", "--vault"])
        .arg(dir.path())
        .arg("what can you do")
        .output()
        .expect("run");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("notes/search"),
        "the model was never told the server exists:\n{text}"
    );
    assert!(
        text.contains("notes/capture"),
        "only some of its tools came through:\n{text}"
    );
}
