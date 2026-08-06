//! "Consume MCP Servers (?)" — where the servers are named.
//!
//! Same shape as `bind <chord> = <command>`, and for the same reason:
//! the name is part of the key because the value is a command line,
//! and a value holding both would need an escape rule for the
//! separator. A command line has commas in it.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

fn parse(src: &str) -> closure_config::Config {
    closure_config::Config::from_org_source(src).expect("config")
}

#[test]
fn a_server_is_a_name_and_a_command() {
    let cfg =
        parse("#+begin_src closure-config\nmcp files = mcp-fs --root /tmp, --ro\n#+end_src\n");
    assert_eq!(
        cfg.mcp_servers,
        vec![("files".to_owned(), "mcp-fs --root /tmp, --ro".to_owned())],
        "the comma in the command line was treated as a separator"
    );
}

#[test]
fn several_of_them_keep_their_order() {
    let cfg =
        parse("#+begin_src closure-config\nmcp files = mcp-fs\nmcp issues = mcp-gh\n#+end_src\n");
    assert_eq!(cfg.mcp_servers.len(), 2);
    assert_eq!(cfg.mcp_servers[1].0, "issues");
}

#[test]
fn a_server_with_no_name_says_so() {
    let err = closure_config::Config::from_org_source(
        "#+begin_src closure-config\nmcp = mcp-fs\n#+end_src\n",
    )
    .expect_err("an unnamed server cannot be called by name");
    assert!(format!("{err}").contains("mcp"), "{err}");
}

#[test]
fn no_line_means_no_servers() {
    assert!(
        parse("#+begin_src closure-config\ntheme = doom\n#+end_src\n")
            .mcp_servers
            .is_empty()
    );
}
