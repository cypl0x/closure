//! Tree / tag / text / backlink queries over a [`Vault`].
//!
//! All queries return borrowed matches over the vault's cached
//! `DocHeadline` records (stable block ids, I2) — no re-parsing.

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::path::Path;

use closure_core::{BlockId, DocHeadline};
use closure_store::Vault;

/// A query match: the file path and the matched headline.
#[derive(Debug, Clone, Copy)]
pub struct Match<'a> {
    /// File containing the headline.
    pub path: &'a Path,
    /// The matched headline record.
    pub headline: &'a DocHeadline,
}

/// True iff this headline carries the `ARCHIVE` tag.
#[must_use]
pub fn is_archived(h: &DocHeadline) -> bool {
    h.tags().iter().any(|t| t == "ARCHIVE")
}

/// All non-archived headlines in the vault.
#[must_use]
pub fn not_archived(vault: &Vault) -> Vec<Match<'_>> {
    all_headlines(vault)
        .into_iter()
        .filter(|m| !is_archived(m.headline))
        .collect()
}

/// All headlines in the vault, depth-first per file.
#[must_use]
pub fn all_headlines(vault: &Vault) -> Vec<Match<'_>> {
    let mut out: Vec<Match<'_>> = Vec::new();
    for (path, doc) in vault.iter() {
        for h in doc.all_headlines() {
            out.push(Match { path, headline: h });
        }
    }
    out
}

/// Headlines whose properties drawer contains `key` with `value`.
#[must_use]
pub fn by_property<'a>(vault: &'a Vault, key: &str, value: &str) -> Vec<Match<'a>> {
    all_headlines(vault)
        .into_iter()
        .filter(|m| m.headline.property(key) == Some(value))
        .collect()
}

/// Headlines that have *every* tag in `tags` (AND filter).
#[must_use]
pub fn by_tags_all<'a>(vault: &'a Vault, tags: &[&str]) -> Vec<Match<'a>> {
    all_headlines(vault)
        .into_iter()
        .filter(|m| {
            tags.iter()
                .all(|wanted| m.headline.tags().iter().any(|t| t == wanted))
        })
        .collect()
}

/// Headlines that have *any* tag in `tags` (OR filter).
#[must_use]
pub fn by_tags_any<'a>(vault: &'a Vault, tags: &[&str]) -> Vec<Match<'a>> {
    all_headlines(vault)
        .into_iter()
        .filter(|m| {
            tags.iter()
                .any(|wanted| m.headline.tags().iter().any(|t| t == wanted))
        })
        .collect()
}

/// Headlines with a specific tag.
#[must_use]
pub fn by_tag<'a>(vault: &'a Vault, tag: &str) -> Vec<Match<'a>> {
    all_headlines(vault)
        .into_iter()
        .filter(|m| m.headline.tags().iter().any(|t| t == tag))
        .collect()
}

/// Headlines with a specific priority letter (`'A'`, `'B'`, etc).
#[must_use]
pub fn by_priority(vault: &Vault, priority: char) -> Vec<Match<'_>> {
    all_headlines(vault)
        .into_iter()
        .filter(|m| m.headline.priority() == Some(priority))
        .collect()
}

/// Headlines that have any priority cookie set.
#[must_use]
pub fn with_priority(vault: &Vault) -> Vec<Match<'_>> {
    all_headlines(vault)
        .into_iter()
        .filter(|m| m.headline.priority().is_some())
        .collect()
}

/// Headlines with a specific TODO keyword.
#[must_use]
pub fn by_todo<'a>(vault: &'a Vault, keyword: &str) -> Vec<Match<'a>> {
    all_headlines(vault)
        .into_iter()
        .filter(|m| m.headline.todo() == Some(keyword))
        .collect()
}

/// Headlines whose title contains `needle` (case-sensitive).
#[must_use]
pub fn by_title_substring<'a>(vault: &'a Vault, needle: &str) -> Vec<Match<'a>> {
    all_headlines(vault)
        .into_iter()
        .filter(|m| m.headline.title().contains(needle))
        .collect()
}

/// Full-text search: headlines whose title OR body contains `needle`
/// (case-sensitive).
#[must_use]
pub fn full_text<'a>(vault: &'a Vault, needle: &str) -> Vec<Match<'a>> {
    all_headlines(vault)
        .into_iter()
        .filter(|m| m.headline.title().contains(needle) || m.headline.body_text().contains(needle))
        .collect()
}

/// Headlines at a specific nesting level.
#[must_use]
pub fn by_level(vault: &Vault, level: u8) -> Vec<Match<'_>> {
    all_headlines(vault)
        .into_iter()
        .filter(|m| m.headline.level() == level)
        .collect()
}

/// Headlines that link to the given block id via an `id:<ULID>` target.
#[must_use]
pub fn backlinks<'a>(vault: &'a Vault, target: &BlockId) -> Vec<Match<'a>> {
    let needle_id = format!("id:{}", target.as_str());
    all_headlines(vault)
        .into_iter()
        .filter(|m| {
            m.headline
                .link_targets()
                .iter()
                .any(|t| t == &needle_id || t == target.as_str())
        })
        .collect()
}

/// Notion-style column projection over a query result: each column is
/// a function from a [`DocHeadline`] to a string.
pub struct DatabaseView<'a> {
    /// Column header names.
    pub columns: Vec<&'static str>,
    /// Per-column extractor.
    pub extractors: Vec<fn(&DocHeadline) -> String>,
    /// Materialised matches.
    pub rows: Vec<Match<'a>>,
}

impl<'a> DatabaseView<'a> {
    /// Build a view over `rows` with the four default columns
    /// (id, level, title, todo).
    #[must_use]
    pub fn default_view(rows: Vec<Match<'a>>) -> Self {
        Self {
            columns: vec!["id", "level", "title", "todo"],
            extractors: vec![
                |h| h.id().to_string(),
                |h| h.level().to_string(),
                |h| h.title().to_owned(),
                |h| h.todo().unwrap_or("").to_owned(),
            ],
            rows,
        }
    }

    /// Render each row as `Vec<String>` using the configured
    /// extractors.
    #[must_use]
    pub fn cells(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|m| self.extractors.iter().map(|f| f(m.headline)).collect())
            .collect()
    }
}

/// Score a fuzzy match of `needle` against `hay`, higher is better.
///
/// `None` when `needle` is not a case-insensitive subsequence of
/// `hay`. Contiguous matches outrank scattered ones (10 points per
/// gap), earlier starts outrank later ones (1 point per column).
#[must_use]
pub fn fuzzy_score(needle: &str, hay: &str) -> Option<u32> {
    const BASE: u32 = 1_000_000;
    if needle.is_empty() {
        return Some(BASE);
    }
    let hay_lower: Vec<char> = hay.chars().flat_map(char::to_lowercase).collect();
    let mut positions: Vec<usize> = Vec::new();
    let mut from = 0usize;
    for nc in needle.chars().flat_map(char::to_lowercase) {
        let rel = hay_lower[from..].iter().position(|&hc| hc == nc)?;
        positions.push(from + rel);
        from += rel + 1;
    }
    let first = *positions.first()?;
    let last = *positions.last()?;
    let gaps = last - first + 1 - positions.len();
    let penalty = u32::try_from(gaps).unwrap_or(u32::MAX).saturating_mul(10);
    let start = u32::try_from(first).unwrap_or(u32::MAX);
    Some(BASE.saturating_sub(penalty).saturating_sub(start))
}

