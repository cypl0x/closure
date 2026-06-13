//! Network sniffer. Observation-only at first. Composable filters
//! (Little Snitch / mitmproxy / Wireshark style).
//!
//! Lives in the closure ecosystem but is its own binary — it does not
//! link into any shell.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Configured filter rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Human-readable identifier.
    pub id: String,
    /// Glob pattern (`*` wildcards) applied to the candidate string.
    pub pattern: String,
    /// What the rule does on a match.
    pub action: Action,
}

/// What a rule does when its pattern matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Allow the request.
    Allow,
    /// Block the request.
    Block,
    /// Log the request (continue allowed by default).
    Log,
}

/// Sniffer error.
#[derive(Debug, Error)]
pub enum SnifferError {
    /// Binding a socket failed.
    #[error("bind: {0}")]
    Bind(String),
}

/// Match `candidate` against the first rule whose pattern matches.
/// Returns the action of that rule, or `None` if no rule matched.
#[must_use]
pub fn match_first<'a>(candidate: &str, rules: &'a [Rule]) -> Option<&'a Rule> {
    rules.iter().find(|r| glob_matches(&r.pattern, candidate))
}

/// Tiny glob matcher: `*` matches any run of characters; everything
/// else is literal. No `?`, no `[abc]`, no escaping. Sufficient for
/// host/path patterns.
#[must_use]
pub fn glob_matches(pattern: &str, candidate: &str) -> bool {
    glob_inner(pattern.as_bytes(), candidate.as_bytes())
}

fn glob_inner(pat: &[u8], cand: &[u8]) -> bool {
    if pat.is_empty() {
        return cand.is_empty();
    }
    if pat[0] == b'*' {
        // Match zero or more of any byte.
        if pat.len() == 1 {
            return true;
        }
        for i in 0..=cand.len() {
            if glob_inner(&pat[1..], &cand[i..]) {
                return true;
            }
        }
        return false;
    }
    if cand.is_empty() {
        return false;
    }
    if pat[0] == cand[0] {
        return glob_inner(&pat[1..], &cand[1..]);
    }
    false
}

/// Capture backend trait (pluggable for live sniffer, mock for tests, external for tcpdump/ss).
/// Core stays dependency-free (no pcap etc in this crate).
pub trait CaptureBackend {
    /// Given a candidate (e.g. "host:port proto"), return the action if a rule matches.
    fn match_action(&self, candidate: &str) -> Option<Action>;
}

/// Mock backend for tests (uses in-memory rules + existing `glob/match_first`).
#[derive(Debug, Clone)]
pub struct MockBackend {
    rules: Vec<Rule>,
}

impl MockBackend {
    /// Create a mock with the given rules (for tests).
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }
}

impl CaptureBackend for MockBackend {
    fn match_action(&self, candidate: &str) -> Option<Action> {
        match_first(candidate, &self.rules).map(|r| r.action)
    }
}

/// Append a capture as an org headline to the given network.org path (org-native log).
///
/// Headline format: * <ts> host=<host> proto=<proto>
/// (Simple append for lean sniffer; later can use Vault for span-preserving if needed.)
pub fn log_capture_to_org(
    path: &std::path::Path,
    host: &str,
    proto: &str,
    ts: &str,
) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let line = format!("* <{ts}> host={host} proto={proto}\n");
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}
