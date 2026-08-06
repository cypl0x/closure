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
/// `POST /capture` and `POST /command`, so every route is
/// unit-testable without sockets.
pub fn respond(vault: &mut Vault, method: &str, target: &str, body: &str) -> Response {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match (method, path) {
        ("GET", "/") => Response::html(200, render(vault)),
        // Q6-W1: the declarative ViewTree as JSON — the client-side
        // renderer's data source, refreshed after each command.
        ("GET", "/view") => Response {
            status: 200,
            content_type: "application/json; charset=utf-8".to_owned(),
            body: closure_shell_core::view_to_json(&closure_shell_core::browse_view(vault)),
        },
        // Q6-W1 / Q9: registry-backed editing over HTTP (I8) — the
        // shared form dispatch lives in the store
        // (`Vault::apply_form_command`), so the endpoint and journal
        // replay speak one vocabulary.
        ("POST", "/command") => match vault.apply_form_command(body) {
            Ok(()) => Response {
                status: 200,
                content_type: "application/json; charset=utf-8".to_owned(),
                body: "{\"ok\":true}".to_owned(),
            },
            Err(e) if e.to_string().contains("unknown command") => {
                Response::html(400, "<p>unknown command</p>".to_owned())
            }
            Err(e) => Response::html(500, format!("<p>{}</p>", escape_html(&e.to_string()))),
        },
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
                target: std::path::PathBuf::from(closure_store::CAPTURE_FILE),
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
    // V12b: responsive — usable on a phone viewport.
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    html.push_str("<title>closure vault</title>");
    html.push_str(
        "<style>body{font-family:sans-serif;max-width:48em;margin:2em auto;padding:0 1em}\
         details{margin-left:1em}\
         .id{color:#888;font-size:0.8em;font-family:monospace}\
         .todo{color:#c00;font-weight:bold;margin-right:0.5em}\
         .tag{background:#eef;padding:0 0.3em;border-radius:0.2em;margin-left:0.3em}\
         @media(max-width:40em){body{margin:1em auto;padding:0 0.6em;font-size:1.05em}}</style>",
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
    html.push_str(&keymap_script());
    html.push_str("</body></html>");
    html
}

/// Q6-W2: the page's key table, sourced from the registry keymap
/// (`closure_input::chord_for_command`, I4/D6 — never hardcoded).
/// Single-stroke vim chords fire `POST /command` on the block whose
/// summary was last clicked; the page reloads on success.
fn keymap_script() -> String {
    let commands = [
        "undo",
        "redo",
        "promote",
        "demote",
        "delete",
        "add-sibling",
        "toggle-todo",
    ];
    let mut table = String::new();
    for cmd in commands {
        if let Some(chord) = closure_input::chord_for_command(closure_config::InputMode::Vim, cmd) {
            let _ = write!(table, "\"{}\":\"{}\",", escape_html(chord), cmd);
        }
    }
    format!(
        "<script>const KEYMAP={{{table}}};\
         let SEL=null;\
         document.querySelectorAll('.id').forEach(e=>e.parentElement.addEventListener('click',()=>{{SEL=e.textContent.trim();}}));\
         document.addEventListener('keydown',ev=>{{\
           if(ev.target.tagName==='INPUT')return;\
           const cmd=KEYMAP[ev.key];\
           if(!cmd||!SEL)return;\
           fetch('/command',{{method:'POST',headers:{{'Content-Type':'application/x-www-form-urlencoded'}},\
             body:'cmd='+encodeURIComponent(cmd)+'&id='+encodeURIComponent(SEL)}})\
             .then(r=>{{if(r.ok)location.reload();}});\
         }});</script>"
    )
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Inline vanilla-JS renderer (V13): rebuilds the `DOM` from the embedded
/// `VIEW` `ViewTree` `JSON`, setting each node's `ARIA` role. No
/// framework, no build step — just a function shipped inside the HTML.
const VIEW_RENDERER_JS: &str = "function render(n){\
const d=document.createElement('div');\
if(n.role)d.setAttribute('role',n.role);\
if(n.k==='pane'){const h=document.createElement('h2');h.textContent=n.title;d.appendChild(h);\
(n.children||[]).forEach(c=>d.appendChild(render(c)));}\
else if(n.k==='rows'){const ul=document.createElement('ul');\
(n.rows||[]).forEach((r,i)=>{const li=document.createElement('li');\
li.textContent=(r.todo?r.todo+' ':'')+r.title;if(i===n.selected)li.className='sel';\
ul.appendChild(li);});d.appendChild(ul);}\
else if(n.k==='detail'){(n.fields||[]).forEach(f=>{const p=document.createElement('div');\
p.textContent=f.label+': '+f.value+(f.chord?' ['+f.chord+']':'');d.appendChild(p);});}\
else if(n.k==='input'){const i=document.createElement('input');i.value=n.buffer;\
i.setAttribute('aria-label',n.label);d.appendChild(i);}\
else if(n.k==='palette'){const ul=document.createElement('ul');\
(n.items||[]).forEach(it=>{const li=document.createElement('li');\
li.textContent='['+it.chord+'] '+it.label;ul.appendChild(li);});d.appendChild(ul);}\
else if(n.k==='hints'){d.textContent=n.line;}\
else if(n.k==='widget'){const pre=document.createElement('pre');pre.textContent=n.content;\
d.appendChild(pre);}\
else if(n.k==='text'){d.textContent=n.text;}\
return d;}\
document.getElementById('app').appendChild(render(VIEW));";

/// Export the vault's browse [`ViewTree`](closure_shell_core::Node) as a
/// self-contained, declarative single HTML file (V13).
///
/// The `Node` tree is embedded as JSON and rebuilt client-side by an
/// inline vanilla-JS renderer — the same declarative description every
/// other shell renders, with no server and no toolchain. Closes the
/// "self-contained artifact" + "declarative UI" loop.
#[must_use]
pub fn export_view_html(vault: &Vault) -> String {
    let json = closure_shell_core::view_to_json(&closure_shell_core::browse_view(vault));
    let mut h = String::new();
    h.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    h.push_str(
        "<title>closure</title><style>body{font-family:sans-serif;max-width:48em;\
         margin:2em auto;padding:0 1em}li.sel{font-weight:bold}\
         @media(max-width:40em){body{margin:1em auto;padding:0 0.6em}}</style>",
    );
    h.push_str("</head><body><div id=\"app\"></div><script>\nconst VIEW=");
    h.push_str(&json);
    h.push_str(";\n");
    h.push_str(VIEW_RENDERER_JS);
    h.push_str("\n</script></body></html>");
    h
}

/// Render a shared [`closure_shell_core::Node`] view tree to HTML (V1b).
///
/// The web shell is one embedder of the declarative tree the engine
/// emits — the same `Node` the TUI renders. Actionable nodes surface
/// their chord in a `<kbd>` (the "keybinding everywhere" rule); all text
/// is HTML-escaped.
#[must_use]
// One flat arm per `Node` kind — exhaustive by design (V1c); splitting the
// match would only hide the one-to-one kind→HTML mapping.
#[allow(clippy::too_many_lines)]
pub fn render_view(node: &closure_shell_core::Node) -> String {
    use closure_shell_core::Node;
    use std::fmt::Write as _;
    match node {
        Node::Pane { title, children } => {
            let inner = children.iter().fold(String::new(), |mut s, c| {
                s.push_str(&render_view(c));
                s
            });
            // V12a: emit the node's semantic role + accessible label.
            format!(
                "<section role=\"{}\" aria-label=\"{}\"><h2>{}</h2>{inner}</section>",
                node.aria_role(),
                escape_html(title),
                escape_html(title)
            )
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
                    let icon = r.icon.as_deref().map_or_else(String::new, |g| {
                        format!("<span class=\"icon\">{}</span> ", escape_html(g))
                    });
                    let badges = r.badges.iter().fold(String::new(), |mut b, badge| {
                        let _ = write!(b, " <span class=\"badge\">{}</span>", escape_html(badge));
                        b
                    });
                    let _ = write!(
                        s,
                        "<li{sel}>{icon}{todo}{}{badges}</li>",
                        escape_html(&r.title)
                    );
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
            "<label>{} <input aria-label=\"{}\" value=\"{}\"></label>",
            escape_html(label),
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
        Node::Split { direction, panes } => {
            let inner = panes.iter().fold(String::new(), |mut s, p| {
                s.push_str(&render_view(p));
                s
            });
            format!(
                "<div role=\"{}\" class=\"split split-{}\">{inner}</div>",
                node.aria_role(),
                direction.as_str()
            )
        }
        Node::Modal { title, body } => format!(
            "<div role=\"{}\" aria-label=\"{}\" class=\"modal\"><h2>{}</h2>{}</div>",
            node.aria_role(),
            escape_html(title),
            escape_html(title),
            render_view(body)
        ),
        Node::Toast { level, text } => format!(
            "<div role=\"{}\" class=\"toast toast-{}\">{}</div>",
            node.aria_role(),
            level.as_str(),
            escape_html(text)
        ),
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

/// Map a typed theme to CSS custom properties (G2).
///
/// Returns the declarative `:root` variable block a styled page consumes
/// — one source of tokens; the browser is the native styling layer.
#[must_use]
pub fn theme_css_variables(theme: &closure_shell_core::Theme) -> String {
    use closure_shell_core::ColorRole::{
        Accent, Bg, Code, Error, Fg, Heading2, Heading3, Muted, Selection, Success, Warning,
    };
    let p = |role| theme.color(role).hex();
    format!(
        "--fg:{};--bg:{};--accent:{};--muted:{};--selection:{};--error:{};--warning:{};\
         --success:{};--heading2:{};--heading3:{};--code:{};--space:{}px;--gap:{}px;\
         --font:{};--mono:{};--font-size:{}px;",
        p(Fg),
        p(Bg),
        p(Accent),
        p(Muted),
        p(Selection),
        p(Error),
        p(Warning),
        p(Success),
        p(Heading2),
        p(Heading3),
        p(Code),
        theme.spacing.unit_px,
        theme.spacing.gap_px,
        theme.typography.font_family,
        theme.typography.mono_family,
        theme.typography.base_px,
    )
}