/// Score `needle` against `hay` the way Doom's `orderless` completion
/// style matches: whitespace splits the query into components, and
/// every component must match somewhere in the candidate, in any
/// order.
///
/// [`fuzzy_score`] is a single subsequence, so a space in the query has
/// to be a space in the candidate — which is why typing `add sibling`
/// found nothing and you had to know the command was spelled
/// `add-sibling`. A filter that makes you guess the punctuation is a
/// filter you have to already know the answer to use.
///
/// The score is the mean of the components' own scores, so it stays in
/// [`fuzzy_score`]'s range and a tighter match still outranks a
/// scattered one. A query that is empty or all whitespace matches
/// everything, as it does there.
#[must_use]
pub fn orderless_score(needle: &str, hay: &str) -> Option<u32> {
    const BASE: u32 = 1_000_000;
    let mut total: u64 = 0;
    let mut parts: u64 = 0;
    for component in needle.split_whitespace() {
        total += u64::from(fuzzy_score(component, hay)?);
        parts += 1;
    }
    if parts == 0 {
        return Some(BASE);
    }
    u32::try_from(total / parts).ok()
}

/// The byte ranges of `hay` that `needle` matched, ascending and
/// non-overlapping. Empty when it does not match at all.
///
/// "Can we implement in all of the filterable/searchable input fields
/// with list items these kind of highlighting" — vertico paints the
/// characters your query matched, which is what tells you why a row is
/// in a list of near-identical ones. The scorers already walk those
/// characters to decide whether the row survives; this is the same walk
/// with the positions kept.
///
/// Byte ranges rather than character indices because that is what a
/// shell slices the label with, and always on char boundaries: an
/// accent is two bytes and a slice through the middle of one panics a
/// repaint. Agrees with [`orderless_score`] about what a match is —
/// a surviving row with nothing highlighted would read as a bug.
#[must_use]
pub fn match_spans(needle: &str, hay: &str) -> Vec<(usize, usize)> {
    // Char index -> byte offset, so a component's positions can be
    // turned back into slices of the original.
    let offsets: Vec<usize> = hay.char_indices().map(|(i, _)| i).collect();
    let lower: Vec<char> = hay.chars().flat_map(char::to_lowercase).collect();
    let mut hits: Vec<usize> = Vec::new();
    for component in needle.split_whitespace() {
        let mut from = 0usize;
        let mut found: Vec<usize> = Vec::new();
        for nc in component.chars().flat_map(char::to_lowercase) {
            let Some(rel) = lower[from..].iter().position(|&hc| hc == nc) else {
                // One component missing is the whole query missing,
                // exactly as `orderless_score` decides it.
                return Vec::new();
            };
            found.push(from + rel);
            from += rel + 1;
        }
        hits.extend(found);
    }
    hits.sort_unstable();
    hits.dedup();
    // Adjacent characters become one run: three one-character spans
    // paint the same pixels and cost three elements to do it.
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for at in hits {
        let Some(&start) = offsets.get(at) else {
            continue;
        };
        let end = offsets.get(at + 1).copied().unwrap_or(hay.len());
        match spans.last_mut() {
            Some(last) if last.1 == start => last.1 = end,
            _ => spans.push((start, end)),
        }
    }
    spans
}

/// Filter `items` by fuzzy-matching `needle`, best score first.
/// Ties keep the input order (stable sort).
#[must_use]
pub fn fuzzy_filter<'a>(needle: &str, items: &[&'a str]) -> Vec<(&'a str, u32)> {
    let mut out: Vec<(&'a str, u32)> = items
        .iter()
        .filter_map(|s| fuzzy_score(needle, s).map(|sc| (*s, sc)))
        .collect();
    out.sort_by_key(|&(_, sc)| std::cmp::Reverse(sc));
    out
}

/// View definition failure.
#[derive(Debug, thiserror::Error)]
pub enum ViewError {
    /// A `:key` other than `:from` / `:columns` / `:sort` appeared.
    #[error("unknown directive: {0}")]
    UnknownDirective(String),
    /// The `:from` value is not `all`, `tag:X`, `todo:X`, or `file:X`.
    #[error("bad source: {0}")]
    BadSource(String),
    /// A `:filter` clause has no recognised operator (`= != ~ > <`).
    #[error("bad filter: {0}")]
    BadFilter(String),
}

/// Row source of a view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Every headline in the vault.
    All,
    /// Headlines carrying a tag.
    Tag(String),
    /// Headlines with a TODO keyword.
    Todo(String),
    /// Headlines of one file (path suffix match).
    File(String),
}

/// One view column: a built-in field or a property key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Column {
    /// Headline title.
    Title,
    /// TODO keyword (empty when none).
    Todo,
    /// Priority letter (empty when none).
    Priority,
    /// Outline level.
    Level,
    /// Stable block id.
    Id,
    /// `:KEY:` property value (empty when absent).
    Property(String),
    /// A property whose value is another headline's id: the column
    /// shows that headline's *title*.
    ///
    /// A ULID is the one string in a vault a reader can do nothing
    /// with. And because this is a link rather than a copy, it keeps
    /// naming the right thing when the target is renamed — which is the
    /// whole reason to have one instead of writing the name into a
    /// property by hand.
    Relation(String),
    /// Aggregate a property over every headline whose relation points
    /// at this row.
    ///
    /// A relation reads forwards — a task naming its project. This
    /// reads the same edge backwards and does arithmetic on it, which
    /// is the half of a database people stare at: a project's row
    /// showing what its tasks add up to.
    Rollup {
        /// The relation property to follow back (`PROJECT`).
        via: String,
        /// The property to read on the rows that come back (`EFFORT`).
        of: String,
        /// What to do with them.
        agg: Agg,
    },
}

/// What a [`Column::Rollup`] does with the values it gathers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agg {
    /// How many rows point here. Needs no property.
    Count,
    /// Total, skipping anything that is not a number.
    Sum,
    /// Smallest; blank when there are none, because the smallest of no
    /// numbers is not zero.
    Min,
    /// Largest; blank when there are none.
    Max,
}

impl Agg {
    /// Parse the word after the last `:`. Anything unrecognised is
    /// [`Self::Count`] — a name nobody implemented should not make a
    /// document unreadable, the same rule an unknown column follows.
    fn parse(s: &str) -> Self {
        match s.trim() {
            "sum" => Self::Sum,
            "min" => Self::Min,
            "max" => Self::Max,
            _ => Self::Count,
        }
    }

