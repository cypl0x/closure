//! Lean, serde-free JSON helpers + JSON-RPC envelope builders.
//!
//! Shared by the MCP / ACP / A2A / LSP bridges so the hand-rolled JSON
//! field extraction and response framing live in one tested place
//! (previously copied verbatim into each crate). Not a general-purpose
//! JSON library — just enough for the bridges' flat request objects and
//! `{"jsonrpc":"2.0",...}` responses. No dependency, no `serde` (I10).

#![forbid(unsafe_code)]

/// Where the *key* `"key"` starts — a quoted string, in key
/// position, with a `:` after it.
///
/// Plain `find("\"key\"")` matches the key's own spelling anywhere,
/// including inside a value: `{"type":"text","text":"hi"}` asked for
/// `text` found the `"text"` that is `type`'s value, saw a comma
/// instead of a colon, and answered "absent". So this walks the
/// string boundaries rather than searching the bytes.
fn find_key(json: &str, key: &str) -> Option<usize> {
    let bytes = json.as_bytes();
    let mut i = 0_usize;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        // The end of this string literal, honouring escapes.
        let start = i;
        let mut j = i + 1;
        while j < bytes.len() {
            match bytes[j] {
                b'\\' => j += 2,
                b'"' => break,
                _ => j += 1,
            }
        }
        if j >= bytes.len() {
            return None;
        }
        let is_key =
            json.get(start + 1..j) == Some(key) && json[j + 1..].trim_start().starts_with(':');
        if is_key {
            return Some(start);
        }
        i = j + 1;
    }
    None
}

/// The raw token after `"key":`.
///
/// A number, `true`/`false`/`null`, or a re-quoted+escaped string. Used
/// for the JSON-RPC `id`, which echoes back verbatim (a number stays a
/// number, a string stays a string). `None` when the key is absent.
#[must_use]
pub fn raw_field(json: &str, key: &str) -> Option<String> {
    let at = find_key(json, key)?;
    let rest = json[at + key.len() + 2..].trim_start();
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
    let at = find_key(json, key)?;
    let rest = json[at + key.len() + 2..].trim_start();
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

/// The balanced raw value after `"key":` — an object, an array, a
/// string literal or a scalar, exactly as written.
///
/// [`raw_field`] stops at the first `,`/`}`/`]`, which is right for a
/// JSON-RPC `id` and wrong for anything structural. This is the
/// structural one: it counts brackets, so `"result":{"tools":[…]}`
/// comes back whole. Like the rest of these it takes the *first* key
/// of that name at any depth, which is what a flat bridge object
/// needs and is why the bridges keep their replies flat.
#[must_use]
pub fn raw_value(json: &str, key: &str) -> Option<String> {
    let at = find_key(json, key)?;
    let rest = json[at + key.len() + 2..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    match rest.chars().next()? {
        '"' => string_value(rest).map(|s| format!("\"{}\"", json_escape(&s))),
        open @ ('{' | '[') => {
            let close = if open == '{' { '}' } else { ']' };
            let mut depth = 0_i32;
            let mut in_str = false;
            let mut escaped = false;
            for (i, c) in rest.char_indices() {
                if in_str {
                    match c {
                        _ if escaped => escaped = false,
                        '\\' => escaped = true,
                        '"' => in_str = false,
                        _ => {}
                    }
                    continue;
                }
                match c {
                    '"' => in_str = true,
                    c if c == open => depth += 1,
                    c if c == close => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(rest[..=i].to_owned());
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        _ => {
            let end = rest.find([',', '}', ']']).unwrap_or(rest.len());
            let tok = rest[..end].trim();
            (!tok.is_empty()).then(|| tok.to_owned())
        }
    }
}

/// The top-level items of a JSON array, each as written.
///
/// The companion to [`raw_value`]: a bridge that *reads* a reply has
/// to walk `"tools":[{…},{…}]`, and splitting on commas gets that
/// wrong the moment a description contains one.
#[must_use]
pub fn array_items(array: &str) -> Vec<String> {
    let Some(inner) = array
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let (mut depth, mut in_str, mut escaped, mut start) = (0_i32, false, false, 0_usize);
    for (i, c) in inner.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                let item = inner[start..i].trim();
                if !item.is_empty() {
                    out.push(item.to_owned());
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = inner[start..].trim();
    if !last.is_empty() {
        out.push(last.to_owned());
    }
    out
}

/// Build a JSON-RPC request: `{"jsonrpc":"2.0","id":<id>,
/// "method":"<method>","params":<params>}`.
///
/// The client side of [`response`] — closure speaks both halves of
/// this protocol family (it serves MCP and consumes it), and the
/// envelope should be built in one place either way.
#[must_use]
pub fn request(id: u64, method: &str, params: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"{}\",\"params\":{params}}}",
        json_escape(method)
    )
}

/// Build a JSON-RPC notification — a request with no `id`, so no
/// reply is expected and none may be waited for.
#[must_use]
pub fn notification(method: &str, params: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"method\":\"{}\",\"params\":{params}}}",
        json_escape(method)
    )
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
