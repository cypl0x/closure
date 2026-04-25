//! Cron-compatible scheduler. Parses cron strings and drives the
//! command registry at scheduled times.
//!
//! M5 skeleton: a rudimentary cron-spec parser and a `should_run` test.
//! Full scheduling loop lands later.

#![forbid(unsafe_code)]

use thiserror::Error;

/// A parsed cron spec (minute, hour, day-of-month, month, day-of-week).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSpec {
    /// Minute field (0-59 or `*`).
    pub minute: Field,
    /// Hour field (0-23 or `*`).
    pub hour: Field,
    /// Day of month (1-31 or `*`).
    pub dom: Field,
    /// Month (1-12 or `*`).
    pub month: Field,
    /// Day of week (0-6 or `*`).
    pub dow: Field,
    /// Command to invoke.
    pub command: String,
}

/// A cron field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    /// Matches any value.
    Any,
    /// Matches only the given value.
    Exact(u8),
}

/// Cron parse error.
#[derive(Debug, Error)]
pub enum CronError {
    /// Wrong number of fields.
    #[error("expected 5 time fields followed by a command, got `{0}`")]
    Shape(String),
    /// Bad number.
    #[error("bad number `{0}`")]
    Number(String),
}

/// Parse a `"* * * * * command-name"` style spec.
#[allow(clippy::missing_errors_doc)]
pub fn parse(s: &str) -> Result<CronSpec, CronError> {
    let parts: Vec<&str> = s.splitn(6, char::is_whitespace).collect();
    if parts.len() < 6 {
        return Err(CronError::Shape(s.into()));
    }
    Ok(CronSpec {
        minute: parse_field(parts[0])?,
        hour: parse_field(parts[1])?,
        dom: parse_field(parts[2])?,
        month: parse_field(parts[3])?,
        dow: parse_field(parts[4])?,
        command: parts[5].to_owned(),
    })
}

fn parse_field(s: &str) -> Result<Field, CronError> {
    if s == "*" {
        return Ok(Field::Any);
    }
    s.parse::<u8>()
        .map(Field::Exact)
        .map_err(|_| CronError::Number(s.into()))
}

/// Test whether `spec` matches a `(minute, hour, dom, month, dow)`
/// tuple. Used by [`Scheduler`] each tick to decide which jobs fire.
#[must_use]
pub const fn matches_time(spec: &CronSpec, m: u8, h: u8, d: u8, mo: u8, dw: u8) -> bool {
    field_matches(&spec.minute, m)
        && field_matches(&spec.hour, h)
        && field_matches(&spec.dom, d)
        && field_matches(&spec.month, mo)
        && field_matches(&spec.dow, dw)
}

const fn field_matches(f: &Field, v: u8) -> bool {
    match f {
        Field::Any => true,
        Field::Exact(x) => *x == v,
    }
}

/// In-process cron scheduler. Holds a list of `(spec, callback)`
/// pairs and `tick(time)` fires every callback whose spec matches.
///
/// Wiring `tick` to a timer thread that wakes once per minute is left
/// to the embedder so this crate stays free of `std::thread` /
/// `std::time` plumbing in tests.
pub struct Scheduler {
    jobs: Vec<(CronSpec, Box<dyn FnMut() + Send>)>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scheduler")
            .field("jobs", &self.jobs.len())
            .finish()
    }
}

impl Scheduler {
    /// New, empty scheduler.
    #[must_use]
    pub const fn new() -> Self {
        Self { jobs: Vec::new() }
    }

    /// Register a `(spec, callback)`.
    pub fn add<F: FnMut() + Send + 'static>(&mut self, spec: CronSpec, cb: F) {
        self.jobs.push((spec, Box::new(cb)));
    }

    /// Run every callback whose spec matches the supplied time tuple.
    pub fn tick(&mut self, m: u8, h: u8, d: u8, mo: u8, dw: u8) {
        for (spec, cb) in &mut self.jobs {
            if matches_time(spec, m, h, d, mo, dw) {
                cb();
            }
        }
    }
}