    /// The name it goes by in a header.
    const fn name(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Min => "min",
            Self::Max => "max",
        }
    }
}

impl Column {
    fn parse(s: &str) -> Self {
        if let Some(key) = s.strip_prefix("rel:") {
            return Self::Relation(key.to_owned());
        }
        if let Some(rest) = s.strip_prefix("rollup:")
            && let Some((path, agg)) = rest.rsplit_once(':')
            && let Some((via, of)) = path.split_once('.')
        {
            return Self::Rollup {
                via: via.to_owned(),
                of: of.to_owned(),
                agg: Agg::parse(agg),
            };
        }
        match s {
            "title" => Self::Title,
            "todo" => Self::Todo,
            "priority" => Self::Priority,
            "level" => Self::Level,
            "id" => Self::Id,
            other => Self::Property(other.to_owned()),
        }
    }

    fn name(&self) -> String {
        match self {
            Self::Title => "title".to_owned(),
            Self::Todo => "todo".to_owned(),
            Self::Priority => "priority".to_owned(),
            Self::Level => "level".to_owned(),
            Self::Id => "id".to_owned(),
            Self::Property(k) | Self::Relation(k) => k.clone(),
            Self::Rollup { of, agg, .. } => format!("{}({of})", agg.name()),
        }
    }

    /// Cell value of this column for `h` (empty string when absent).
    #[must_use]
    pub fn extract(&self, h: &DocHeadline) -> String {
        match self {
            Self::Title => h.title().to_owned(),
            Self::Todo => h.todo().unwrap_or("").to_owned(),
            Self::Priority => h.priority().map(String::from).unwrap_or_default(),
            Self::Level => h.level().to_string(),
            Self::Id => h.id().to_string(),
            Self::Property(k) => h.property(k).unwrap_or("").to_owned(),
            // Without a vault there is nothing to resolve against, so
            // the id is all there is to give. Every path that renders a
            // view goes through `extract_in`, which has one.
            Self::Relation(k) => h.property(k).unwrap_or("").to_owned(),
            // A rollup is about rows this headline knows nothing
            // about, so without a vault there is no answer to give.
            Self::Rollup { .. } => String::new(),
        }
    }

    /// [`Self::extract`], with the vault a [`Self::Relation`] needs to
    /// resolve what it points at.
    ///
    /// A relation that points nowhere renders as `?<id>` rather than as
    /// blank: a dangling id is a mistake in the vault, and a reader has
    /// to be able to tell it from "no project at all".
    #[must_use]
    pub fn extract_in(&self, h: &DocHeadline, vault: &Vault) -> String {
        if let Self::Rollup { via, of, agg } = self {
            let here = h.id().to_string();
            let values: Vec<&str> = all_headlines(vault)
                .into_iter()
                .filter(|m| m.headline.property(via) == Some(here.as_str()))
                .map(|m| m.headline.property(of).unwrap_or(""))
                .collect();
            if *agg == Agg::Count {
                return values.len().to_string();
            }
            // Anything that is not a number is skipped rather than
            // read as zero: a total poisoned by one typo is worse than
            // a total that is quietly short.
            let nums: Vec<f64> = values
                .iter()
                .filter_map(|v| v.trim().parse().ok())
                .collect();
            return match agg {
                Agg::Sum => format_number(nums.iter().sum()),
                Agg::Min => nums
                    .iter()
                    .copied()
                    .reduce(f64::min)
                    .map(format_number)
                    .unwrap_or_default(),
                Agg::Max => nums
                    .iter()
                    .copied()
                    .reduce(f64::max)
                    .map(format_number)
                    .unwrap_or_default(),
                Agg::Count => unreachable!("handled above"),
            };
        }
        let Self::Relation(key) = self else {
            return self.extract(h);
        };
        let Some(id) = h.property(key).filter(|v| !v.is_empty()) else {
            return String::new();
        };
        let bid = closure_core::BlockId::from_existing(id);
        vault
            .find_by_id(&bid)
            .map_or_else(|| format!("?{id}"), |(target, _)| target.title().to_owned())
    }

    /// Typed sort key for `h`: numeric for [`Self::Level`], lexical for
    /// every other column. Lets a view sort 2 < 10 instead of "10" <
    /// "2".
    #[must_use]
    pub fn sort_val(&self, h: &DocHeadline) -> SortVal {
        match self {
            Self::Level => SortVal::Num(i64::from(h.level())),
            _ => SortVal::Text(self.extract(h)),
        }
    }
}

/// A typed, comparable cell value for sorting. Within a single column
/// every row produces the same variant, so cross-variant ordering is
/// never observed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortVal {
    /// Numeric key (e.g. outline level).
    Num(i64),
    /// Lexical key.
    Text(String),
}

/// Comparison operator for a view [`Filter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    /// `=` exact equality.
    Eq,
    /// `!=` inequality.
    Ne,
    /// `~` case-insensitive substring.
    Contains,
    /// `>` numeric greater-than (on a typed column).
    Gt,
    /// `<` numeric less-than (on a typed column).
    Lt,
}

/// One row filter: `column <op> value`. Several are AND-combined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    /// Column tested.
    pub column: Column,
    /// Comparison operator.
    pub op: FilterOp,
    /// Right-hand value.
    pub value: String,
}

impl Filter {
    /// Parse `col<op>value` (longest operators first). The column name
    /// is everything before the operator; the value everything after.
    fn parse(token: &str) -> Result<Self, ViewError> {
        // Order matters: `!=` before `=`.
        for (sym, op) in [
            ("!=", FilterOp::Ne),
            ("~", FilterOp::Contains),
            (">", FilterOp::Gt),
            ("<", FilterOp::Lt),
            ("=", FilterOp::Eq),
        ] {
            if let Some((col, val)) = token.split_once(sym) {
                return Ok(Self {
                    column: Column::parse(col),
                    op,
                    value: val.to_owned(),
                });
            }
        }
        Err(ViewError::BadFilter(token.to_owned()))
    }

    /// Whether headline `h` passes this filter.
    fn matches(&self, h: &DocHeadline) -> bool {
        let cell = self.column.extract(h);
        match self.op {
            FilterOp::Eq => cell == self.value,
            FilterOp::Ne => cell != self.value,
            FilterOp::Contains => cell.to_lowercase().contains(&self.value.to_lowercase()),
            FilterOp::Gt | FilterOp::Lt => {
                match (cell.parse::<i64>(), self.value.parse::<i64>()) {
                    (Ok(c), Ok(want)) => {
                        if self.op == FilterOp::Gt {
                            c > want
                        } else {
                            c < want
                        }
                    }
                    // Non-numeric operands never satisfy a numeric compare.
                    _ => false,
                }
            }
        }
    }
}

