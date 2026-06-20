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

/// An HTTP response produced by [`respond`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// HTTP status code.
    pub status: u16,
    /// `Content-Type` header value.
    pub content_type: String,
    /// Response body.
    pub body: String,
}

impl Response {
    fn html(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "text/html; charset=utf-8".to_owned(),
            body,
        }
    }
}

/// Route a request to a response. Pure aside from vault writes on
/// `POST /capture`, so every route is unit-testable without sockets.
pub fn respond(vault: &mut Vault, method: &str, target: &str, body: &str) -> Response {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match (method, path) {
        ("GET", "/") => Response::html(200, render(vault)),
        ("GET", "/search") => {
            let q = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("q="))
                .map(url_decode)
                .unwrap_or_default();
            Response::html(200, render_search(vault, &q))
        }
        ("POST", "/capture") => {
            let title = body
                .split('&')
                .find_map(|kv| kv.strip_prefix("title="))
                .map(url_decode)
                .unwrap_or_default();
            if title.is_empty() {
                return Response::html(400, "<p>title required</p>".to_owned());
            }
            let template = closure_store::CaptureTemplate {
                target: std::path::PathBuf::from("inbox.org"),
                headline_prefix: "TODO ".to_owned(),
                body: String::new(),
            };
            match vault.capture(&template, &title) {
                Ok(_) => Response {
                    status: 303,
                    content_type: "text/html; charset=utf-8".to_owned(),
                    body: String::new(),
                },
                Err(e) => Response::html(500, format!("<p>{}</p>", escape_html(&e.to_string()))),
            }
        }
        _ => Response::html(404, "<p>not found</p>".to_owned()),
    }
}

/// Decode `+` and `%XX` form encoding.
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                if let Some(h) = s.get(i + 1..i + 3)
                    && let Ok(v) = u8::from_str_radix(h, 16)
                {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn render_search(vault: &Vault, q: &str) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
    html.push_str("<title>closure search</title></head><body>");
    let _ = writeln!(
        html,
        "<form action=\"/search\" method=\"get\">\
         <input name=\"q\" value=\"{}\" autofocus>\
         <button>search</button></form>",
        escape_html(q)
    );
    if !q.is_empty() {
        html.push_str("<ul>");
        for (path, doc) in vault.iter() {
            for h in doc.all_headlines() {
                if closure_query::fuzzy_score(q, h.title()).is_some() {
                    let _ = writeln!(
                        html,
                        "<li>{} <span class=\"id\">({})</span></li>",
                        escape_html(h.title()),
                        escape_html(&path.display().to_string())
                    );
                }
            }
        }
        html.push_str("</ul>");
    }
    html.push_str("<p><a href=\"/\">back</a></p></body></html>");
    html
}

