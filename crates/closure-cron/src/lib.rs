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

/// The five fields as the cron expression they were parsed from.
///
/// The Jobs pane printed `{:?}` of the spec — `CronSpec { minute:
/// Exact(0), … }` — which is neither what the user wrote nor anything
/// they would recognise. This is what they wrote.
#[must_use]
pub fn expression(spec: &CronSpec) -> String {
    format!(
        "{} {} {} {} {}",
        field_str(&spec.minute),
        field_str(&spec.hour),
        field_str(&spec.dom),
        field_str(&spec.month),
        field_str(&spec.dow)
    )
}

/// One field, back in cron's own spelling.
fn field_str(f: &Field) -> String {
    match f {
        Field::Any => "*".to_owned(),
        Field::Exact(n) => n.to_string(),
        Field::List(v) => v
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
        Field::Range(a, b) => format!("{a}-{b}"),
        Field::Step(n) => format!("*/{n}"),
    }
}

/// When this fires, in a sentence — or the expression when there is no
/// sentence for it.
///
/// Deliberately narrow. Only the shapes people actually write get
/// words; everything else falls back to the expression, because a
/// wrong sentence about when a job runs is worse than a cron line you
/// have to read. "every day at 09:00" is what you check against what
/// you meant.
#[must_use]
pub fn describe(spec: &CronSpec) -> String {
    const DAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let daily = matches!(spec.dom, Field::Any) && matches!(spec.month, Field::Any);
    match (&spec.minute, &spec.hour, &spec.dow) {
        // `*/N * * * *`
        (Field::Step(n), Field::Any, Field::Any) if daily => {
            format!("every {n} minutes")
        }
        // `M H * * *`
        (Field::Exact(m), Field::Exact(h), Field::Any) if daily => {
            format!("every day at {h:02}:{m:02}")
        }
        // `M H * * D`
        (Field::Exact(m), Field::Exact(h), Field::Exact(d)) if daily => {
            DAYS.get(*d as usize).map_or_else(
                || expression(spec),
                |day| format!("every {day} at {h:02}:{m:02}"),
            )
        }
        _ => expression(spec),
    }
}

/// A cron field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    /// Matches any value.
    Any,
    /// Matches only the given value.
    Exact(u8),
    /// Matches any value in the list.
    List(Vec<u8>),
    /// Matches values in the inclusive range `start..=end`.
    Range(u8, u8),
    /// Matches every Nth value (`*/N`).
    Step(u8),
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
    if let Some(rest) = s.strip_prefix("*/") {
        let step = rest
            .parse::<u8>()
            .map_err(|_| CronError::Number(s.into()))?;
        if step == 0 {
            return Err(CronError::Number(s.into()));
        }
        return Ok(Field::Step(step));
    }
    if let Some((lo, hi)) = s.split_once('-') {
        let lo = lo.parse::<u8>().map_err(|_| CronError::Number(s.into()))?;
        let hi = hi.parse::<u8>().map_err(|_| CronError::Number(s.into()))?;
        return Ok(Field::Range(lo, hi));
    }
    if s.contains(',') {
        let parts: Result<Vec<u8>, _> = s
            .split(',')
            .map(|p| p.parse::<u8>().map_err(|_| CronError::Number(s.into())))
            .collect();
        return Ok(Field::List(parts?));
    }
    s.parse::<u8>()
        .map(Field::Exact)
        .map_err(|_| CronError::Number(s.into()))
}

/// Test whether `spec` matches a `(minute, hour, dom, month, dow)`
/// tuple. Used by [`Scheduler`] each tick to decide which jobs fire.
#[must_use]
pub fn matches_time(spec: &CronSpec, m: u8, h: u8, d: u8, mo: u8, dw: u8) -> bool {
    field_matches(&spec.minute, m)
        && field_matches(&spec.hour, h)
        && field_matches(&spec.dom, d)
        && field_matches(&spec.month, mo)
        && field_matches(&spec.dow, dw)
}

fn field_matches(f: &Field, v: u8) -> bool {
    match f {
        Field::Any => true,
        Field::Exact(x) => *x == v,
        Field::List(xs) => xs.contains(&v),
        Field::Range(lo, hi) => v >= *lo && v <= *hi,
        Field::Step(step) => *step != 0 && v.is_multiple_of(*step),
    }
}

/// Bind a [`CronSpec`] to a registered command name. Drives the
/// scheduler when the embedder owns a `closure_core::Registry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// When to fire.
    pub spec: CronSpec,
    /// Registry command name to invoke.
    pub command: String,
}

impl Job {
    /// Build a job from a parsed spec and a command name.
    #[must_use]
    pub fn new(spec: CronSpec, command: impl Into<String>) -> Self {
        Self {
            spec,
            command: command.into(),
        }
    }

    /// Whether this job should fire at the given wall-clock tuple.
    #[must_use]
    pub fn matches(&self, m: u8, h: u8, d: u8, mo: u8, dw: u8) -> bool {
        matches_time(&self.spec, m, h, d, mo, dw)
    }
}

/// Parse a multi-line cron block into a list of jobs. One spec per
/// line. Blank lines and `#`-prefixed comments are ignored. The
/// trailing `command` token of each spec becomes the job command.
#[allow(clippy::missing_errors_doc)]
pub fn parse_jobs(content: &str) -> Result<Vec<Job>, CronError> {
    let mut out: Vec<Job> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let spec = parse(trimmed)?;
        let command = spec.command.clone();
        out.push(Job::new(spec, command));
    }
    Ok(out)
}

/// Compute the next `(minute, hour)` after `now_min`, `now_hour`.
///
/// Searches forward up to 24 hours; returns `None` if no match is
/// found in that window. Day-of-month / month / day-of-week fields
/// are ignored — caller drives them externally.
#[must_use]
pub fn next_match_today(spec: &CronSpec, now_min: u8, now_hour: u8) -> Option<(u8, u8)> {
    for delta in 1..=(24u32 * 60) {
        let total = u32::from(now_hour) * 60 + u32::from(now_min) + delta;
        let m = (total % 60) as u8;
        let h = ((total / 60) % 24) as u8;
        if field_matches(&spec.minute, m) && field_matches(&spec.hour, h) {
            return Some((m, h));
        }
    }
    None
}

/// Filter a slice of jobs to those that match `time`.
#[must_use]
pub fn jobs_matching(jobs: &[Job], m: u8, h: u8, d: u8, mo: u8, dw: u8) -> Vec<&Job> {
    jobs.iter().filter(|j| j.matches(m, h, d, mo, dw)).collect()
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