/// One sort key: a column and a direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    /// Column to order by.
    pub column: Column,
    /// Descending when true (`:sort -col`), ascending otherwise.
    pub descending: bool,
}

impl SortKey {
    /// Parse a single sort token: a leading `-` means descending.
    fn parse(token: &str) -> Self {
        token.strip_prefix('-').map_or_else(
            || Self {
                column: Column::parse(token),
                descending: false,
            },
            |rest| Self {
                column: Column::parse(rest),
                descending: true,
            },
        )
    }
}

/// An org-defined database view: parsed from the params of a
/// `#+BEGIN: closure-view` dynamic block, e.g.
/// `:from tag:work :columns title,todo,EFFORT :sort title`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewSpec {
    /// Where rows come from.
    pub from: Source,
    /// Columns, left to right.
    pub columns: Vec<Column>,
    /// Sort keys, applied in order (typed + directional). Empty = none.
    pub sort: Vec<SortKey>,
    /// Row filters, AND-combined. Empty = no filtering.
    pub filter: Vec<Filter>,
    /// Optional view name (`:name`), for picking among saved views.
    pub name: Option<String>,
    /// Column whose value the rows are grouped by (`:group`).
    ///
    /// A database that cannot group is a saved search: grouping is what
    /// turns "every task with this tag" into a board.
    pub group: Option<Column>,
}

impl ViewSpec {
    /// Parse a params string. Missing `:from` means `all`; missing
    /// `:columns` means `title,todo`.
    ///
    /// # Errors
    ///
    /// [`ViewError::UnknownDirective`] for unrecognised `:key`s,
    /// [`ViewError::BadSource`] for malformed `:from` values.
    pub fn parse(params: &str) -> Result<Self, ViewError> {
        let mut from = Source::All;
        let mut columns = vec![Column::Title, Column::Todo];
        let mut sort: Vec<SortKey> = Vec::new();
        let mut filter: Vec<Filter> = Vec::new();
        let mut name: Option<String> = None;
        let mut group: Option<Column> = None;
        let mut tokens = params.split_whitespace();
        while let Some(tok) = tokens.next() {
            let value = tokens.next().unwrap_or("");
            match tok {
                ":from" => {
                    from = if value == "all" {
                        Source::All
                    } else if let Some(t) = value.strip_prefix("tag:") {
                        Source::Tag(t.to_owned())
                    } else if let Some(t) = value.strip_prefix("todo:") {
                        Source::Todo(t.to_owned())
                    } else if let Some(t) = value.strip_prefix("file:") {
                        Source::File(t.to_owned())
                    } else {
                        return Err(ViewError::BadSource(value.to_owned()));
                    };
                }
                ":columns" => {
                    columns = value
                        .split(',')
                        .filter(|c| !c.is_empty())
                        .map(Column::parse)
                        .collect();
                }
                ":sort" => {
                    sort = value
                        .split(',')
                        .filter(|t| !t.is_empty())
                        .map(SortKey::parse)
                        .collect();
                }
                ":filter" => filter.push(Filter::parse(value)?),
                ":name" => name = Some(value.to_owned()),
                ":group" => group = Some(Column::parse(value)),
                other => return Err(ViewError::UnknownDirective(other.to_owned())),
            }
        }
        Ok(Self {
            from,
            columns,
            sort,
            filter,
            name,
            group,
        })
    }

    /// Column header names, left to right.
    #[must_use]
    pub fn header(&self) -> Vec<String> {
        self.columns.iter().map(Column::name).collect()
    }

    /// Rows matching [`Self::from`] (and `:filter`), in file order.
    #[must_use]
    pub fn rows<'a>(&self, vault: &'a Vault) -> Vec<Match<'a>> {
        let base = match &self.from {
            Source::All => all_headlines(vault),
            Source::Tag(t) => by_tag(vault, t),
            Source::Todo(k) => by_todo(vault, k),
            Source::File(f) => all_headlines(vault)
                .into_iter()
                .filter(|m| m.path.ends_with(f))
                .collect(),
        };
        if self.filter.is_empty() {
            return base;
        }
        base.into_iter()
            .filter(|m| self.filter.iter().all(|f| f.matches(m.headline)))
            .collect()
    }

    /// Materialised cells: one `Vec<String>` per row, ordered by the
    /// `:sort` keys (typed + directional, applied in order as a stable
    /// lexicographic ordering) when present.
    #[must_use]
    pub fn cells(&self, vault: &Vault) -> Vec<Vec<String>> {
        let mut rows = self.rows(vault);
        if !self.sort.is_empty() {
            rows.sort_by(|a, b| {
                for key in &self.sort {
                    let ord = key
                        .column
                        .sort_val(a.headline)
                        .cmp(&key.column.sort_val(b.headline));
                    let ord = if key.descending { ord.reverse() } else { ord };
                    if ord != std::cmp::Ordering::Equal {
                        return ord;
                    }
                }
                std::cmp::Ordering::Equal
            });
        }
        rows.iter()
            .map(|m| {
                self.columns
                    .iter()
                    .map(|c| c.extract_in(m.headline, vault))
                    .collect()
            })
            .collect()
    }

    /// [`Self::cells`], grouped by `:group`.
    ///
    /// Always at least one group, so a renderer has one shape to draw
    /// rather than two: without `:group` everything is in a single
    /// group with no name.
    #[must_use]
    pub fn groups(&self, vault: &Vault) -> Vec<(String, Vec<Vec<String>>)> {
        let cells = self.cells(vault);
        let Some(by) = &self.group else {
            return vec![(String::new(), cells)];
        };
        // The grouping column need not be one of the shown columns, so
        // it is extracted here rather than looked up by position.
        let at = self.columns.iter().position(|c| c == by);
        match at {
            Some(i) => group_cells(cells, i),
            None => {
                let mut keyed: Vec<Vec<String>> = Vec::new();
                for (row, m) in cells.into_iter().zip(self.rows(vault)) {
                    let mut r = row;
                    r.push(by.extract_in(m.headline, vault));
                    keyed.push(r);
                }
                let last = keyed.first().map_or(0, |r| r.len().saturating_sub(1));
                group_cells(keyed, last)
                    .into_iter()
                    .map(|(k, rows)| {
                        (
                            k,
                            rows.into_iter()
                                .map(|mut r| {
                                    r.pop();
                                    r
                                })
                                .collect(),
                        )
                    })
                    .collect()
            }
        }
    }
}

/// A number without a trailing `.0`, so a table of whole efforts reads
/// like whole numbers.
fn format_number(n: f64) -> String {
    // Negative zero is a real f64 and `{:.0}` prints it as `-0`, which
    // is not a total anybody wants to read.
    if n == 0.0 {
        return "0".to_owned();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        n.to_string()
    }
}

