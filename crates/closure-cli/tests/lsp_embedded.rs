//! "org-edit-special on src blocks and then fiddle with the source
//! code" — the other half: a language server for what is *inside* the
//! block.
//!
//! The block is handed to whatever server the config names for its
//! language, as a document of its own, and the answer is shifted back
//! into the org file's coordinates before the editor sees it.
//!
//! The server spawned here is `closure lsp` on a nested `#+BEGIN_SRC
//! org` block, which is a real case and the only language server
//! guaranteed to exist on the machine running these tests. Everything
//! it exercises — the process, the Content-Length framing, the
//! handshake, the shift home — is what rust-analyzer would go through.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_lsp::{Embedded, SrcBlock};

/// An org note whose src block is itself org, with a headline in it.
const ORG: &str = "\
* Outer note
:PROPERTIES:
:ID: 01EMBEDDED0000000000001
:END:
Prose.

#+BEGIN_SRC org
* Inner headline
:PROPERTIES:
:ID: 01EMBEDDED0000000000002
:END:
#+END_SRC
";

fn vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.org"), ORG).unwrap();
    dir
}

#[test]
fn the_block_is_asked_of_the_server_for_its_language() {
    let dir = vault();
    let command = format!(
        "{} lsp {}",
        env!("CARGO_BIN_EXE_closure"),
        dir.path().display()
    );
    let block = SrcBlock::at(ORG, 7).expect("the org block");
    assert_eq!(block.language, "org");

    let mut server = Embedded::start("org", &command).expect("start");
    let symbols = server
        .document_symbols(&block)
        .expect("the embedded server answered");
    assert!(
        symbols.contains("Inner headline"),
        "the block's own contents were not what the server saw: {symbols}"
    );
    assert!(
        !symbols.contains("Outer note"),
        "the server was shown the whole file instead of the block: {symbols}"
    );
}

#[test]
fn what_comes_back_is_in_the_org_files_coordinates() {
    let dir = vault();
    let command = format!(
        "{} lsp {}",
        env!("CARGO_BIN_EXE_closure"),
        dir.path().display()
    );
    let block = SrcBlock::at(ORG, 7).expect("the org block");
    let mut server = Embedded::start("org", &command).expect("start");
    let symbols = server.document_symbols(&block).expect("answered");
    // `* Inner headline` is line 0 of the block and line 7 of the file.
    assert!(
        symbols.contains(r#""line":7"#),
        "the answer is still in the block's own line numbers: {symbols}"
    );
    assert!(
        !symbols.contains(r#""line":0"#),
        "a line was left unshifted: {symbols}"
    );
}

#[test]
fn a_server_that_will_not_start_is_reported() {
    assert!(
        Embedded::start("rust", "definitely-not-a-language-server").is_err(),
        "a missing language server looked like a working one"
    );
}

#[test]
fn a_hover_inside_a_block_is_answered_by_the_blocks_own_server() {
    // The wiring, end to end: config.org names a server for `org`, the
    // file has an org block in it, and a hover on line 8 — a headline
    // inside that block — comes back from the embedded server rather
    // than from closure's own org knowledge of the outer file.
    let inner = vault();
    let dir = vault();
    std::fs::write(
        dir.path().join("config.org"),
        format!(
            "#+begin_src closure-config\nlsp org = {} lsp {}\n#+end_src\n",
            env!("CARGO_BIN_EXE_closure"),
            inner.path().display()
        ),
    )
    .unwrap();

    let mut servers = closure_lsp::Embeddings::from_config(
        &closure_config::Config::from_path(&dir.path().join("config.org"))
            .expect("config")
            .lsp_servers,
    );
    let block = SrcBlock::at(ORG, 7).expect("the org block");
    let answered = servers
        .ask("textDocument/documentSymbol", &block, 7, 0)
        .expect("a server is configured for org")
        .expect("it answered");
    assert!(answered.contains("Inner headline"), "{answered}");
}

#[test]
fn a_language_with_no_server_configured_is_left_to_closure() {
    let mut servers = closure_lsp::Embeddings::from_config(&[]);
    let block = SrcBlock::at(ORG, 7).expect("the org block");
    assert!(
        servers.ask("textDocument/hover", &block, 7, 0).is_none(),
        "a block with no configured server was still routed somewhere"
    );
}

/// Speak LSP to a child process: send each message as a frame, return
/// the bodies of the replies that carry an id.
fn talk(command: &[&str], messages: &[String]) -> Vec<String> {
    use std::io::{BufRead as _, Read as _, Write as _};
    let mut child = std::process::Command::new(command[0])
        .args(&command[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut replies = Vec::new();
    let expected = messages.iter().filter(|m| m.contains("\"id\"")).count();
    for m in messages {
        write!(stdin, "Content-Length: {}\r\n\r\n{m}", m.len()).unwrap();
        stdin.flush().unwrap();
    }
    while replies.len() < expected {
        let mut len = 0usize;
        loop {
            let mut header = String::new();
            if stdout.read_line(&mut header).unwrap() == 0 {
                let _ = child.kill();
                let _ = child.wait();
                return replies;
            }
            let trimmed = header.trim();
            if trimmed.is_empty() {
                break;
            }
            if let Some(v) = trimmed.strip_prefix("Content-Length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut buf = vec![0u8; len];
        stdout.read_exact(&mut buf).unwrap();
        replies.push(String::from_utf8(buf).unwrap());
    }
    let _ = child.kill();
    let _ = child.wait();
    replies
}

#[test]
fn a_real_server_forwards_a_position_inside_a_block() {
    // The whole path through one running `closure lsp`: it reads its
    // own config.org, spawns the server named there for `org`, hands
    // the block over, and shifts the answer home.
    let inner = vault();
    let dir = vault();
    std::fs::write(
        dir.path().join("config.org"),
        format!(
            "#+begin_src closure-config\nlsp org = {} lsp {}\n#+end_src\n",
            env!("CARGO_BIN_EXE_closure"),
            inner.path().display()
        ),
    )
    .unwrap();

    let text = ORG.replace('\n', "\\n").replace('"', "\\\"");
    let messages = vec![
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#.to_owned(),
        format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file://notes.org","languageId":"org","version":1,"text":"{text}"}}}}}}"#
        ),
        // Line 7 is `* Inner headline`, inside the org block.
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file://notes.org"},"position":{"line":7,"character":3}}}"#.to_owned(),
    ];
    let replies = talk(
        &[
            env!("CARGO_BIN_EXE_closure"),
            "lsp",
            dir.path().to_str().unwrap(),
        ],
        &messages,
    );
    assert_eq!(replies.len(), 2, "{replies:?}");
    let hover = &replies[1];
    // closure's hover says `level N · id:…` rather than the title, so
    // *which* headline answered is the id: 0002 is the one inside the
    // block, 0001 is the outer note the file is about.
    assert!(
        hover.contains("01EMBEDDED0000000000002"),
        "the hover was answered by the outer file, not the block: {hover}"
    );
    assert!(
        !hover.contains("01EMBEDDED0000000000001"),
        "the outer note answered a position inside the block: {hover}"
    );
}
