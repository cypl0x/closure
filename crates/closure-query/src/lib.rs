//! Tree / tag / text / backlink queries over a [`Vault`].
//!
//! All queries return borrowed matches over the vault's cached
//! `DocHeadline` records (stable block ids, I2) — no re-parsing.

#![forbid(unsafe_code)]

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
}

impl Column {
    fn parse(s: &str) -> Self {
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
            Self::Property(k) => k.clone(),
        }
    }

    fn extract(&self, h: &DocHeadline) -> String {
        match self {
            Self::Title => h.title().to_owned(),
            Self::Todo => h.todo().unwrap_or("").to_owned(),
            Self::Priority => h.priority().map(String::from).unwrap_or_default(),
            Self::Level => h.level().to_string(),
            Self::Id => h.id().to_string(),
            Self::Property(k) => h.property(k).unwrap_or("").to_owned(),
        }
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
    /// Optional sort column (string ascending).
    pub sort: Option<Column>,
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
        let mut sort = None;
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
                ":sort" => sort = Some(Column::parse(value)),
                other => return Err(ViewError::UnknownDirective(other.to_owned())),
            }
        }
        Ok(Self {
            from,
            columns,
            sort,
        })
    }

    /// Column header names, left to right.
    #[must_use]
    pub fn header(&self) -> Vec<String> {
        self.columns.iter().map(Column::name).collect()
    }

    /// Rows matching [`Self::from`], in file order.
    #[must_use]
    pub fn rows<'a>(&self, vault: &'a Vault) -> Vec<Match<'a>> {
        match &self.from {
            Source::All => all_headlines(vault),
            Source::Tag(t) => by_tag(vault, t),
            Source::Todo(k) => by_todo(vault, k),
            Source::File(f) => all_headlines(vault)
                .into_iter()
                .filter(|m| m.path.ends_with(f))
                .collect(),
        }
    }

    /// Materialised cells: one `Vec<String>` per row, sorted by the
    /// `:sort` column when present.
    #[must_use]
    pub fn cells(&self, vault: &Vault) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = self
            .rows(vault)
            .iter()
            .map(|m| self.columns.iter().map(|c| c.extract(m.headline)).collect())
            .collect();
        if let Some(sort) = &self.sort
            && let Some(idx) = self.columns.iter().position(|c| c == sort)
        {
            out.sort_by(|a, b| a[idx].cmp(&b[idx]));
        }
        out
    }
}