/// Group `cells` by the value in column `at`, in order of value.
///
/// A row whose group value is empty keeps its own group rather than
/// being dropped: hiding it would make the table disagree with the
/// query that produced it.
fn group_cells(cells: Vec<Vec<String>>, at: usize) -> Vec<(String, Vec<Vec<String>>)> {
    let mut out: Vec<(String, Vec<Vec<String>>)> = Vec::new();
    let mut keys: Vec<String> = cells
        .iter()
        .map(|r| r.get(at).cloned().unwrap_or_default())
        .collect();
    keys.sort();
    keys.dedup();
    for key in keys {
        let rows: Vec<Vec<String>> = cells
            .iter()
            .filter(|r| r.get(at).map(String::as_str).unwrap_or("") == key)
            .cloned()
            .collect();
        out.push((key, rows));
    }
    out
}

/// Render header + grouped rows as one org table with a rule between
/// groups.
///
/// One table rather than several: it is one query with one set of
/// columns, and a reader scanning down a column should not have to
/// re-find it after every heading. `|---|` is an ordinary org
/// separator, so what comes out is still a table Emacs will align.
#[must_use]
pub fn render_grouped_table(header: &[String], groups: &[(String, Vec<Vec<String>>)]) -> String {
    let all: Vec<Vec<String>> = groups
        .iter()
        .flat_map(|(_, rows)| rows.iter().cloned())
        .collect();
    let full = render_table(header, &all);
    if groups.len() < 2 {
        return full;
    }
    // Re-insert a rule where each group after the first begins. The
    // widths are already right because they were computed over every
    // row at once.
    let lines: Vec<&str> = full.lines().collect();
    let rule = lines.get(1).copied().unwrap_or_default().to_owned();
    let mut out = String::new();
    let mut line = 0usize;
    for (i, (_, rows)) in groups.iter().enumerate() {
        if i > 0 {
            out.push_str(&rule);
            out.push('\n');
        }
        for _ in 0..rows.len() {
            // Header and its rule come first, so data starts at 2.
            if let Some(l) = lines.get(2 + line) {
                out.push_str(l);
                out.push('\n');
            }
            line += 1;
        }
    }
    let head = lines
        .iter()
        .take(2)
        .map(|l| format!("{l}\n"))
        .collect::<String>();
    format!("{head}{out}")
}

/// Enumerate the vault's saved database views.
///
/// Every `#+BEGIN: closure-view <params>` dynamic block becomes a
/// `(name, ViewSpec)`, in document + line order. The name comes from the
/// block's `:name` param, falling back to `view-N` (its 0-based
/// position). A malformed block fails the whole batch.
///
/// # Errors
///
/// Propagates the first [`ViewError`] from any block's params.
pub fn views(vault: &Vault) -> Result<Vec<(String, ViewSpec)>, ViewError> {
    let mut out: Vec<(String, ViewSpec)> = Vec::new();
    for (_path, doc) in vault.iter() {
        for line in doc.source().lines() {
            let trimmed = line.trim_start();
            // Case-insensitive `#+BEGIN: closure-view` prefix.
            let lower = trimmed.to_ascii_lowercase();
            let Some(rest) = lower.strip_prefix("#+begin: closure-view") else {
                continue;
            };
            // Params are the original-case remainder after the keyword.
            let params = trimmed[trimmed.len() - rest.len()..].trim();
            let spec = ViewSpec::parse(params)?;
            let name = spec
                .name
                .clone()
                .unwrap_or_else(|| format!("view-{}", out.len()));
            out.push((name, spec));
        }
    }
    Ok(out)
}

/// An error expanding `#+BEGIN: closure-widget` composite blocks (V2a).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WidgetError {
    /// A widget references itself, directly or through others.
    ///
    /// Carries the whole ring rather than the name the expander
    /// happened to be standing on: in a vault where `page` uses
    /// `panel` uses `header` uses `page`, one name leaves the reader
    /// to find the other two.
    #[error("widget cycle: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
    /// Composition nested deeper than [`DEPTH_LIMIT`] without
    /// repeating a name.
    ///
    /// Not a cycle, and not a panic either: recursing it would end the
    /// process, and I5 forbids that more strongly than it forbids a
    /// panic — a blown stack takes the window with it.
    #[error("widget nesting deeper than {limit}: {}", path.join(" -> "))]
    TooDeep {
        /// How deep composition may nest.
        limit: usize,
        /// The chain as far as it got.
        path: Vec<String>,
    },
    /// A `{{ref}}` names a widget that is not defined.
    #[error("unknown widget `{0}`")]
    Unknown(String),
    /// A call site named an input the widget does not declare — the
    /// typo that used to render as silence.
    #[error("widget `{widget}` has no input `{argument}`")]
    UnknownArgument {
        /// The widget called.
        widget: String,
        /// The argument name that matched none of its inputs.
        argument: String,
    },
    /// A value that the input's declared type cannot hold.
    #[error("widget `{widget}` input `{input}` expects {expected}, got `{got}`")]
    BadArgument {
        /// The widget called.
        widget: String,
        /// The input whose type was not satisfied.
        input: String,
        /// What that input is declared to hold.
        expected: String,
        /// The value as written at the call site.
        got: String,
    },
}

/// What an input is declared to hold.
///
/// Deliberately three. A type system here earns its place by catching
/// the mistakes that are always mistakes — a word where a number
/// belongs — and stops earning it the moment a template needs a
/// grammar to describe its own arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputType {
    /// Anything, and what an undeclared type means.
    #[default]
    Text,
    /// Parses as a number.
    Number,
    /// Exactly `true` or `false`.
    Bool,
}

impl InputType {
    /// The declared spelling, for the error message.
    const fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Number => "number",
            Self::Bool => "bool",
        }
    }

    /// Parse the part after `:` in `count:number`. An unknown word is
    /// text rather than an error: a type nobody implemented should not
    /// make a document unreadable.
    fn of(word: &str) -> Self {
        match word.trim() {
            "number" => Self::Number,
            "bool" => Self::Bool,
            _ => Self::Text,
        }
    }

    /// Whether `value` is one of these.
    fn accepts(self, value: &str) -> bool {
        match self {
            Self::Text => true,
            Self::Number => value.trim().parse::<f64>().is_ok(),
            Self::Bool => matches!(value.trim(), "true" | "false"),
        }
    }
}

/// A widget: the template between its delimiters, and the names it
/// declares it takes.
///
/// Declaring is what makes a name a parameter. A widget block is its
/// own call site with no arguments, so `{{who}}` in a widget that
/// declares nothing is indistinguishable from a reference to a widget
/// called `who` that nobody defined — and that has to stay an error.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WidgetDef {
    /// The template, verbatim, as it sits between BEGIN and END.
    pub body: String,
    /// The parameters from `:inputs a,b:number`, in declared order.
    pub inputs: Vec<(String, InputType)>,
}