/// Bind a TCP listener at `addr` (e.g. `"127.0.0.1:7878"`) and serve
/// the vault. Blocks until the listener fails. Captures (`POST
/// /capture`) write back through the vault, hence the mutable borrow.
pub fn serve(vault: &mut Vault, addr: &str) -> Result<(), WebError> {
    let listener = TcpListener::bind(addr).map_err(|e| WebError::Bind(e.to_string()))?;
    for stream in listener.incoming() {
        let stream = stream.map_err(|e| WebError::Io(e.to_string()))?;
        if let Err(e) = handle(stream, vault) {
            eprintln!("request error: {e}");
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, vault: &mut Vault) -> std::io::Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_owned();
    let target = parts.next().unwrap_or("/").to_owned();
    // Read headers, tracking Content-Length for POST bodies.
    let mut content_length = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body_bytes = vec![0u8; content_length];
    if content_length > 0 {
        use std::io::Read as _;
        reader.read_exact(&mut body_bytes)?;
    }
    let body = String::from_utf8_lossy(&body_bytes).into_owned();
    let r = respond(vault, &method, &target, &body);
    let location = if r.status == 303 {
        "Location: /\r\n"
    } else {
        ""
    };
    let response = format!(
        "HTTP/1.1 {} X\r\nContent-Type: {}\r\n{}Content-Length: {}\r\n\r\n{}",
        r.status,
        r.content_type,
        location,
        r.body.len(),
        r.body,
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn render(vault: &Vault) -> String {
    let mut html = String::new();
    html.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
    html.push_str("<title>closure vault</title>");
    html.push_str(
        "<style>body{font-family:sans-serif;max-width:48em;margin:2em auto;padding:0 1em}\
         details{margin-left:1em}\
         .id{color:#888;font-size:0.8em;font-family:monospace}\
         .todo{color:#c00;font-weight:bold;margin-right:0.5em}\
         .tag{background:#eef;padding:0 0.3em;border-radius:0.2em;margin-left:0.3em}</style>",
    );
    html.push_str("</head><body>");
    let _ = writeln!(html, "<h1>closure vault — {} file(s)</h1>", vault.len());
    html.push_str(
        "<p><a href=\"/search\">search</a></p>\
         <form action=\"/capture\" method=\"post\">\
         <input name=\"title\" placeholder=\"capture a TODO\">\
         <button>capture</button></form>",
    );
    for (path, doc) in vault.iter() {
        let _ = writeln!(html, "<details open><summary>{}</summary>", path.display());
        let mut open_level: u8 = 0;
        for h in doc.all_headlines() {
            while open_level >= h.level() && open_level > 0 {
                html.push_str("</details>");
                open_level -= 1;
            }
            html.push_str("<details open><summary>");
            if let Some(t) = h.todo() {
                let _ = write!(html, "<span class=\"todo\">{}</span>", escape_html(t));
            }
            let _ = write!(html, "{}", escape_html(h.title()));
            for tag in h.tags() {
                let _ = write!(html, "<span class=\"tag\">{}</span>", escape_html(tag));
            }
            let _ = writeln!(
                html,
                " <span class=\"id\">{}</span></summary>",
                escape_html(&h.id().to_string())
            );
            open_level = h.level();
        }
        while open_level > 0 {
            html.push_str("</details>");
            open_level -= 1;
        }
        html.push_str("</details>");
    }
    html.push_str("</body></html>");
    html
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Render a shared [`closure_shell_core::Node`] view tree to HTML (V1b).
///
/// The web shell is one embedder of the declarative tree the engine
/// emits — the same `Node` the TUI renders. Actionable nodes surface
/// their chord in a `<kbd>` (the "keybinding everywhere" rule); all text
/// is HTML-escaped.
#[must_use]
pub fn render_view(node: &closure_shell_core::Node) -> String {
    use closure_shell_core::Node;
    use std::fmt::Write as _;
    match node {
        Node::Pane { title, children } => {
            let inner = children.iter().fold(String::new(), |mut s, c| {
                s.push_str(&render_view(c));
                s
            });
            format!("<section><h2>{}</h2>{inner}</section>", escape_html(title))
        }
        Node::Rows { rows, selected } => {
            let items = rows
                .iter()
                .enumerate()
                .fold(String::new(), |mut s, (i, r)| {
                    let sel = if i == *selected { " class=\"sel\"" } else { "" };
                    let todo = r.todo.as_deref().map_or_else(String::new, |t| {
                        format!("<span class=\"todo\">{}</span> ", escape_html(t))
                    });
                    let _ = write!(s, "<li{sel}>{todo}{}</li>", escape_html(&r.title));
                    s
                });
            format!("<ul class=\"rows\">{items}</ul>")
        }
        Node::Detail { fields } => {
            let items = fields.iter().fold(String::new(), |mut s, f| {
                let kbd = f.action.as_ref().map_or_else(String::new, |a| {
                    format!(" <kbd>{}</kbd>", escape_html(a.chord()))
                });
                let _ = write!(
                    s,
                    "<dt>{}</dt><dd>{}{kbd}</dd>",
                    escape_html(&f.label),
                    escape_html(&f.value)
                );
                s
            });
            format!("<dl class=\"detail\">{items}</dl>")
        }
        Node::Input { label, buffer } => format!(
            "<label>{} <input value=\"{}\"></label>",
            escape_html(label),
            escape_html(buffer)
        ),
        Node::Palette { items, cursor } => {
            let lis = items
                .iter()
                .enumerate()
                .fold(String::new(), |mut s, (i, it)| {
                    let sel = if i == *cursor { " class=\"sel\"" } else { "" };
                    let _ = write!(
                        s,
                        "<li{sel}><kbd>{}</kbd> {}</li>",
                        escape_html(it.action.chord()),
                        escape_html(&it.label)
                    );
                    s
                });
            format!("<ul class=\"palette\">{lis}</ul>")
        }
        Node::Hints { line } => format!("<footer>{}</footer>", escape_html(line)),
        Node::Widget { name, content } => format!(
            "<section class=\"widget\" data-widget=\"{}\"><pre>{}</pre></section>",
            escape_html(name),
            escape_html(content)
        ),
        Node::Text(t) => format!("<p>{}</p>", escape_html(t)),
    }
}

/// Export the vault as a self-contained single HTML file (no server, no external deps).
///
/// Includes the vault tree (browse) + client-side JS for fuzzy search (as per vision/ROADMAP).
/// The JS does a simple includes filter (portable "fuzzy" idea); can be enhanced with full score.
#[must_use]
pub fn export_html(vault: &Vault) -> String {
    let mut html = String::from(
        r#"<!doctype html>
<html><head><meta charset="utf-8">
<title>closure vault (self-contained single file)</title>
<style>
body { font-family: sans-serif; max-width: 48em; margin: 2em auto; padding: 0 1em; }
details { margin-left: 1em; }
.id { color: #888; font-size: 0.8em; font-family: monospace; }
.todo { color: #c00; font-weight: bold; margin-right: 0.5em; }
.tag { background: #eef; padding: 0 0.3em; border-radius: 0.2em; margin-left: 0.3em; }
#search { width: 100%; padding: 0.5em; margin-bottom: 1em; font-size: 1em; }
li.hidden { display: none; }
</style>
</head><body>
<h1>closure vault (self-contained) — "#,
    );
    let _ = write!(html, "{} file(s)</h1>", vault.len());
    html.push_str(
        r#"<input id="search" placeholder="fuzzy search (client-side JS)" autofocus>
<ul id="list">"#,
    );

    // Build a simple list of headlines (path + title) for the client JS to filter.
    // Static tree for initial browse (simplified from the server render logic).
    for (path, doc) in vault.iter() {
        let _ = write!(
            html,
            "<li><strong>{}</strong><ul>",
            escape_html(&path.display().to_string())
        );
        for h in doc.all_headlines() {
            let mut line = String::new();
            if let Some(t) = h.todo() {
                let _ = write!(line, r#"<span class="todo">{}</span>"#, escape_html(t));
            }
            let _ = write!(line, "{}", escape_html(h.title()));
            for tag in h.tags() {
                let _ = write!(line, r#"<span class="tag">{}</span>"#, escape_html(tag));
            }
            let _ = write!(
                line,
                r#" <span class="id">({})</span>"#,
                escape_html(&h.id().to_string())
            );
            let _ = write!(html, "<li>{line}</li>");
        }
        html.push_str("</ul></li>");
    }
    html.push_str(
        r"</ul>
<script>
// Client-side fuzzy/search (includes-based for portability; idea from the Rust fuzzy_score).
const input = document.getElementById('search');
const list = document.getElementById('list');
input.addEventListener('input', () => {
  const q = input.value.toLowerCase();
  list.querySelectorAll('li').forEach(li => {
    const text = li.textContent.toLowerCase();
    li.classList.toggle('hidden', q && !text.includes(q));
  });
});
console.log('self-contained closure export ready');
</script>
<!-- TODO (not implemented): inline a wasm32 build of closure-org so the
     page can re-parse edited org client-side. Today the export is a
     read-only browse+search snapshot; no wasm is embedded. -->
</body></html>",
    );
    html
}
