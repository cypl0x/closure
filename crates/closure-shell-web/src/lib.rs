//! Web shell.
//!
//! M8 skeleton: a single-threaded `std::net` HTTP/1.1 server that
//! exposes a vault as a read-only HTML page. Two delivery modes are
//! planned:
//! * embedded wasm kernel served as a single-file HTML bundle
//! * localhost server (this crate)
//!
//! Per I7, the shell consumes only `closure_core` + `closure_store` —
//! no parser internals leak to the browser.

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write as _IoWrite};
use std::net::{TcpListener, TcpStream};

use closure_store::Vault;
use thiserror::Error;

/// Web shell errors.
#[derive(Debug, Error)]
pub enum WebError {
    /// Failed to bind the listening socket.
    #[error("bind: {0}")]
    Bind(String),
    /// IO failure while serving a request.
    #[error("io: {0}")]
    Io(String),
}

/// Bind a TCP listener at `addr` (e.g. `"127.0.0.1:7878"`) and serve
/// the vault read-only. Blocks until the listener fails.
pub fn serve(vault: &Vault, addr: &str) -> Result<(), WebError> {
    let listener = TcpListener::bind(addr).map_err(|e| WebError::Bind(e.to_string()))?;
    for stream in listener.incoming() {
        let stream = stream.map_err(|e| WebError::Io(e.to_string()))?;
        if let Err(e) = handle(stream, vault) {
            eprintln!("request error: {e}");
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, vault: &Vault) -> std::io::Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    // Discard headers up to blank line.
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
    }
    let body = render(vault);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body,
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn render(vault: &Vault) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
    html.push_str("<title>closure vault</title></head><body>");
    let _ = writeln!(html, "<h1>closure vault — {} file(s)</h1>", vault.len());
    for (path, doc) in vault.iter() {
        let _ = writeln!(html, "<h2>{}</h2><ul>", path.display());
        for h in doc.all_headlines() {
            let _ = writeln!(
                html,
                "<li>L{} {} — <code>{}</code></li>",
                h.level(),
                escape_html(h.title()),
                h.id()
            );
        }
        html.push_str("</ul>");
    }
    html.push_str("</body></html>");
    html
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