/// What a `#+BEGIN: closure-widget` line says.
///
/// `:name` defines, `:call` invokes with the block's own content as
/// the slot — one keyword apart, and both are ordinary org dynamic
/// blocks, so a file full of them still opens in Emacs.
enum WidgetBlock {
    /// `:name panel :inputs title` — a definition.
    Define {
        /// The widget's name.
        name: String,
        /// What it declares it takes.
        inputs: Vec<(String, InputType)>,
    },
    /// `:call panel :with title=Notes` — an invocation whose body is
    /// the content to wrap.
    Call {
        /// The widget being called.
        name: String,
        /// The arguments from `:with`.
        args: Vec<(String, String)>,
    },
}

/// How deep composition may nest before it is called a runaway.
///
/// Well past what a page needs — a layout three or four widgets deep
/// is already unusual — and far below what recursing this expander can
/// survive.
pub const DEPTH_LIMIT: usize = 64;

/// The name given to a call block's content inside the widget it calls.
///
/// Always in scope, empty when there is none — otherwise `{{slot}}` in
/// a widget nobody passed content to would fall through to a widget
/// lookup and fail as an unknown name.
const SLOT: &str = "slot";

/// Read a `#+BEGIN: closure-widget` line, if this is one.
fn widget_block_of(line: &str) -> Option<WidgetBlock> {
    let trimmed = line.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    let rest = lower.strip_prefix("#+begin: closure-widget")?;
    let params = trimmed[trimmed.len() - rest.len()..].trim();
    if let Some((_, after)) = params.split_once(":call") {
        let after = after.trim_start();
        let name = after.split_whitespace().next()?.to_owned();
        let args = params
            .split_once(":with")
            .map(|(_, w)| parse_reference(&format!("_ {}", w.trim())).1)
            .unwrap_or_default();
        return Some(WidgetBlock::Call { name, args });
    }
    let (name, inputs) = widget_begin_parts(line)?;
    Some(WidgetBlock::Define { name, inputs })
}

