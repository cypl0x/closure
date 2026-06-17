//! Lean, serde-free JSON helpers + JSON-RPC envelope builders.
//!
//! Shared by the MCP / ACP / A2A / LSP bridges so the hand-rolled JSON
//! field extraction and response framing live in one tested place
//! (previously copied verbatim into each crate). Not a general-purpose
//! JSON library — just enough for the bridges' flat request objects and
//! `{"jsonrpc":"2.0",...}` responses. No dependency, no `serde` (I10).

#![forbid(unsafe_code)]

/// The raw token after `"key":`.
///
/// A number, `true`/`false`/`null`, or a re-quoted+escaped string. Used
/// for the JSON-RPC `id`, which echoes back verbatim (a number stays a
/// number, a string stays a string). `None` when the key is absent.
#[must_use]
pub fn raw_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let at = json.find(&needle)?;
    let rest = json[at + needle.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if rest.starts_with('"') {
        return string_value(rest).map(|s| format!("\"{}\"", json_escape(&s)));
    }
    let end = rest.find([',', '}', ']']).unwrap_or(rest.len());
    let tok = rest[..end].trim();
    if tok.is_empty() {
        None
    } else {
        Some(tok.to_owned())
    }
}

/// The unescaped string value of `"key":"..."`. `None` when the key is
/// absent or its value is not a string literal.
#[must_use]
pub fn string_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let at = json.find(&needle)?;
    let rest = json[at + needle.len()..].trim_start();
    string_value(rest.strip_prefix(':')?.trim_start())
}

/// Parse a JSON string literal at the start of `s`, unescaping
/// `\" \\ \n \t \r` (and passing other escapes through verbatim).
#[must_use]
pub fn string_value(s: &str) -> Option<String> {
    let mut chars = s.strip_prefix('"')?.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                other => out.push(other),
            },
            other => out.push(other),
        }
    }
    None
}

/// Escape `s` for embedding inside a JSON string literal (quotes,
/// backslash, and the common control whitespace).
#[must_use]
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// Build a JSON-RPC success envelope: `{"jsonrpc":"2.0","id":<id>,
/// "result":<result>}`. `id` is the raw token from [`raw_field`]
/// (already quoted if it was a string); `result` is a JSON fragment.
#[must_use]
pub fn response(id: &str, result: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result}}}")
}

/// Build a JSON-RPC error envelope: `{"jsonrpc":"2.0","id":<id>,
/// "error":{"code":<code>,"message":<message>}}`. `message` is escaped.
#[must_use]
pub fn error(id: &str, code: i64, message: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":{code},\"message\":\"{}\"}}}}",
        json_escape(message)
    )
}

/// The standard JSON-RPC `-32601 method not found` error for `id`.
#[must_use]
pub fn method_not_found(id: &str) -> String {
    error(id, -32601, "method not found")
}