/// The `:name` and `:inputs` of a `#+BEGIN: closure-widget` line, if
/// this is one.
fn widget_begin_parts(line: &str) -> Option<(String, Vec<(String, InputType)>)> {
    let trimmed = line.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    let rest = lower.strip_prefix("#+begin: closure-widget")?;
    // Params are the original-case remainder after the keyword.
    let params = trimmed[trimmed.len() - rest.len()..].trim();
    let after = params.split_once(":name")?.1.trim_start();
    let name = after.split_whitespace().next()?;
    let inputs = params
        .split_once(":inputs")
        .and_then(|(_, a)| a.trim_start().split_whitespace().next())
        .map(|list| {
            list.split(',')
                .map(str::trim)
                .filter(|i| !i.is_empty())
                .map(|i| {
                    i.split_once(':').map_or_else(
                        || (i.to_owned(), InputType::Text),
                        |(n, t)| (n.trim().to_owned(), InputType::of(t)),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    Some((name.to_owned(), inputs))
}

/// The `:name` of a `#+BEGIN: closure-widget` line, if this is one.
fn widget_begin_name(line: &str) -> Option<String> {
    widget_begin_parts(line).map(|(n, _)| n)
}

/// True for the dynamic-block terminator `#+END:`.
fn is_widget_end(line: &str) -> bool {
    line.trim_start().to_ascii_lowercase().starts_with("#+end:")
}

/// `name -> definition` for every widget defined in `src`.
fn collect_widget_defs(src: &str) -> std::collections::HashMap<String, WidgetDef> {
    let mut defs = std::collections::HashMap::new();
    let mut lines = src.split_inclusive('\n');
    while let Some(line) = lines.next() {
        if let Some(block) = widget_block_of(line) {
            let mut body = String::new();
            for l in lines.by_ref() {
                if is_widget_end(l) {
                    break;
                }
                body.push_str(l);
            }
            // A call block's body is content, not a template: it
            // defines nothing.
            if let WidgetBlock::Define { name, inputs } = block {
                defs.insert(name, WidgetDef { body, inputs });
            }
        }
    }
    defs
}

/// The text of one `{{…}}` reference and whatever follows it.
///
/// Depth-aware, because an argument's value can itself be a reference:
/// `{{inner x={{x}}}}` is one reference with one argument, and a scan
/// that stopped at the first `}}` would read it as a broken one.
fn split_reference(after_open: &str) -> Option<(&str, &str)> {
    let b = after_open.as_bytes();
    let mut depth = 1usize;
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'{' && b[i + 1] == b'{' {
            depth += 1;
            i += 2;
        } else if b[i] == b'}' && b[i + 1] == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some((&after_open[..i], &after_open[i + 2..]));
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

/// Split a reference's text into its name and its arguments.
///
/// `greet who=world`, `greet who="the whole world"`, `greet` — and an
/// unquoted value may hold a nested reference, so the scan for the end
/// of one counts braces rather than stopping at the first space.
fn parse_reference(inner: &str) -> (String, Vec<(String, String)>) {
    let inner = inner.trim();
    let (name, mut rest) = inner
        .split_once(char::is_whitespace)
        .map_or((inner, ""), |(n, r)| (n, r));
    let mut args = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let Some((key, after_eq)) = rest.split_once('=') else {
            break;
        };
        // A key with a space in it is not a key; the reference is
        // malformed and the rest is left alone rather than guessed at.
        if key.trim().is_empty() || key.contains(char::is_whitespace) {
            break;
        }
        let (value, tail) = if let Some(quoted) = after_eq.strip_prefix('"') {
            match quoted.split_once('"') {
                Some((v, t)) => (v, t),
                None => (quoted, ""),
            }
        } else {
            let b = after_eq.as_bytes();
            let mut depth = 0usize;
            let mut i = 0;
            while i < b.len() {
                if i + 1 < b.len() && b[i] == b'{' && b[i + 1] == b'{' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'}' && b[i + 1] == b'}' {
                    depth = depth.saturating_sub(1);
                    i += 2;
                } else if depth == 0 && b[i].is_ascii_whitespace() {
                    break;
                } else {
                    i += 1;
                }
            }
            (&after_eq[..i], &after_eq[i..])
        };
        args.push((key.trim().to_owned(), value.to_owned()));
        rest = tail;
    }
    (name.to_owned(), args)
}

/// Expand `text` in a scope: `args` are the arguments the enclosing
/// widget was called with, `defs` every widget in reach.
///
/// An argument shadows a widget of the same name. Locals beat globals —
/// the rule every language with both already uses, and the one that
/// lets a widget be written without knowing every name in the vault.
/// A reference that carries arguments of its own is always a widget
/// call: `{{who}}` may be a parameter, `{{who x=1}}` cannot be.
fn expand_text(
    text: &str,
    args: &[(String, String)],
    defs: &std::collections::HashMap<String, WidgetDef>,
    stack: &mut Vec<String>,
) -> Result<String, WidgetError> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some((inner, tail)) = split_reference(after) else {
            out.push_str("{{");
            rest = after;
            continue;
        };
        let (name, call_args) = parse_reference(inner);
        if call_args.is_empty()
            && let Some((_, value)) = args.iter().find(|(k, _)| *k == name)
        {
            out.push_str(value);
            rest = tail;
            continue;
        }
        // The values are the caller's to work out, so they are expanded
        // here, in this scope, before the callee ever sees them.
        let mut bound = Vec::with_capacity(call_args.len());
        for (k, v) in call_args {
            bound.push((k, expand_text(&v, args, defs, stack)?));
        }
        let value = expand_widget_name(&name, &bound, defs, stack)?;
        out.push_str(value.trim_end_matches('\n'));
        rest = tail;
    }
    out.push_str(rest);
    Ok(out)
}

/// Fully expand widget `name`, called with `args`; `stack` carries the
/// active expansion chain for cycle detection.
fn expand_widget_name(
    name: &str,
    args: &[(String, String)],
    defs: &std::collections::HashMap<String, WidgetDef>,
    stack: &mut Vec<String>,
) -> Result<String, WidgetError> {
    if stack.iter().any(|s| s == name) {
        // From where the ring closes, not from where expansion began:
        // the widgets before it are on the way in, not part of it.
        let from = stack.iter().position(|s| s == name).unwrap_or(0);
        let mut ring: Vec<String> = stack[from..].to_vec();
        ring.push(name.to_owned());
        return Err(WidgetError::Cycle(ring));
    }
    if stack.len() >= DEPTH_LIMIT {
        return Err(WidgetError::TooDeep {
            limit: DEPTH_LIMIT,
            path: stack.clone(),
        });
    }
    let def = defs
        .get(name)
        .ok_or_else(|| WidgetError::Unknown(name.to_owned()))?;
    // Every declared input is in scope whether or not the call site
    // bound it: a parameter nobody passed expands to nothing, because
    // the block that defines a widget is also a call site with no
    // arguments and showing a template is not an error.
    let mut scope: Vec<(String, String)> = def
        .inputs
        .iter()
        .map(|(n, _)| (n.clone(), String::new()))
        .collect();
    // A slot is optional, like an argument nobody read, and is in
    // scope either way so that `{{slot}}` is never mistaken for a
    // widget nobody defined.
    scope.push((SLOT.to_owned(), String::new()));
    for (k, v) in args {
        // Checked before anything is rendered, so a mistake arrives as
        // a message rather than as content (I9).
        if let Some((_, ty)) = def.inputs.iter().find(|(n, _)| n == k) {
            if !ty.accepts(v) {
                return Err(WidgetError::BadArgument {
                    widget: name.to_owned(),
                    input: k.clone(),
                    expected: ty.name().to_owned(),
                    got: v.clone(),
                });
            }
        } else if !def.inputs.is_empty() && k != SLOT {
            return Err(WidgetError::UnknownArgument {
                widget: name.to_owned(),
                argument: k.clone(),
            });
        }
        if let Some(slot) = scope.iter_mut().find(|(n, _)| n == k) {
            slot.1 = v.clone();
        } else {
            scope.push((k.clone(), v.clone()));
        }
    }
    stack.push(name.to_owned());
    let out = expand_text(&def.body, &scope, defs, stack);
    stack.pop();
    out
}

/// Expand every `#+BEGIN: closure-widget :name X` block in `src` in place
/// (V2a).
///
/// Each block's body is replaced by its fully-expanded content (`{{ref}}`
/// references resolved recursively, cycle-detected), while every byte
/// outside the block bodies — including the `BEGIN`/`END` lines
/// themselves — is preserved verbatim (I1).
///
/// # Errors
///
/// [`WidgetError::Cycle`] on a reference cycle, [`WidgetError::Unknown`]
/// for a `{{ref}}` with no matching definition.
pub fn expand_widgets(src: &str) -> Result<String, WidgetError> {
    expand_widgets_with(src, &std::collections::HashMap::<String, WidgetDef>::new())
}

/// The names of every widget defined in `src`, in document order (V2b).
#[must_use]
pub fn widget_def_names(src: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in src.split_inclusive('\n') {
        if let Some(n) = widget_begin_name(line) {
            names.push(n);
        }
    }
    names
}

/// Like [`expand_widgets`], but resolving against `extra` definitions too
/// (V2b).
///
/// `extra` holds widgets defined in other vault files; a definition local
/// to `src` shadows `extra`.
///
/// # Errors
///
/// [`WidgetError::Cycle`] / [`WidgetError::Unknown`] as [`expand_widgets`].
pub fn expand_widgets_with<S: std::hash::BuildHasher>(
    src: &str,
    extra: &std::collections::HashMap<String, WidgetDef, S>,
) -> Result<String, WidgetError> {
    let mut defs: std::collections::HashMap<String, WidgetDef> = std::collections::HashMap::new();
    for (k, v) in extra {
        defs.insert(k.clone(), v.clone());
    }
    // Local definitions win over external ones.
    for (k, v) in collect_widget_defs(src) {
        defs.insert(k, v);
    }
    let mut out = String::new();
    let mut lines = src.split_inclusive('\n');
    while let Some(line) = lines.next() {
        out.push_str(line);
        let Some(block) = widget_block_of(line) else {
            continue;
        };
        // Take the body, capturing the END terminator. A definition's
        // body is a template and is replaced by what it means; a call
        // block's body is content and becomes the callee's slot.
        let mut body = String::new();
        let mut end = None;
        for l in lines.by_ref() {
            if is_widget_end(l) {
                end = Some(l);
                break;
            }
            body.push_str(l);
        }
        let expanded = match block {
            WidgetBlock::Define { name, .. } => {
                expand_widget_name(&name, &[], &defs, &mut Vec::new())?
            }
            WidgetBlock::Call { name, mut args } => {
                // The content is the caller's, so it is expanded here,
                // in the caller's scope, before the callee sees it —
                // the same rule as an argument's value.
                let slot = expand_text(&body, &[], &defs, &mut Vec::new())?;
                args.push((SLOT.to_owned(), slot.trim_end_matches('\n').to_owned()));
                expand_widget_name(&name, &args, &defs, &mut Vec::new())?
            }
        };
        out.push_str(&expanded);
        if !expanded.ends_with('\n') {
            out.push('\n');
        }
        if let Some(e) = end {
            out.push_str(e);
        }
    }
    Ok(out)
}

/// Every widget definition across the vault, `name -> definition`
/// (V2b). A name defined in more than one file resolves to the
/// document-order-last definition (deterministic over `Vault::iter`).
#[must_use]
pub fn vault_widget_defs(vault: &Vault) -> std::collections::HashMap<String, WidgetDef> {
    let mut defs = std::collections::HashMap::new();
    for (_path, doc) in vault.iter() {
        for (name, def) in collect_widget_defs(&doc.source()) {
            defs.insert(name, def);
        }
    }
    defs
}

/// Every widget definition's `(name, file)` across the vault, sorted by
/// `(name, file)` for a deterministic listing (V2b; `closure widgets`).
#[must_use]
pub fn vault_widget_names(vault: &Vault) -> Vec<(String, std::path::PathBuf)> {
    let mut out: Vec<(String, std::path::PathBuf)> = Vec::new();
    for (path, doc) in vault.iter() {
        for name in widget_def_names(&doc.source()) {
            out.push((name, path.to_path_buf()));
        }
    }
    out.sort();
    out
}

/// Expand a single widget `name` using definitions from the whole vault
/// (V2c), returning its fully-resolved content (`{{ref}}`s expanded).
///
/// Lets a shell turn a widget name into rendered content for a
/// `Node::Widget`.
///
/// # Errors
///
/// [`WidgetError::Unknown`] if no widget is named `name`;
/// [`WidgetError::Cycle`] on a reference cycle.
pub fn expand_named_widget(vault: &Vault, name: &str) -> Result<String, WidgetError> {
    let defs = vault_widget_defs(vault);
    // Wrap the name as a one-line reference and expand it against the defs.
    expand_widgets_with(
        &format!("#+BEGIN: closure-widget :name __q__\n{{{{{name}}}}}\n#+END:\n"),
        &defs,
    )
    .map(|expanded| {
        // Strip the synthetic wrapper's BEGIN/END lines, keeping the body.
        let lines: Vec<&str> = expanded.lines().collect();
        let end = lines.len().saturating_sub(1);
        lines.get(1..end).unwrap_or_default().join("\n")
    })
}

/// Expand the widget blocks in the vault file at `relative`, resolving
/// `{{ref}}` against widget definitions from the whole vault (V2b).
///
/// # Errors
///
/// [`WidgetError::Cycle`] / [`WidgetError::Unknown`]; returns
/// [`WidgetError::Unknown`] with the path if the file is not in the vault.
pub fn expand_doc_widgets(
    vault: &Vault,
    relative: &std::path::Path,
) -> Result<String, WidgetError> {
    let doc = vault
        .document_relative(relative)
        .ok_or_else(|| WidgetError::Unknown(relative.display().to_string()))?;
    expand_widgets_with(&doc.source(), &vault_widget_defs(vault))
}

/// Render header + rows as an aligned org-mode table.
#[must_use]
pub fn render_table(header: &[String], rows: &[Vec<String>]) -> String {
    let cols = header.len();
    let mut widths: Vec<usize> = header.iter().map(String::len).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate().take(cols) {
            if cell.len() > widths[i] {
                widths[i] = cell.len();
            }
        }
    }
    let mut out = String::new();
    let push_row = |cells: &[String], out: &mut String| {
        out.push('|');
        for (i, w) in widths.iter().enumerate() {
            let cell = cells.get(i).map_or("", String::as_str);
            let _ = write!(out, " {cell:<w$} |");
        }
        out.push('\n');
    };
    push_row(header, &mut out);
    out.push('|');
    for (i, w) in widths.iter().enumerate() {
        out.push_str(&"-".repeat(w + 2));
        out.push(if i + 1 == widths.len() { '|' } else { '+' });
    }
    out.push('\n');
    for row in rows {
        push_row(row, &mut out);
    }
    out
}

/// A full-text search hit: file, 1-based line, and the matched line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// File containing the match.
    pub path: std::path::PathBuf,
    /// 1-based line number.
    pub line: usize,
    /// The matching line's text (trimmed of the trailing newline).
    pub text: String,
}

/// A pluggable full-text search engine over a vault directory.
pub trait SearchBackend {
    /// Engine identifier (matches the `search_backend` config value).
    fn name(&self) -> &str;
    /// Search `*.org` files under `root` for `needle`, returning hits.
    fn search(&self, root: &std::path::Path, needle: &str) -> Vec<Hit>;
}

/// Built-in case-insensitive substring search (the default; no
/// external dependency).
#[derive(Debug, Default, Clone, Copy)]
pub struct BuiltinSearch;

fn walk_text_files(root: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_text_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "org" || e == "md") {
            out.push(path);
        }
    }
}

impl SearchBackend for BuiltinSearch {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "builtin"
    }

    fn search(&self, root: &std::path::Path, needle: &str) -> Vec<Hit> {
        let lower = needle.to_lowercase();
        let mut files = Vec::new();
        walk_text_files(root, &mut files);
        files.sort();
        let mut hits = Vec::new();
        for path in files {
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&lower) {
                    hits.push(Hit {
                        path: path.clone(),
                        line: i + 1,
                        text: line.to_owned(),
                    });
                }
            }
        }
        hits
    }
}

/// Ripgrep-backed search (external `rg` binary). Falls back to no
/// hits if `rg` is unavailable.
#[derive(Debug, Default, Clone, Copy)]
pub struct RipgrepSearch;

impl SearchBackend for RipgrepSearch {
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "ripgrep"
    }

    fn search(&self, root: &std::path::Path, needle: &str) -> Vec<Hit> {
        let Ok(out) = std::process::Command::new("rg")
            .args([
                "--line-number",
                "--no-heading",
                "--color=never",
                "-g",
                "*.org",
                "-g",
                "*.md",
                "-i",
            ])
            .arg(needle)
            .arg(root)
            .output()
        else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut hits = Vec::new();
        for raw in text.lines() {
            // rg output: <path>:<line>:<text>
            let mut parts = raw.splitn(3, ':');
            if let (Some(p), Some(l), Some(t)) = (parts.next(), parts.next(), parts.next())
                && let Ok(line) = l.parse::<usize>()
            {
                hits.push(Hit {
                    path: std::path::PathBuf::from(p),
                    line,
                    text: t.to_owned(),
                });
            }
        }
        hits
    }
}

/// Select a search backend by name (`builtin`, `ripgrep`/`rg`).
/// Unknown names fall back to the built-in engine.
#[must_use]
pub fn backend_for(name: &str) -> Box<dyn SearchBackend> {
    match name {
        "ripgrep" | "rg" => Box::new(RipgrepSearch),
        _ => Box::new(BuiltinSearch),
    }
}

/// Markdown backlinks (Q4-M3).
///
/// Every `.md` file under `root` (one level, non-recursive like the
/// vault loader) whose link targets name `target` — matched on the
/// raw target or its extension-less slug (md identity is path/slug;
/// there is no `:ID:` in markdown, see the Q4 Decision). Read-only; a
/// missing or unreadable dir is empty, never a panic (I5).
#[must_use]
pub fn md_backlinks(root: &std::path::Path, target: &str) -> Vec<std::path::PathBuf> {
    let slug = target.strip_suffix(".md").unwrap_or(target);
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<std::path::PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .filter(|p| {
            std::fs::read_to_string(p).is_ok_and(|md| {
                closure_markdown::link_targets(&md).iter().any(|t| {
                    let t_slug = t.strip_suffix(".md").unwrap_or(t);
                    t == target || t_slug == slug
                })
            })
        })
        .collect();
    out.sort();
    out
}
