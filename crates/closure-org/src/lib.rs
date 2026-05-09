//! Lossless Emacs org-mode parser and printer.
//!
//! Invariant I1 (byte-exact roundtrip on the golden corpus) is the core
//! guarantee of this crate: for every `s` under `fixtures/org/`,
//! `print(parse(s)?) == s` byte-for-byte.
//!
//! The parser is a hand-written line-cursor classifier that captures each
//! region's source span into a shared `Arc<str>` held by the document.
//! Printing slices back into that source, so unedited documents roundtrip
//! by construction. Later cycles add structured editing where mutated
//! nodes set a dirty flag and re-serialize from structured fields.
//!
//! `Span` is deliberately `pub(crate)`. Nothing outside `closure-org` ever
//! sees a byte offset — that's the firewall that keeps CRDT, shells, and
//! adapters clean (spec invariants I7, I8).

#![forbid(unsafe_code)]

use std::sync::Arc;
use thiserror::Error;

/// A parsed org document.
///
/// Owns its source once (via [`Arc<str>`]) and every [`Node`] or
/// [`Headline`] references a slice of it by span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgDoc {
    source: Arc<str>,
    preamble: Vec<Node>,
    roots: Vec<Headline>,
}

impl OrgDoc {
    /// Nodes preceding the first headline.
    #[must_use]
    pub fn preamble(&self) -> &[Node] {
        &self.preamble
    }

    /// Top-level headlines. In the current cycle they are siblings; the
    /// nesting cycle restructures into a tree.
    #[must_use]
    pub fn roots(&self) -> &[Headline] {
        &self.roots
    }

    /// The document's full source text.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// True iff the document has no preamble nodes and no headlines.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.preamble.is_empty() && self.roots.is_empty()
    }

    /// True iff the document has at least one headline.
    #[must_use]
    pub const fn has_headlines(&self) -> bool {
        !self.roots.is_empty()
    }

    /// True iff the document source contains a `:NAME:` ... `:END:`
    /// drawer with the given name (anywhere in the source).
    #[must_use]
    pub fn has_drawer(&self, name: &str) -> bool {
        find_drawers(&self.source).iter().any(|d| d.name == name)
    }

    /// True iff any headline in the document has `tag`.
    #[must_use]
    pub fn has_tag(&self, tag: &str) -> bool {
        fn walk(h: &Headline, target: &str) -> bool {
            h.has_tag(target) || h.children().iter().any(|c| walk(c, target))
        }
        self.roots.iter().any(|r| walk(r, tag))
    }

    /// True iff any headline in the document has TODO `keyword`.
    #[must_use]
    pub fn has_todo(&self, keyword: &str) -> bool {
        fn walk(h: &Headline, target: &str) -> bool {
            h.todo() == Some(target) || h.children().iter().any(|c| walk(c, target))
        }
        self.roots.iter().any(|r| walk(r, keyword))
    }

    /// True iff any headline carries the given priority letter.
    #[must_use]
    pub fn has_priority(&self, letter: char) -> bool {
        fn walk(h: &Headline, target: char) -> bool {
            h.priority() == Some(target) || h.children().iter().any(|c| walk(c, target))
        }
        self.roots.iter().any(|r| walk(r, letter))
    }

    /// True iff any headline contains a link to `target`.
    #[must_use]
    pub fn has_link_to(&self, target: &str) -> bool {
        !self.find_link_sources(target).is_empty()
    }

    /// Count headlines matching a predicate, recursively.
    #[must_use]
    pub fn count_headlines_where<F: Fn(&Headline) -> bool>(&self, pred: F) -> usize {
        fn walk<F: Fn(&Headline) -> bool>(h: &Headline, pred: &F) -> usize {
            usize::from(pred(h)) + h.children().iter().map(|c| walk(c, pred)).sum::<usize>()
        }
        self.roots.iter().map(|r| walk(r, &pred)).sum()
    }

    /// Count of TODO-marked headlines.
    #[must_use]
    pub fn count_todos(&self) -> usize {
        self.count_headlines_where(|h| h.todo().is_some())
    }

    /// Count of headlines with priority cookies.
    #[must_use]
    pub fn count_with_priority(&self) -> usize {
        self.count_headlines_where(|h| h.priority().is_some())
    }

    /// Count of headlines with planning lines.
    #[must_use]
    pub fn count_with_planning(&self) -> usize {
        self.count_headlines_where(|h| h.planning().is_some())
    }

    /// Count of leaf headlines.
    #[must_use]
    pub fn count_leaves(&self) -> usize {
        self.count_headlines_where(Headline::is_leaf)
    }

    /// Count of headlines tagged :ARCHIVE:.
    #[must_use]
    pub fn count_archived(&self) -> usize {
        self.count_headlines_where(Headline::is_archived)
    }

    /// Count of headlines prefixed `COMMENT`.
    #[must_use]
    pub fn count_comments(&self) -> usize {
        self.count_headlines_where(Headline::is_comment)
    }

    /// Count of headlines that carry an `:ID:` property.
    #[must_use]
    pub fn count_with_id(&self) -> usize {
        self.count_headlines_where(Headline::has_id)
    }

    /// Count of headlines that carry a `SCHEDULED:` timestamp.
    #[must_use]
    pub fn count_scheduled(&self) -> usize {
        self.count_headlines_where(Headline::is_scheduled)
    }

    /// Count of headlines that carry a `DEADLINE:` timestamp.
    #[must_use]
    pub fn count_with_deadline(&self) -> usize {
        self.count_headlines_where(Headline::has_deadline)
    }

    /// Count of headlines that carry a `CLOSED:` timestamp.
    #[must_use]
    pub fn count_closed(&self) -> usize {
        self.count_headlines_where(Headline::is_closed)
    }

    /// Count of headlines tagged with `tag` (recursive).
    #[must_use]
    pub fn count_tagged(&self, tag: &str) -> usize {
        self.count_headlines_where(|h| h.has_tag(tag))
    }

    /// Count of headlines with TODO `keyword` (recursive).
    #[must_use]
    pub fn count_todo(&self, keyword: &str) -> usize {
        self.count_headlines_where(|h| h.todo() == Some(keyword))
    }

    /// Count of headlines whose title contains `needle` (recursive).
    #[must_use]
    pub fn count_title_contains(&self, needle: &str) -> usize {
        self.count_headlines_where(|h| h.title().contains(needle))
    }

    /// Count of headlines at level `level`.
    #[must_use]
    pub fn count_at_level(&self, level: u8) -> usize {
        self.count_headlines_where(|h| h.level() == level)
    }

    /// Count of leaf headlines tagged with `tag`.
    #[must_use]
    pub fn count_leaves_with_tag(&self, tag: &str) -> usize {
        self.count_headlines_where(|h| h.is_leaf() && h.has_tag(tag))
    }

    /// Mean body word count over headlines (rounded down).
    #[must_use]
    pub fn mean_body_word_count(&self) -> usize {
        self.total_body_word_count()
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Mean body byte count over headlines (rounded down).
    #[must_use]
    pub fn mean_body_byte_count(&self) -> usize {
        self.total_body_bytes()
            .checked_div(self.headline_count())
            .unwrap_or(0)
    }

    /// Mean depth across headlines (rounded down).
    #[must_use]
    pub fn mean_depth(&self) -> usize {
        let total: usize = self.count_headlines_where(|_| true);
        let sum: usize = {
            fn walk(h: &Headline) -> usize {
                usize::from(h.level()) + h.children().iter().map(walk).sum::<usize>()
            }
            self.roots.iter().map(walk).sum()
        };
        sum.checked_div(total).unwrap_or(0)
    }

    /// Mean child count across non-leaf headlines.
    #[must_use]
    pub fn mean_branching(&self) -> usize {
        let total = self.count_headlines_where(|h| !h.is_leaf());
        let sum: usize = {
            fn walk(h: &Headline) -> usize {
                let me = if h.is_leaf() { 0 } else { h.child_count() };
                me + h.children().iter().map(walk).sum::<usize>()
            }
            self.roots.iter().map(walk).sum()
        };
        sum.checked_div(total).unwrap_or(0)
    }

    /// Returns every headline at exactly `level`.
    #[must_use]
    pub fn headlines_at_level(&self, level: u8) -> Vec<&Headline> {
        self.filter_headlines(|h| h.level() == level)
    }

    /// Returns every TODO-marked headline.
    #[must_use]
    pub fn todo_headlines(&self) -> Vec<&Headline> {
        self.filter_headlines(|h| h.todo().is_some())
    }

    /// Returns every leaf headline.
    #[must_use]
    pub fn leaf_headlines(&self) -> Vec<&Headline> {
        self.filter_headlines(Headline::is_leaf)
    }

    /// Returns every headline matching `pred` (depth-first).
    #[must_use]
    pub fn filter_headlines<F: Fn(&Headline) -> bool>(&self, pred: F) -> Vec<&Headline> {
        fn walk<'a, F: Fn(&Headline) -> bool>(
            h: &'a Headline,
            pred: &F,
            out: &mut Vec<&'a Headline>,
        ) {
            if pred(h) {
                out.push(h);
            }
            for c in h.children() {
                walk(c, pred, out);
            }
        }
        let mut out: Vec<&Headline> = Vec::new();
        for r in &self.roots {
            walk(r, &pred, &mut out);
        }
        out
    }

    /// Returns the first headline that satisfies `pred` (depth-first).
    #[must_use]
    pub fn find_headline<F: Fn(&Headline) -> bool>(&self, pred: F) -> Option<&Headline> {
        fn walk<'a, F: Fn(&Headline) -> bool>(h: &'a Headline, pred: &F) -> Option<&'a Headline> {
            if pred(h) {
                return Some(h);
            }
            for c in h.children() {
                if let Some(found) = walk(c, pred) {
                    return Some(found);
                }
            }
            None
        }
        for r in &self.roots {
            if let Some(found) = walk(r, &pred) {
                return Some(found);
            }
        }
        None
    }

    /// Returns the shortest title (in chars).
    #[must_use]
    pub fn shortest_title(&self) -> Option<&Headline> {
        fn walk<'a>(h: &'a Headline, best: &mut Option<&'a Headline>) {
            if best.is_none_or(|b| b.title().chars().count() > h.title().chars().count()) {
                *best = Some(h);
            }
            for c in h.children() {
                walk(c, best);
            }
        }
        let mut best: Option<&Headline> = None;
        for r in &self.roots {
            walk(r, &mut best);
        }
        best
    }

    /// Returns the longest title (in chars).
    #[must_use]
    pub fn longest_title(&self) -> Option<&Headline> {
        fn walk<'a>(h: &'a Headline, best: &mut Option<&'a Headline>) {
            if best.is_none_or(|b| b.title().chars().count() < h.title().chars().count()) {
                *best = Some(h);
            }
            for c in h.children() {
                walk(c, best);
            }
        }
        let mut best: Option<&Headline> = None;
        for r in &self.roots {
            walk(r, &mut best);
        }
        best
    }

    /// Returns the highest-priority headline (`'A' < 'B' < ...`).
    /// Headlines without priority cookies are skipped.
    #[must_use]
    pub fn highest_priority(&self) -> Option<&Headline> {
        fn walk<'a>(h: &'a Headline, best: &mut Option<&'a Headline>) {
            if let Some(p) = h.priority()
                && best.is_none_or(|b| b.priority().is_none_or(|q| p < q))
            {
                *best = Some(h);
            }
            for c in h.children() {
                walk(c, best);
            }
        }
        let mut best: Option<&Headline> = None;
        for r in &self.roots {
            walk(r, &mut best);
        }
        best
    }

    /// Returns the headline with the most outgoing links.
    #[must_use]
    pub fn most_linked(&self) -> Option<&Headline> {
        fn walk<'a>(h: &'a Headline, best: &mut Option<&'a Headline>) {
            if best.is_none_or(|b| b.link_count() < h.link_count()) {
                *best = Some(h);
            }
            for c in h.children() {
                walk(c, best);
            }
        }
        let mut best: Option<&Headline> = None;
        for r in &self.roots {
            walk(r, &mut best);
        }
        best
    }

    /// Returns the headline with the most properties drawer entries.
    #[must_use]
    pub fn most_propertied(&self) -> Option<&Headline> {
        fn walk<'a>(h: &'a Headline, best: &mut Option<&'a Headline>) {
            if best.is_none_or(|b| b.property_count() < h.property_count()) {
                *best = Some(h);
            }
            for c in h.children() {
                walk(c, best);
            }
        }
        let mut best: Option<&Headline> = None;
        for r in &self.roots {
            walk(r, &mut best);
        }
        best
    }

    /// Returns the most-tagged headline.
    #[must_use]
    pub fn most_tagged(&self) -> Option<&Headline> {
        fn walk<'a>(h: &'a Headline, best: &mut Option<&'a Headline>) {
            if best.is_none_or(|b| b.tag_count() < h.tag_count()) {
                *best = Some(h);
            }
            for c in h.children() {
                walk(c, best);
            }
        }
        let mut best: Option<&Headline> = None;
        for r in &self.roots {
            walk(r, &mut best);
        }
        best
    }

    /// Returns the headline with the most body words.
    #[must_use]
    pub fn longest_body(&self) -> Option<&Headline> {
        fn walk<'a>(h: &'a Headline, best: &mut Option<&'a Headline>) {
            if best.is_none_or(|b| b.body_word_count() < h.body_word_count()) {
                *best = Some(h);
            }
            for c in h.children() {
                walk(c, best);
            }
        }
        let mut best: Option<&Headline> = None;
        for r in &self.roots {
            walk(r, &mut best);
        }
        best
    }

    /// Returns the headline with the largest subtree (most descendants).
    #[must_use]
    pub fn largest_subtree(&self) -> Option<&Headline> {
        fn walk<'a>(h: &'a Headline, best: &mut Option<&'a Headline>) {
            if best.is_none_or(|b| b.descendant_count() < h.descendant_count()) {
                *best = Some(h);
            }
            for c in h.children() {
                walk(c, best);
            }
        }
        let mut best: Option<&Headline> = None;
        for r in &self.roots {
            walk(r, &mut best);
        }
        best
    }

    /// Returns the deepest leaf headline (max level among leaves).
    #[must_use]
    pub fn deepest_leaf(&self) -> Option<&Headline> {
        fn walk<'a>(h: &'a Headline, best: &mut Option<&'a Headline>) {
            if h.is_leaf() {
                if best.is_none_or(|b| b.level() < h.level()) {
                    *best = Some(h);
                }
            } else {
                for c in h.children() {
                    walk(c, best);
                }
            }
        }
        let mut best: Option<&Headline> = None;
        for r in &self.roots {
            walk(r, &mut best);
        }
        best
    }

    /// Number of top-level headlines.
    #[must_use]
    pub const fn root_count(&self) -> usize {
        self.roots.len()
    }

    /// Iterate top-level headlines.
    pub fn iter_roots(&self) -> impl Iterator<Item = &Headline> {
        self.roots.iter()
    }

    /// Number of preamble nodes.
    #[must_use]
    pub const fn preamble_len(&self) -> usize {
        self.preamble.len()
    }

    /// Iterate preamble nodes.
    pub fn iter_preamble(&self) -> impl Iterator<Item = &Node> {
        self.preamble.iter()
    }

    /// Total node count: preamble nodes plus every headline's body
    /// nodes (recursive). Headlines themselves are not included.
    #[must_use]
    pub fn total_node_count(&self) -> usize {
        fn walk(h: &Headline) -> usize {
            h.body().len() + h.children().iter().map(walk).sum::<usize>()
        }
        self.preamble.len() + self.roots.iter().map(walk).sum::<usize>()
    }

    /// Total tag occurrences across all headlines (recursive).
    #[must_use]
    pub fn total_tag_count(&self) -> usize {
        fn walk(h: &Headline) -> usize {
            h.tags().len() + h.children().iter().map(walk).sum::<usize>()
        }
        self.roots.iter().map(walk).sum()
    }

    /// Total properties drawer entries across all headlines.
    #[must_use]
    pub fn total_property_count(&self) -> usize {
        fn walk(h: &Headline) -> usize {
            h.properties().map_or(0, Properties::len) + h.children().iter().map(walk).sum::<usize>()
        }
        self.roots.iter().map(walk).sum()
    }

    /// Total code block count over preamble + every headline body.
    #[must_use]
    pub fn total_code_block_count(&self) -> usize {
        fn walk_cb(h: &Headline) -> usize {
            h.body_code_block_count() + h.children().iter().map(walk_cb).sum::<usize>()
        }
        let preamble = self.nodes_by_kind(NodeKind::CodeBlock).count();
        preamble + self.roots.iter().map(walk_cb).sum::<usize>()
    }

    /// Total list item count over preamble + every headline body.
    #[must_use]
    pub fn total_list_item_count(&self) -> usize {
        fn walk_li(h: &Headline) -> usize {
            h.body_list_item_count() + h.children().iter().map(walk_li).sum::<usize>()
        }
        let preamble = self.nodes_by_kind(NodeKind::ListItem).count();
        preamble + self.roots.iter().map(walk_li).sum::<usize>()
    }

    /// Sum of body word counts across every headline (recursive).
    #[must_use]
    pub fn total_body_word_count(&self) -> usize {
        fn walk(h: &Headline) -> usize {
            h.body_word_count() + h.children().iter().map(walk).sum::<usize>()
        }
        self.roots.iter().map(walk).sum()
    }

    /// Total table row count over preamble + every headline body.
    #[must_use]
    pub fn total_table_row_count(&self) -> usize {
        fn walk_tr(h: &Headline) -> usize {
            h.body_table_row_count() + h.children().iter().map(walk_tr).sum::<usize>()
        }
        let preamble = self.nodes_by_kind(NodeKind::TableRow).count();
        preamble + self.roots.iter().map(walk_tr).sum::<usize>()
    }

    /// Total paragraph count over preamble + every headline body.
    #[must_use]
    pub fn total_paragraph_count(&self) -> usize {
        fn walk_p(h: &Headline) -> usize {
            h.body_paragraph_count() + h.children().iter().map(walk_p).sum::<usize>()
        }
        let preamble = self.nodes_by_kind(NodeKind::Paragraph).count();
        preamble + self.roots.iter().map(walk_p).sum::<usize>()
    }

    /// Total comment line count over preamble + every body.
    #[must_use]
    pub fn total_comment_count(&self) -> usize {
        fn walk_c(h: &Headline) -> usize {
            h.body_comment_count() + h.children().iter().map(walk_c).sum::<usize>()
        }
        let preamble = self.nodes_by_kind(NodeKind::Comment).count();
        preamble + self.roots.iter().map(walk_c).sum::<usize>()
    }

    /// Total blank line count over preamble + every body.
    #[must_use]
    pub fn total_blank_line_count(&self) -> usize {
        fn walk_b(h: &Headline) -> usize {
            h.body_blank_line_count() + h.children().iter().map(walk_b).sum::<usize>()
        }
        let preamble = self.nodes_by_kind(NodeKind::BlankLine).count();
        preamble + self.roots.iter().map(walk_b).sum::<usize>()
    }

    /// Total keyword line count over preamble + every body.
    #[must_use]
    pub fn total_keyword_count(&self) -> usize {
        fn walk_k(h: &Headline) -> usize {
            h.body_keyword_count() + h.children().iter().map(walk_k).sum::<usize>()
        }
        let preamble = self.nodes_by_kind(NodeKind::Keyword).count();
        preamble + self.roots.iter().map(walk_k).sum::<usize>()
    }

    /// Total timestamp count across every headline (recursive).
    #[must_use]
    pub fn total_timestamp_count(&self) -> usize {
        fn walk(h: &Headline) -> usize {
            h.timestamp_count() + h.children().iter().map(walk).sum::<usize>()
        }
        self.roots.iter().map(walk).sum()
    }

    /// Total cookie count across every headline.
    #[must_use]
    pub fn total_cookie_count(&self) -> usize {
        fn walk(h: &Headline) -> usize {
            h.cookie_count() + h.children().iter().map(walk).sum::<usize>()
        }
        self.roots.iter().map(walk).sum()
    }

    /// Total footnote count across every headline.
    #[must_use]
    pub fn total_footnote_count(&self) -> usize {
        fn walk(h: &Headline) -> usize {
            h.footnote_count() + h.children().iter().map(walk).sum::<usize>()
        }
        self.roots.iter().map(walk).sum()
    }

    /// Total macro count across every headline.
    #[must_use]
    pub fn total_macro_count(&self) -> usize {
        fn walk(h: &Headline) -> usize {
            h.macro_count() + h.children().iter().map(walk).sum::<usize>()
        }
        self.roots.iter().map(walk).sum()
    }

    /// Whether any headline in the tree has `:ID: id`.
    #[must_use]
    pub fn contains_id(&self, id: &str) -> bool {
        self.headline_by_id(id).is_some()
    }

    /// Total byte count of every subtree in the document. Roughly the
    /// source length minus preamble bytes.
    #[must_use]
    pub fn total_subtree_bytes(&self) -> usize {
        self.roots.iter().map(Headline::subtree_byte_count).sum()
    }

    /// Sum of body bytes across every headline's body in the document.
    #[must_use]
    pub fn total_body_bytes(&self) -> usize {
        fn walk(h: &Headline) -> usize {
            h.body_byte_count() + h.children().iter().map(walk).sum::<usize>()
        }
        self.roots.iter().map(walk).sum()
    }

    /// Total number of links across every headline (title + body).
    #[must_use]
    pub fn total_link_count(&self) -> usize {
        fn walk(h: &Headline) -> usize {
            h.link_targets().len() + h.children().iter().map(walk).sum::<usize>()
        }
        self.roots.iter().map(walk).sum()
    }

    /// Headlines whose title or body contains a link to `target`.
    /// Both `id:<ULID>` and bare `<ULID>` are accepted.
    #[must_use]
    pub fn find_link_sources(&self, target: &str) -> Vec<&Headline> {
        fn walk<'a>(h: &'a Headline, want: &str, bare: &str, out: &mut Vec<&'a Headline>) {
            if h.link_targets()
                .iter()
                .any(|t| t == want || t == bare || t.strip_prefix("id:") == Some(bare))
            {
                out.push(h);
            }
            for c in h.children() {
                walk(c, want, bare, out);
            }
        }
        let bare = target.strip_prefix("id:").unwrap_or(target);
        let mut out: Vec<&Headline> = Vec::new();
        for r in &self.roots {
            walk(r, target, bare, &mut out);
        }
        out
    }

    /// Lookup a headline whose `:ID:` property equals `id`. Walks the
    /// tree depth-first and returns the first match.
    #[must_use]
    pub fn headline_by_id(&self, id: &str) -> Option<&Headline> {
        fn walk<'a>(h: &'a Headline, target: &str) -> Option<&'a Headline> {
            if h.properties().and_then(Properties::id) == Some(target) {
                return Some(h);
            }
            for c in h.children() {
                if let Some(found) = walk(c, target) {
                    return Some(found);
                }
            }
            None
        }
        for r in &self.roots {
            if let Some(found) = walk(r, id) {
                return Some(found);
            }
        }
        None
    }

    /// Iterate preamble nodes whose kind matches `target`.
    pub fn nodes_by_kind(&self, target: NodeKind) -> impl Iterator<Item = &Node> {
        self.preamble.iter().filter(move |n| n.kind == target)
    }

    /// Every preamble list item with a checkbox in the unchecked or
    /// partial state.
    #[must_use]
    pub fn unfinished_checkboxes(&self) -> Vec<ListItemView<'_>> {
        self.preamble
            .iter()
            .filter_map(Node::as_list_item)
            .filter(|li| {
                matches!(
                    li.checkbox,
                    Some(Checkbox::Unchecked | Checkbox::Partial)
                )
            })
            .collect()
    }

    /// Histogram of preamble node kinds. Keys ordered alphabetically.
    #[must_use]
    pub fn preamble_kind_counts(&self) -> Vec<(NodeKind, usize)> {
        let mut counts: std::collections::BTreeMap<&str, (NodeKind, usize)> =
            std::collections::BTreeMap::new();
        for n in &self.preamble {
            let key = match n.kind {
                NodeKind::BlankLine => "BlankLine",
                NodeKind::Comment => "Comment",
                NodeKind::Keyword => "Keyword",
                NodeKind::Paragraph => "Paragraph",
                NodeKind::CodeBlock => "CodeBlock",
                NodeKind::ListItem => "ListItem",
                NodeKind::TableRow => "TableRow",
            };
            counts
                .entry(key)
                .and_modify(|(_, c)| *c += 1)
                .or_insert((n.kind, 1));
        }
        counts.into_values().collect()
    }

    /// Total headline count (every level, recursive).
    #[must_use]
    pub fn headline_count(&self) -> usize {
        fn walk(h: &Headline) -> usize {
            1 + h.children().iter().map(walk).sum::<usize>()
        }
        self.roots.iter().map(walk).sum()
    }

    /// Every distinct todo keyword used in the document, sorted.
    #[must_use]
    pub fn all_todos(&self) -> Vec<&str> {
        fn walk_todos<'a>(h: &'a Headline, out: &mut std::collections::BTreeSet<&'a str>) {
            if let Some(t) = h.todo() {
                out.insert(t);
            }
            for c in h.children() {
                walk_todos(c, out);
            }
        }
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for r in &self.roots {
            walk_todos(r, &mut seen);
        }
        seen.into_iter().collect()
    }

    /// Every distinct tag used across all headlines, sorted.
    #[must_use]
    pub fn all_tags(&self) -> Vec<&str> {
        fn walk_tags<'a>(h: &'a Headline, out: &mut std::collections::BTreeSet<&'a str>) {
            for t in h.tags() {
                out.insert(t);
            }
            for c in h.children() {
                walk_tags(c, out);
            }
        }
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for r in &self.roots {
            walk_tags(r, &mut seen);
        }
        seen.into_iter().collect()
    }

    /// Maximum nesting depth across every headline (root level = 1).
    /// Empty documents return 0.
    #[must_use]
    pub fn max_depth(&self) -> u8 {
        fn walk(h: &Headline) -> u8 {
            let child_max = h.children().iter().map(walk).max().unwrap_or(0);
            h.level().max(child_max)
        }
        self.roots.iter().map(walk).max().unwrap_or(0)
    }

    /// Bundled `(chars, words, lines, headlines)` counts.
    #[must_use]
    pub fn wc(&self) -> (usize, usize, usize, usize) {
        (
            self.source.chars().count(),
            self.source.split_whitespace().count(),
            self.source.lines().count(),
            self.headline_count(),
        )
    }

    /// 64-bit FNV-1a hash of the source. Stable across runs and
    /// machines; intended for cache keying and change detection. Two
    /// byte-equal sources yield the same hash regardless of how they
    /// were parsed.
    #[must_use]
    pub fn source_hash(&self) -> u64 {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = OFFSET;
        for b in self.source.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(PRIME);
        }
        h
    }

    fn source_of(&self, span: Span) -> &str {
        &self.source[span.start..span.end]
    }

    /// Lookup a document-level keyword (`#+KEY: value`) in the preamble.
    /// Match is case-insensitive on the key. Returns the value with
    /// surrounding whitespace trimmed. Returns the first match if a
    /// keyword appears more than once.
    #[must_use]
    pub fn keyword(&self, key: &str) -> Option<&str> {
        for n in &self.preamble {
            if n.kind != NodeKind::Keyword {
                continue;
            }
            let text = &self.source[n.span.start..n.span.end];
            let body = text.strip_prefix("#+")?;
            let (k, v) = body.split_once(':')?;
            if k.eq_ignore_ascii_case(key) {
                return Some(v.trim_matches([' ', '\t', '\n', '\r']));
            }
        }
        None
    }

    /// `#+TITLE:` value if set in the preamble.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.keyword("TITLE")
    }

    /// `#+AUTHOR:` value if set.
    #[must_use]
    pub fn author(&self) -> Option<&str> {
        self.keyword("AUTHOR")
    }

    /// `#+DATE:` value if set.
    #[must_use]
    pub fn date(&self) -> Option<&str> {
        self.keyword("DATE")
    }

    /// `#+FILETAGS:` parsed as a list of `:tag:` separated names. Returns
    /// an empty `Vec` when the keyword is absent.
    #[must_use]
    pub fn filetags(&self) -> Vec<&str> {
        self.keyword("FILETAGS").map_or_else(Vec::new, |raw| {
            raw.split(':').filter(|s| !s.is_empty()).collect()
        })
    }

    /// Group consecutive list items in the preamble into [`ListGroup`]s.
    /// A blank line or any non-list node breaks the group.
    #[must_use]
    pub fn lists(&self) -> Vec<ListGroup<'_>> {
        let mut out: Vec<ListGroup<'_>> = Vec::new();
        let mut current: Vec<ListItemView<'_>> = Vec::new();
        for n in &self.preamble {
            if n.kind == NodeKind::ListItem
                && let Some(view) = n.as_list_item()
            {
                current.push(view);
            } else if !current.is_empty() {
                out.push(ListGroup {
                    items: std::mem::take(&mut current),
                });
            }
        }
        if !current.is_empty() {
            out.push(ListGroup { items: current });
        }
        out
    }

    /// Group consecutive table rows in the preamble into [`TableView`]s.
    /// A blank line, headline, or any non-table node breaks the group.
    #[must_use]
    pub fn tables(&self) -> Vec<TableView<'_>> {
        let mut out: Vec<TableView<'_>> = Vec::new();
        let mut current: Vec<TableRowView<'_>> = Vec::new();
        for n in &self.preamble {
            if n.kind == NodeKind::TableRow {
                let line = &self.source[n.span.start..n.span.end];
                let trimmed = line.strip_suffix('\n').unwrap_or(line);
                let is_separator = trimmed.starts_with("|-") || trimmed.starts_with("| -");
                let cells: Vec<&str> = if is_separator {
                    Vec::new()
                } else {
                    let inner = trimmed
                        .strip_prefix('|')
                        .and_then(|s| s.strip_suffix('|'))
                        .unwrap_or(trimmed);
                    inner.split('|').map(str::trim).collect()
                };
                current.push(TableRowView {
                    is_separator,
                    cells,
                });
            } else if !current.is_empty() {
                out.push(TableView {
                    rows: std::mem::take(&mut current),
                });
            }
        }
        if !current.is_empty() {
            out.push(TableView { rows: current });
        }
        out
    }

    /// All `#+KEY: value` keyword lines in the preamble in source order.
    /// Both key and value are returned trimmed of surrounding whitespace.
    #[must_use]
    pub fn all_keywords(&self) -> Vec<(&str, &str)> {
        let mut out: Vec<(&str, &str)> = Vec::new();
        for n in &self.preamble {
            if n.kind != NodeKind::Keyword {
                continue;
            }
            let text = &self.source[n.span.start..n.span.end];
            let Some(body) = text.strip_prefix("#+") else {
                continue;
            };
            let Some((k, v)) = body.split_once(':') else {
                continue;
            };
            out.push((k.trim(), v.trim_matches([' ', '\t', '\n', '\r'])));
        }
        out
    }
}

/// Failure mode for structure-preserving rewrites.
#[derive(Debug, Error)]
pub enum RewriteError {
    /// No headline exists at the given path.
    #[error("no headline at the given path")]
    NotFound,
    /// Rewriting produced source the parser could not accept.
    #[error("rewrite produced unparsable source")]
    Parse,
}

/// Rewrite a headline's title in-place and return a freshly parsed
/// [`OrgDoc`]. Byte-exact roundtrip holds on the unchanged portion of
/// the source; only the `title_span` is replaced.
pub fn rewrite_headline_title(
    doc: &OrgDoc,
    path: &[usize],
    new_title: &str,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let mut src = doc.source().to_owned();
    src.replace_range(target.title_span.start..target.title_span.end, new_title);
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Set or clear the TODO keyword on a headline.
///
/// `keyword: Some("TODO")` sets the keyword (replaces the existing one
/// if present); `keyword: None` removes it. The rest of the header
/// line (priority, title, tags) is preserved verbatim.
pub fn rewrite_headline_set_todo(
    doc: &OrgDoc,
    path: &[usize],
    keyword: Option<&str>,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    // Determine the current "header content" region: between stars+space
    // and the trailing `\n`. Replace the leading TODO+space (if any) and
    // re-emit with the new keyword.
    let header = &doc.source()[target.header_span.start..target.header_span.end];
    let body = header.strip_suffix('\n').unwrap_or(header);
    let stars = body.chars().take_while(|&c| c == '*').count();
    let after_stars = &body[stars..];
    let ws_skip = after_stars
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    let content = &after_stars[ws_skip..];

    let stripped = target.todo_span.map_or(content, |span| {
        let kw_len = span.end - span.start;
        let after_kw = &content[kw_len..];
        after_kw.strip_prefix(' ').unwrap_or(after_kw)
    });

    let stars_str = "*".repeat(stars);
    let new_header = keyword.map_or_else(
        || format!("{stars_str} {stripped}\n"),
        |k| format!("{stars_str} {k} {stripped}\n"),
    );

    let mut src = doc.source().to_owned();
    src.replace_range(
        target.header_span.start..target.header_span.end,
        &new_header,
    );
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Set or clear the `[#X]` priority on a headline.
///
/// `Some('A')` inserts a priority cookie immediately after the optional
/// TODO keyword; `None` removes any existing cookie. Title and tags
/// are preserved verbatim.
pub fn rewrite_headline_set_priority(
    doc: &OrgDoc,
    path: &[usize],
    priority: Option<char>,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let header = &doc.source()[target.header_span.start..target.header_span.end];
    let body = header.strip_suffix('\n').unwrap_or(header);
    let stars = body.chars().take_while(|&c| c == '*').count();
    let after_stars = &body[stars..];
    let ws = after_stars
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    let content = &body[stars + ws..];

    let mut prefix = String::new();
    let mut rest: &str = target.todo_span.map_or(content, |span| {
        let kw_len = span.end - span.start;
        prefix.push_str(&content[..kw_len]);
        prefix.push(' ');
        let after = &content[kw_len..];
        after.strip_prefix(' ').unwrap_or(after)
    });

    // Strip any existing `[#X]` priority cookie from `rest`.
    if rest.starts_with("[#") && rest.len() >= 4 && rest.as_bytes()[3] == b']' {
        let after = &rest[4..];
        rest = after.strip_prefix(' ').unwrap_or(after);
    }

    if let Some(p) = priority {
        use std::fmt::Write as _;
        let _ = write!(prefix, "[#{p}] ");
    }

    let stars_str = "*".repeat(stars);
    let new_header = format!("{stars_str} {prefix}{rest}\n");

    let mut src = doc.source().to_owned();
    src.replace_range(
        target.header_span.start..target.header_span.end,
        &new_header,
    );
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Set the trailing tag list on a headline.
///
/// `tags: &[]` clears the tag block. Otherwise the trailing
/// `:tag1:tag2:` block is replaced wholesale (or appended if absent).
/// Title, TODO, and priority are preserved.
pub fn rewrite_headline_set_tags(
    doc: &OrgDoc,
    path: &[usize],
    tags: &[&str],
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let header = &doc.source()[target.header_span.start..target.header_span.end];
    let body = header.strip_suffix('\n').unwrap_or(header);

    // Determine where the trailing tag block sits in `body`.
    let trim_end = target.tag_spans.first().map_or_else(
        || body.trim_end_matches([' ', '\t']).len(),
        |first_tag| {
            let start_in_body = first_tag.start - target.header_span.start;
            // The opening `:` lives at start_in_body - 1; trim
            // whitespace before it to drop the separator.
            body[..start_in_body - 1]
                .trim_end_matches([' ', '\t'])
                .len()
        },
    );

    let title_part = &body[..trim_end];

    let new_header = if tags.is_empty() {
        format!("{title_part}\n")
    } else {
        let block: String = tags.iter().fold(String::from(":"), |mut s, t| {
            s.push_str(t);
            s.push(':');
            s
        });
        format!("{title_part} {block}\n")
    };

    let mut src = doc.source().to_owned();
    src.replace_range(
        target.header_span.start..target.header_span.end,
        &new_header,
    );
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Toggle the checkbox on the Nth preamble list item.
///
/// `[ ]` and `[-]` flip to `[X]`. `[X]` flips back to `[ ]`. List items
/// without a checkbox return `RewriteError::NotFound`.
pub fn rewrite_toggle_checkbox(doc: &OrgDoc, item_index: usize) -> Result<OrgDoc, RewriteError> {
    let mut idx = 0usize;
    let mut found: Option<(Span, Checkbox)> = None;
    for n in &doc.preamble {
        if n.kind == NodeKind::ListItem {
            if idx == item_index {
                if let NodeMeta::ListItem { checkbox, .. } = &n.meta
                    && let Some(cb) = checkbox
                {
                    found = Some((n.span, *cb));
                }
                break;
            }
            idx += 1;
        }
    }
    let (line_span, current) = found.ok_or(RewriteError::NotFound)?;
    let line = &doc.source()[line_span.start..line_span.end];
    let cb_offset = line
        .find("[ ]")
        .or_else(|| line.find("[X]"))
        .or_else(|| line.find("[x]"))
        .or_else(|| line.find("[-]"))
        .ok_or(RewriteError::Parse)?;
    let new_marker = match current {
        Checkbox::Checked => "[ ]",
        Checkbox::Unchecked | Checkbox::Partial => "[X]",
    };
    let mut src = doc.source().to_owned();
    let abs_start = line_span.start + cb_offset;
    src.replace_range(abs_start..abs_start + 3, new_marker);
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Remove a named drawer (`:NAME:` ... `:END:`) from a headline.
///
/// Searches the source from the header onwards for the first
/// occurrence of `:NAME:\n` followed eventually by `:END:\n`. If
/// found, that block is excised. No-op if the drawer is absent.
pub fn rewrite_headline_clear_drawer(
    doc: &OrgDoc,
    path: &[usize],
    name: &str,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let header_end = target.header_span.end;
    let src = doc.source();
    let after = &src[header_end..];
    let needle = format!(":{name}:\n");
    let Some(rel) = after.find(&needle) else {
        return Ok(doc.clone());
    };
    let drawer_start = header_end + rel;
    let after_open = drawer_start + needle.len();
    let Some(end_rel) = src[after_open..].find(":END:") else {
        return Err(RewriteError::Parse);
    };
    let close_line_end = src[after_open + end_rel..]
        .find('\n')
        .map_or(src.len(), |n| after_open + end_rel + n + 1);
    let mut new_src = src.to_owned();
    new_src.replace_range(drawer_start..close_line_end, "");
    parse(&new_src).map_err(|_| RewriteError::Parse)
}

/// Append a line to the headline's `:LOGBOOK:` drawer.
///
/// The drawer is created (immediately after the header line, before
/// any other drawer) if absent. `entry` should be a single line
/// without the trailing newline.
pub fn rewrite_headline_append_logbook(
    doc: &OrgDoc,
    path: &[usize],
    entry: &str,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let after_header = target.header_span.end;
    let src = doc.source();
    let after = &src[after_header..];
    let after_drawer_start = after_header + after.find(":LOGBOOK:\n").unwrap_or(usize::MAX / 2);

    let mut new_src = src.to_owned();
    if let Some(rel) = after.find(":LOGBOOK:\n") {
        let logbook_start = after_header + rel;
        let after_logbook = &src[logbook_start + ":LOGBOOK:\n".len()..];
        let end_rel = after_logbook.find(":END:").ok_or(RewriteError::Parse)?;
        let insert_at = logbook_start + ":LOGBOOK:\n".len() + end_rel;
        let mut buf = String::from(entry);
        buf.push('\n');
        new_src.insert_str(insert_at, &buf);
    } else {
        let _ = after_drawer_start;
        let mut buf = String::from(":LOGBOOK:\n");
        buf.push_str(entry);
        buf.push('\n');
        buf.push_str(":END:\n");
        new_src.insert_str(after_header, &buf);
    }
    parse(&new_src).map_err(|_| RewriteError::Parse)
}

/// Toggle the `COMMENT` keyword on a headline. The keyword is a prefix
/// to the title (`* COMMENT Foo`) that excludes the headline from
/// agenda views, exports, and code-block evaluation in Emacs Org.
pub fn rewrite_headline_toggle_comment(
    doc: &OrgDoc,
    path: &[usize],
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let title = &doc.source()[target.title_span.start..target.title_span.end];
    let new_title = if title == "COMMENT" || title.starts_with("COMMENT ") {
        title
            .strip_prefix("COMMENT")
            .unwrap_or(title)
            .trim_start()
            .to_owned()
    } else if title.is_empty() {
        "COMMENT".to_owned()
    } else {
        format!("COMMENT {title}")
    };
    rewrite_headline_title(doc, path, &new_title)
}

/// Toggle the `ARCHIVE` tag on a headline. Adds it when absent,
/// removes it when present. Other tags are preserved in source order.
pub fn rewrite_headline_toggle_archive(
    doc: &OrgDoc,
    path: &[usize],
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let mut tags: Vec<String> = target.tags().into_iter().map(str::to_owned).collect();
    if let Some(pos) = tags.iter().position(|t| t == "ARCHIVE") {
        tags.remove(pos);
    } else {
        tags.push("ARCHIVE".to_owned());
    }
    let refs: Vec<&str> = tags.iter().map(String::as_str).collect();
    rewrite_headline_set_tags(doc, path, &refs)
}

/// Insert a new sibling headline immediately after the subtree
/// rooted at `path`, with the same level.
pub fn rewrite_add_sibling_after(
    doc: &OrgDoc,
    path: &[usize],
    title: &str,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let end = subtree_end(target);
    let stars = "*".repeat(usize::from(target.level));
    let insert = format!("{stars} {title}\n");
    let mut src = doc.source().to_owned();
    src.insert_str(end, &insert);
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Replace a headline's body (the text between its `:PROPERTIES:`
/// drawer and its first child or subtree end) with `new_body`.
///
/// `new_body` is appended verbatim. Pass an empty string to clear the
/// body. The properties drawer, header line, and recursive children
/// are preserved.
pub fn rewrite_headline_set_body(
    doc: &OrgDoc,
    path: &[usize],
    new_body: &str,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let body_start = target
        .properties
        .as_ref()
        .map_or(target.header_span.end, |p| p.drawer_span.end);
    let body_end = target
        .children
        .first()
        .map_or_else(|| subtree_end(target), |c| c.header_span.start);
    let mut src = doc.source().to_owned();
    src.replace_range(body_start..body_end, new_body);
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Remove any planning line (`SCHEDULED:` / `DEADLINE:` / `CLOSED:`)
/// directly under the headline. Equivalent to calling
/// [`rewrite_headline_set_planning`] with all `None`.
pub fn rewrite_headline_remove_planning(
    doc: &OrgDoc,
    path: &[usize],
) -> Result<OrgDoc, RewriteError> {
    rewrite_headline_set_planning(doc, path, None, None, None)
}

/// Set or clear the planning line (`SCHEDULED:`, `DEADLINE:`, `CLOSED:`)
/// directly under the headline.
///
/// All three timestamp parameters are optional; pass `None` for any to
/// omit it. Passing all three as `None` removes any existing planning
/// line. Each timestamp string should already include its `<...>` or
/// `[...]` brackets.
pub fn rewrite_headline_set_planning(
    doc: &OrgDoc,
    path: &[usize],
    scheduled: Option<&str>,
    deadline: Option<&str>,
    closed: Option<&str>,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let src = doc.source();
    let after_header = target.header_span.end;
    let line_end = src[after_header..]
        .find('\n')
        .map_or(src.len(), |n| after_header + n + 1);
    let first_line = &src[after_header..line_end];
    let existing_is_planning = is_planning_line(first_line);
    let replace_end = if existing_is_planning {
        line_end
    } else {
        after_header
    };

    let new_line = format_planning_line(scheduled, deadline, closed);
    let mut new_src = src.to_owned();
    new_src.replace_range(after_header..replace_end, &new_line);
    parse(&new_src).map_err(|_| RewriteError::Parse)
}

fn is_planning_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("SCHEDULED:")
        || trimmed.starts_with("DEADLINE:")
        || trimmed.starts_with("CLOSED:")
}

fn format_planning_line(
    scheduled: Option<&str>,
    deadline: Option<&str>,
    closed: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(s) = scheduled {
        parts.push(format!("SCHEDULED: {s}"));
    }
    if let Some(d) = deadline {
        parts.push(format!("DEADLINE: {d}"));
    }
    if let Some(c) = closed {
        parts.push(format!("CLOSED: {c}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{}\n", parts.join(" "))
    }
}

/// Replace the content of the Nth preamble code block with
/// `new_content`. The fence lines (`#+BEGIN_SRC ...` and
/// `#+END_SRC`) are preserved.
pub fn rewrite_code_block_content(
    doc: &OrgDoc,
    block_index: usize,
    new_content: &str,
) -> Result<OrgDoc, RewriteError> {
    let mut idx = 0usize;
    let mut content_span: Option<Span> = None;
    for n in doc.preamble() {
        if n.kind() == NodeKind::CodeBlock {
            if idx == block_index {
                if let NodeMeta::CodeBlock {
                    content_span: cs, ..
                } = &n.meta
                {
                    content_span = Some(*cs);
                }
                break;
            }
            idx += 1;
        }
    }
    let span = content_span.ok_or(RewriteError::NotFound)?;
    let mut src = doc.source().to_owned();
    src.replace_range(span.start..span.end, new_content);
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Attach a `#+RESULTS:` block to the Nth preamble code block.
///
/// The results lines are formatted as `: <line>` org-babel-style
/// verbatim output. If a `#+RESULTS:` block already follows the code
/// block, it is replaced.
pub fn rewrite_attach_results_to_code_block(
    doc: &OrgDoc,
    block_index: usize,
    results: &str,
) -> Result<OrgDoc, RewriteError> {
    // Find the Nth CodeBlock node in the preamble, get its end span.
    let mut idx = 0usize;
    let mut block_end: Option<usize> = None;
    for n in doc.preamble() {
        if n.kind() == NodeKind::CodeBlock {
            if idx == block_index {
                block_end = Some(n.span.end);
                break;
            }
            idx += 1;
        }
    }
    let block_end = block_end.ok_or(RewriteError::NotFound)?;

    let mut formatted = String::from("#+RESULTS:\n");
    for line in results.lines() {
        formatted.push_str(": ");
        formatted.push_str(line);
        formatted.push('\n');
    }
    if results.is_empty() {
        formatted.push_str(":\n");
    }

    // If existing `#+RESULTS:` block immediately follows, replace it.
    let src = doc.source();
    let after = &src[block_end..];
    let trimmed_lead = after.trim_start_matches([' ', '\t', '\n']);
    let lead_skipped = after.len() - trimmed_lead.len();
    let replace_end =
        if trimmed_lead.starts_with("#+RESULTS:") || trimmed_lead.starts_with("#+results:") {
            // Consume the `#+RESULTS:` line and following `: ...` lines.
            let mut cursor = block_end + lead_skipped;
            // Skip the `#+RESULTS:` line.
            if let Some(nl) = src[cursor..].find('\n') {
                cursor += nl + 1;
            }
            // Skip subsequent `: ...` lines.
            loop {
                let rest = &src[cursor..];
                if rest.starts_with(": ") || rest.starts_with(":\n") || rest.starts_with(":\r\n") {
                    if let Some(nl) = rest.find('\n') {
                        cursor += nl + 1;
                    } else {
                        cursor = src.len();
                        break;
                    }
                } else {
                    break;
                }
            }
            cursor
        } else {
            block_end
        };

    let mut new_src = doc.source().to_owned();
    new_src.replace_range(block_end..replace_end, &formatted);
    parse(&new_src).map_err(|_| RewriteError::Parse)
}

/// Splice a pre-formatted subtree source verbatim immediately after
/// the subtree rooted at `after_path`. Used by move/cut/paste paths
/// that already have a captured source string.
pub fn rewrite_splice_subtree_after(
    doc: &OrgDoc,
    after_path: &[usize],
    subtree_source: &str,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, after_path).ok_or(RewriteError::NotFound)?;
    let end = subtree_end(target);
    let mut src = doc.source().to_owned();
    src.insert_str(end, subtree_source);
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Insert a sibling headline with a pre-pinned `:ID:` so the kernel
/// can address it deterministically across rebuilds.
pub fn rewrite_add_sibling_after_with_id(
    doc: &OrgDoc,
    path: &[usize],
    title: &str,
    id: &str,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let end = subtree_end(target);
    let stars = "*".repeat(usize::from(target.level));
    let insert = format!("{stars} {title}\n:PROPERTIES:\n:ID: {id}\n:END:\n");
    let mut src = doc.source().to_owned();
    src.insert_str(end, &insert);
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Remove the subtree rooted at `path` from the document. The header,
/// any property drawer, body nodes, and recursive children are all
/// dropped; the resulting source remains a valid org document.
pub fn rewrite_remove_subtree(doc: &OrgDoc, path: &[usize]) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let start = target.header_span.start;
    let end = subtree_end(target);
    let mut src = doc.source().to_owned();
    src.replace_range(start..end, "");
    parse(&src).map_err(|_| RewriteError::Parse)
}

fn subtree_end(h: &Headline) -> usize {
    let mut end = h.header_span.end;
    if let Some(p) = &h.properties {
        end = end.max(p.drawer_span.end);
    }
    for n in &h.body {
        end = end.max(n.span.end);
    }
    if let Some(last) = h.children.last() {
        end = end.max(subtree_end(last));
    }
    end
}

/// Decrease (promote) the headline's level by one.
///
/// Returns `RewriteError::Parse` if the headline is already at level 1
/// (cannot promote further). Children are NOT promoted; the caller
/// passes a level adjustment for the whole subtree separately if
/// desired.
pub fn rewrite_headline_promote(doc: &OrgDoc, path: &[usize]) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    if target.level <= 1 {
        return Err(RewriteError::Parse);
    }
    rewrite_stars(doc, target, target.level - 1)
}

/// Increase (demote) the headline's level by one. The new level is
/// capped at `u8::MAX`.
pub fn rewrite_headline_demote(doc: &OrgDoc, path: &[usize]) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let new_level = target.level.saturating_add(1);
    rewrite_stars(doc, target, new_level)
}

fn rewrite_stars(doc: &OrgDoc, target: &Headline, new_level: u8) -> Result<OrgDoc, RewriteError> {
    let header = &doc.source()[target.header_span.start..target.header_span.end];
    let (body, trailer) = header
        .strip_suffix('\n')
        .map_or((header, ""), |b| (b, "\n"));
    let stars = body.chars().take_while(|&c| c == '*').count();
    let after_stars = &body[stars..];
    let new_stars = "*".repeat(usize::from(new_level));
    let new_header = format!("{new_stars}{after_stars}{trailer}");
    let mut src = doc.source().to_owned();
    src.replace_range(
        target.header_span.start..target.header_span.end,
        &new_header,
    );
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Ensure the headline at `path` has an `:ID:` property.
///
/// If the drawer is absent, a fresh one is inserted immediately after
/// the headline's header line. If the drawer exists without an `:ID:`
/// entry, one is inserted at the top of the drawer. If the drawer
/// already has a different `:ID:`, this is a no-op — existing ids are
/// never replaced (I2).
/// Set or replace a `:KEY: value` entry in the headline's properties
/// drawer. Creates the drawer if absent. If `key` already exists, its
/// value is overwritten in place; otherwise the entry is appended just
/// before `:END:`.
pub fn rewrite_headline_set_property(
    doc: &OrgDoc,
    path: &[usize],
    key: &str,
    value: &str,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let mut src = doc.source().to_owned();
    if let Some(p) = &target.properties {
        if let Some(e) = p
            .entries
            .iter()
            .find(|e| &src[e.key_span.start..e.key_span.end] == key)
        {
            src.replace_range(e.value_span.start..e.value_span.end, value);
        } else {
            let end_marker = src[p.drawer_span.start..p.drawer_span.end]
                .rfind(":END:")
                .ok_or(RewriteError::Parse)?;
            let insert_at = p.drawer_span.start + end_marker;
            let insert = format!(":{key}: {value}\n");
            src.insert_str(insert_at, &insert);
        }
    } else {
        let after_header = target.header_span.end;
        let insert = format!(":PROPERTIES:\n:{key}: {value}\n:END:\n");
        src.insert_str(after_header, &insert);
    }
    parse(&src).map_err(|_| RewriteError::Parse)
}

/// Ensure the headline has a `:ID:` property pinned to `id`.
///
/// If the drawer is absent, a fresh one is inserted immediately after
/// the headline's header line. If the drawer exists without an `:ID:`
/// entry, one is inserted at the top of the drawer. If the drawer
/// already has a different `:ID:`, this is a no-op — existing ids are
/// never replaced (I2).
pub fn rewrite_headline_ensure_id(
    doc: &OrgDoc,
    path: &[usize],
    id: &str,
) -> Result<OrgDoc, RewriteError> {
    let target = navigate_headline(doc, path).ok_or(RewriteError::NotFound)?;
    let mut src = doc.source().to_owned();
    if let Some(p) = &target.properties {
        if p.entries
            .iter()
            .any(|e| &src[e.key_span.start..e.key_span.end] == "ID")
        {
            return Ok(doc.clone());
        }
        let after_open = find_line_end(&src, p.drawer_span.start).ok_or(RewriteError::Parse)?;
        let insert = format!(":ID: {id}\n");
        src.insert_str(after_open, &insert);
    } else {
        let after_header = target.header_span.end;
        let insert = format!(":PROPERTIES:\n:ID: {id}\n:END:\n");
        src.insert_str(after_header, &insert);
    }
    parse(&src).map_err(|_| RewriteError::Parse)
}

fn find_line_end(src: &str, from: usize) -> Option<usize> {
    let rest = src.get(from..)?;
    let nl = rest.find('\n')?;
    Some(from + nl + 1)
}

fn navigate_headline<'a>(doc: &'a OrgDoc, path: &[usize]) -> Option<&'a Headline> {
    let first = *path.first()?;
    let mut cur = doc.roots.get(first)?;
    for &i in &path[1..] {
        cur = cur.children.get(i)?;
    }
    Some(cur)
}

/// Byte range into [`OrgDoc::source`]. Crate-internal by design: exposing
/// byte offsets to the kernel or shells would break the span firewall
/// described in `docs/architecture.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// A line-level construct. [`Node::kind`] exposes the classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    source: Arc<str>,
    kind: NodeKind,
    span: Span,
    meta: NodeMeta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeMeta {
    None,
    CodeBlock {
        lang_span: Option<Span>,
        args_span: Option<Span>,
        content_span: Span,
    },
    ListItem {
        indent: usize,
        marker: ListMarker,
        checkbox: Option<Checkbox>,
        content_span: Span,
    },
}

/// List item marker kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMarker {
    /// `-` bullet.
    Dash,
    /// `+` bullet.
    Plus,
    /// `N.` ordered marker.
    OrderedDot,
    /// `N)` ordered marker.
    OrderedParen,
}

/// Checkbox state on a list item (`[ ]`, `[X]`, `[-]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkbox {
    /// Unchecked `[ ]`.
    Unchecked,
    /// Checked `[X]` or `[x]`.
    Checked,
    /// Partial `[-]`.
    Partial,
}

/// Structural view of a [`NodeKind::ListItem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListItemView<'a> {
    /// Leading indent width in columns (space/tab counted as 1 each).
    pub indent: usize,
    /// Bullet / ordered marker.
    pub marker: ListMarker,
    /// Checkbox if one is present after the marker.
    pub checkbox: Option<Checkbox>,
    /// Item content following the marker (and checkbox), with trailing
    /// newline stripped.
    pub content: &'a str,
}

/// Coarse classification of a [`Node`]. Structured fields per kind arrive
/// in later cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// A whitespace-only line (possibly empty).
    BlankLine,
    /// A `#` comment line starting at column 0, followed by space or EOL.
    Comment,
    /// A `#+KEY: value` metadata line starting at column 0.
    Keyword,
    /// One or more adjacent non-blank, non-comment, non-keyword lines.
    Paragraph,
    /// A `#+BEGIN_SRC` / `#+END_SRC` fenced block.
    CodeBlock,
    /// A single list item line (`-`, `+`, `1.`, `1)`, optionally with
    /// leading indent and `[ ]` / `[X]` / `[-]` checkbox).
    ListItem,
    /// A `| a | b |` table row (including the `|---|` separator rows).
    TableRow,
}

/// Structural view of a [`Node`] classified as [`NodeKind::CodeBlock`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeBlockView<'a> {
    /// Language identifier if one was specified on the begin line.
    pub language: Option<&'a str>,
    /// Raw header arguments after the language (e.g. `:results output`).
    pub args: Option<&'a str>,
    /// Verbatim content between begin and end lines, including trailing
    /// newline on each line.
    pub content: &'a str,
}

/// Org-style progress cookie: `[N/M]` checkbox count or `[P%]` percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieView {
    /// `[done/total]` count.
    Count {
        /// Done count.
        done: u32,
        /// Total count.
        total: u32,
    },
    /// `[N%]` percentage.
    Percent(u32),
}

/// Kind of `:LOGBOOK:` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogbookKind {
    /// `- State "X" from "Y" [date]` TODO state change.
    StateChange,
    /// `CLOCK: [start]--[end]` clock-in/clock-out interval.
    Clock,
    /// Anything else (free-form note).
    Note,
}

/// One parsed entry inside a `:LOGBOOK:` drawer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogbookEntry<'a> {
    /// Classification of this entry.
    pub kind: LogbookKind,
    /// New state on a `StateChange` (e.g. "DONE").
    pub new_state: Option<&'a str>,
    /// Previous state on a `StateChange`.
    pub old_state: Option<&'a str>,
    /// Inactive timestamp body (without brackets) attached to the entry.
    pub when: Option<&'a str>,
}

/// Parse the body of a `:LOGBOOK:` drawer into one entry per line.
///
/// Empty lines are skipped. Recognises `- State "X" from "Y" [date]`
/// transitions and `CLOCK:` clock entries; everything else is `Note`.
#[must_use]
pub fn parse_logbook(body: &str) -> Vec<LogbookEntry<'_>> {
    let mut out: Vec<LogbookEntry<'_>> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- State ") {
            let new_state = quoted(rest);
            let old_state = rest
                .split_once("from ")
                .and_then(|(_, after)| quoted(after));
            let when = rest
                .find('[')
                .and_then(|i| rest[i + 1..].find(']').map(|j| &rest[i + 1..i + 1 + j]));
            out.push(LogbookEntry {
                kind: LogbookKind::StateChange,
                new_state,
                old_state,
                when,
            });
        } else if let Some(rest) = trimmed.strip_prefix("CLOCK:") {
            let when = rest
                .find('[')
                .and_then(|i| rest[i + 1..].find(']').map(|j| &rest[i + 1..i + 1 + j]));
            out.push(LogbookEntry {
                kind: LogbookKind::Clock,
                new_state: None,
                old_state: None,
                when,
            });
        } else {
            out.push(LogbookEntry {
                kind: LogbookKind::Note,
                new_state: None,
                old_state: None,
                when: None,
            });
        }
    }
    out
}

fn quoted(s: &str) -> Option<&str> {
    let start = s.find('"')? + 1;
    let after = &s[start..];
    let end = after.find('"')?;
    Some(&after[..end])
}

/// Parse `#+BEGIN_SRC` header arguments into `(key, value)` tuples.
///
/// Keys keep their leading colon; values run from after the key up to
/// the next key (or end of input). Keys without a value are dropped.
/// Example: `:results output :wrap example` →
/// `[(":results", "output"), (":wrap", "example")]`.
#[must_use]
pub fn parse_block_args(args: &str) -> Vec<(&str, &str)> {
    let mut out: Vec<(&str, &str)> = Vec::new();
    let mut tokens = args.split_whitespace().peekable();
    while let Some(key) = tokens.next() {
        if !key.starts_with(':') {
            continue;
        }
        let mut value: Vec<&str> = Vec::new();
        while let Some(&peek) = tokens.peek() {
            if peek.starts_with(':') {
                break;
            }
            tokens.next();
            value.push(peek);
        }
        if value.is_empty() {
            continue;
        }
        let start = value[0].as_ptr() as usize - args.as_ptr() as usize;
        let last = value[value.len() - 1];
        let end = (last.as_ptr() as usize - args.as_ptr() as usize) + last.len();
        out.push((key, &args[start..end]));
    }
    out
}

/// Format a `:PROPERTIES:` drawer from `(key, value)` pairs.
///
/// Pairs are emitted in source order. Returns
/// `":PROPERTIES:\n:K: v\n...\n:END:\n"` ready to splice after a
/// headline's header line. Empty input returns an empty string.
#[must_use]
pub fn format_property_drawer<I, K, V>(entries: I) -> String
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut iter = entries.into_iter().peekable();
    if iter.peek().is_none() {
        return String::new();
    }
    let mut out = String::from(":PROPERTIES:\n");
    for (k, v) in iter {
        out.push(':');
        out.push_str(k.as_ref());
        out.push_str(": ");
        out.push_str(v.as_ref());
        out.push('\n');
    }
    out.push_str(":END:\n");
    out
}

/// Format a link as `[[target][description]]` or `[[target]]`.
/// Counterpart to [`find_links`].
#[must_use]
pub fn format_link(target: &str, description: Option<&str>) -> String {
    description.map_or_else(|| format!("[[{target}]]"), |d| format!("[[{target}][{d}]]"))
}

/// Format a timestamp body (e.g. `"2026-04-25 Sat"`) as an active
/// `<...>` or inactive `[...]` org timestamp.
#[must_use]
pub fn format_timestamp(content: &str, active: bool) -> String {
    if active {
        format!("<{content}>")
    } else {
        format!("[{content}]")
    }
}

/// Anchor target inside text body: `<<NAME>>` or `<<<NAME>>>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorTargetView<'a> {
    /// Target name.
    pub name: &'a str,
    /// True for radio targets (`<<<NAME>>>`).
    pub is_radio: bool,
}

/// Scan `text` for `<<...>>` and `<<<...>>>` anchor targets.
#[must_use]
pub fn find_anchor_targets(text: &str) -> Vec<AnchorTargetView<'_>> {
    let mut out: Vec<AnchorTargetView<'_>> = Vec::new();
    let mut i = 0usize;
    while i < text.len() {
        if text[i..].starts_with("<<<")
            && let Some(end) = text[i + 3..].find(">>>")
        {
            let name = &text[i + 3..i + 3 + end];
            if !name.is_empty() {
                out.push(AnchorTargetView {
                    name,
                    is_radio: true,
                });
            }
            i += 3 + end + 3;
            continue;
        }
        if text[i..].starts_with("<<")
            && let Some(end) = text[i + 2..].find(">>")
        {
            let name = &text[i + 2..i + 2 + end];
            if !name.is_empty() && !name.starts_with('<') {
                out.push(AnchorTargetView {
                    name,
                    is_radio: false,
                });
            }
            i += 2 + end + 2;
            continue;
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// `#+BEGIN_NAME` ... `#+END_NAME` named block (QUOTE, EXAMPLE, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedBlockView<'a> {
    /// Block name (uppercase, without `#+BEGIN_` prefix).
    pub name: &'a str,
    /// Body between begin and end lines (no fence lines).
    pub content: &'a str,
}

/// Scan `text` for `#+BEGIN_NAME` / `#+END_NAME` blocks.
///
/// Matches are case-insensitive on the directive. SRC blocks are *not*
/// returned here (use `as_code_block` on the parsed Node) — only
/// QUOTE, EXAMPLE, VERSE, CENTER, EXPORT, and any other named block.
#[must_use]
pub fn find_named_blocks(text: &str) -> Vec<NamedBlockView<'_>> {
    let mut out: Vec<NamedBlockView<'_>> = Vec::new();
    let mut cursor = 0usize;
    while cursor < text.len() {
        let Some(line_end) = text[cursor..].find('\n') else {
            break;
        };
        let line = &text[cursor..cursor + line_end];
        let after_line = cursor + line_end + 1;
        let upper = line.to_ascii_uppercase();
        if let Some(name) = upper.strip_prefix("#+BEGIN_") {
            if name == "SRC" || name.starts_with("SRC ") {
                cursor = after_line;
                continue;
            }
            let close_marker = format!("#+END_{name}");
            if let Some(end_rel) = text[after_line..].to_ascii_uppercase().find(&close_marker) {
                let body_end = after_line + end_rel;
                let content = &text[after_line..body_end];
                let name_len = name.len();
                let name_in_src = &line[8..8 + name_len];
                out.push(NamedBlockView {
                    name: name_in_src,
                    content,
                });
                let close_line_end = text[body_end..]
                    .find('\n')
                    .map_or(text.len(), |n| body_end + n + 1);
                cursor = close_line_end;
                continue;
            }
        }
        cursor = after_line;
    }
    out
}

/// Group of consecutive list items (any markers, any indent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListGroup<'a> {
    /// Items in source order.
    pub items: Vec<ListItemView<'a>>,
}

/// Group of consecutive `|...|` table rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableView<'a> {
    /// Rows in source order, including separator rows.
    pub rows: Vec<TableRowView<'a>>,
}

/// One row in a [`TableView`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRowView<'a> {
    /// Whether this row is a `|---|---|` separator. Separators have an
    /// empty `cells` vec.
    pub is_separator: bool,
    /// Cell texts (trimmed of leading/trailing whitespace).
    pub cells: Vec<&'a str>,
}

/// Generic Org drawer (`:NAME:` ... `:END:`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawerView<'a> {
    /// Drawer name (without surrounding colons).
    pub name: &'a str,
    /// Drawer body between the opening line and the closing `:END:`,
    /// excluding both lines themselves.
    pub content: &'a str,
}

/// Scan `text` for `:NAME:` ... `:END:` drawers in source order.
/// Drawer names match `[A-Z][A-Z0-9_]*`. Unclosed drawers are skipped.
#[must_use]
pub fn find_drawers(text: &str) -> Vec<DrawerView<'_>> {
    let mut out: Vec<DrawerView<'_>> = Vec::new();
    let mut cursor = 0usize;
    while cursor < text.len() {
        let Some(line_end) = text[cursor..].find('\n') else {
            break;
        };
        let line = &text[cursor..cursor + line_end];
        let trimmed = line.trim_start();
        let after_first = cursor + line_end + 1;
        if let Some(name) = drawer_name(trimmed)
            && let Some(end_rel) = text[after_first..].find(":END:")
        {
            let body_end = after_first + end_rel;
            let content = &text[after_first..body_end];
            out.push(DrawerView { name, content });
            let close_line_end = text[body_end..]
                .find('\n')
                .map_or(text.len(), |n| body_end + n + 1);
            cursor = close_line_end;
            continue;
        }
        cursor = after_first;
    }
    out
}

fn drawer_name(line: &str) -> Option<&str> {
    let inner = line.strip_prefix(':')?.strip_suffix(':')?;
    if inner.is_empty() || inner == "END" {
        return None;
    }
    let bytes = inner.as_bytes();
    let first_ok = bytes[0].is_ascii_uppercase();
    let rest_ok = bytes
        .iter()
        .skip(1)
        .all(|&b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    (first_ok && rest_ok).then_some(inner)
}

/// Org-mode macro invocation `{{{name(arg1,arg2)}}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroView<'a> {
    /// Macro name.
    pub name: &'a str,
    /// Comma-separated argument list (empty when invoked without parens).
    pub args: Vec<&'a str>,
}

/// Scan `text` for `{{{name(args)}}}` macro invocations in source order.
/// Macros without arguments (`{{{name}}}`) yield an empty `args` vec.
#[must_use]
pub fn find_macros(text: &str) -> Vec<MacroView<'_>> {
    let mut out: Vec<MacroView<'_>> = Vec::new();
    let mut i = 0usize;
    while let Some(start) = text[i..].find("{{{") {
        let abs_start = i + start + 3;
        let Some(end_rel) = text[abs_start..].find("}}}") else {
            break;
        };
        let body = &text[abs_start..abs_start + end_rel];
        if let Some(open) = body.find('(')
            && let Some(close) = body.rfind(')')
            && close > open
        {
            let name = &body[..open];
            let args_raw = &body[open + 1..close];
            let args: Vec<&str> = if args_raw.is_empty() {
                Vec::new()
            } else {
                args_raw.split(',').map(str::trim).collect()
            };
            out.push(MacroView { name, args });
        } else if !body.is_empty() && !body.contains(char::is_whitespace) {
            out.push(MacroView {
                name: body,
                args: Vec::new(),
            });
        }
        i = abs_start + end_rel + 3;
    }
    out
}

/// Inline footnote occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FootnoteView<'a> {
    /// Footnote name (the `1` in `[fn:1]` or `foo` in `[fn:foo]`).
    pub name: &'a str,
    /// Inline definition body if present (`[fn:name: body]`).
    pub definition: Option<&'a str>,
}

/// Scan `text` for `[fn:NAME]` references and `[fn:NAME: body]`
/// inline definitions. Anonymous footnotes (`[fn::body]`) are skipped
/// — they do not have a stable name to reference.
#[must_use]
pub fn find_footnotes(text: &str) -> Vec<FootnoteView<'_>> {
    let mut out: Vec<FootnoteView<'_>> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(rest) = text.get(i..)
            && rest.starts_with("[fn:")
            && let Some(end) = rest.find(']')
        {
            let inner = &rest[4..end];
            let (name, def) = inner
                .split_once(':')
                .map_or((inner, None), |(n, d)| (n, Some(d.trim_start_matches(' '))));
            if !name.is_empty() {
                out.push(FootnoteView {
                    name,
                    definition: def,
                });
            }
            i += end + 1;
            continue;
        }
        i += 1;
    }
    out
}

/// Scan `text` for `[N/M]` and `[N%]` progress cookies in source
/// order. Useful for headlines that summarise checkbox lists.
#[must_use]
pub fn find_cookies(text: &str) -> Vec<CookieView> {
    let mut out: Vec<CookieView> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'['
            && let Some(end) = text[i + 1..].find(']')
        {
            let inner = &text[i + 1..i + 1 + end];
            if let Some(stripped) = inner.strip_suffix('%')
                && let Ok(n) = stripped.parse::<u32>()
            {
                out.push(CookieView::Percent(n));
                i += end + 2;
                continue;
            }
            if let Some((d, t)) = inner.split_once('/')
                && let (Ok(done), Ok(total)) = (d.parse::<u32>(), t.parse::<u32>())
            {
                out.push(CookieView::Count { done, total });
                i += end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Borrowed view of an inline link `[[target][description]]` or `[[target]]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkView<'a> {
    /// Link target (URL, id, file path, etc.) before the optional `][`.
    pub target: &'a str,
    /// Optional human-readable description.
    pub description: Option<&'a str>,
}

/// Borrowed view of a timestamp `<YYYY-MM-DD ...>` (active) or
/// `[YYYY-MM-DD ...]` (inactive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimestampView<'a> {
    /// Whether this timestamp is active (angle brackets).
    pub active: bool,
    /// The content between the delimiters (e.g. `2026-05-01 Fri 14:30`).
    pub content: &'a str,
}

/// Scan `text` for `<YYYY-MM-DD ...>` (active) and `[YYYY-MM-DD ...]`
/// (inactive) timestamps. Returns them in source order. Malformed
/// brackets without a leading date are skipped.
#[must_use]
pub fn find_timestamps(text: &str) -> Vec<TimestampView<'_>> {
    let mut out: Vec<TimestampView<'_>> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'<' || b == b'[' {
            let close = if b == b'<' { b'>' } else { b']' };
            if let Some(end_rel) = text[i + 1..].find(close as char) {
                let inner = &text[i + 1..i + 1 + end_rel];
                if is_timestamp_content(inner) {
                    out.push(TimestampView {
                        active: b == b'<',
                        content: inner,
                    });
                    i = i + 1 + end_rel + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

fn is_timestamp_content(s: &str) -> bool {
    // Require leading `YYYY-MM-DD`.
    if s.len() < 10 {
        return false;
    }
    let b = s.as_bytes();
    b[0].is_ascii_digit()
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && b[4] == b'-'
        && b[5].is_ascii_digit()
        && b[6].is_ascii_digit()
        && b[7] == b'-'
        && b[8].is_ascii_digit()
        && b[9].is_ascii_digit()
}

/// Inline markup kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupKind {
    /// `*bold*`
    Bold,
    /// `/italic/`
    Italic,
    /// `=code=`
    Code,
    /// `~verbatim~`
    Verbatim,
    /// `+strike+`
    Strikethrough,
    /// `_under_`
    Underline,
}

/// Borrowed view of an inline markup run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkupView<'a> {
    /// Kind of markup.
    pub kind: MarkupKind,
    /// Content between the markers (markers excluded).
    pub content: &'a str,
}

/// Scan `text` for inline markup runs.
///
/// A run is `MARKER NON-WS ... NON-WS MARKER` where MARKER is one of
/// `*/=~+_` and the neighbouring characters (outside the markers) must
/// be non-alphanumeric, whitespace, or string boundaries so the marker
/// isn't inside a word.
#[must_use]
pub fn find_markup(text: &str) -> Vec<MarkupView<'_>> {
    const MARKERS: &[(u8, MarkupKind)] = &[
        (b'*', MarkupKind::Bold),
        (b'/', MarkupKind::Italic),
        (b'=', MarkupKind::Code),
        (b'~', MarkupKind::Verbatim),
        (b'+', MarkupKind::Strikethrough),
        (b'_', MarkupKind::Underline),
    ];
    let bytes = text.as_bytes();
    let mut out: Vec<MarkupView<'_>> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let Some(&(_, kind)) = MARKERS.iter().find(|(m, _)| *m == b) else {
            i += 1;
            continue;
        };
        // Left boundary: start of string, or preceding char is non-word.
        let left_ok = i == 0 || !is_word_byte(bytes[i - 1]);
        if !left_ok {
            i += 1;
            continue;
        }
        // Require at least one non-ws char after marker.
        if i + 1 >= bytes.len() {
            break;
        }
        let after = bytes[i + 1];
        if after == b' ' || after == b'\t' || after == b'\n' || after == b {
            i += 1;
            continue;
        }
        // Find closing marker on same line.
        let mut j = i + 1;
        let mut found: Option<usize> = None;
        while j < bytes.len() {
            let c = bytes[j];
            if c == b'\n' {
                break;
            }
            if c == b && bytes[j - 1] != b' ' && bytes[j - 1] != b'\t' {
                // Right boundary: next char is non-word (or end).
                let right_ok = j + 1 >= bytes.len() || !is_word_byte(bytes[j + 1]);
                if right_ok {
                    found = Some(j);
                    break;
                }
            }
            j += 1;
        }
        if let Some(end) = found {
            out.push(MarkupView {
                kind,
                content: &text[i + 1..end],
            });
            i = end + 1;
            continue;
        }
        i += 1;
    }
    out
}

const fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Scan `text` for inline `[[target][desc]]` / `[[target]]` links.
/// Returns them in source order. Unterminated `[[` is skipped without
/// panicking (I5).
#[must_use]
pub fn find_links(text: &str) -> Vec<LinkView<'_>> {
    let mut out: Vec<LinkView<'_>> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let after = &text[i + 2..];
            if let Some(end_rel) = after.find("]]") {
                let inner = &after[..end_rel];
                let (target, description) = inner.find("][").map_or((inner, None), |mid| {
                    (&inner[..mid], Some(&inner[mid + 2..]))
                });
                out.push(LinkView {
                    target,
                    description,
                });
                i = i + 2 + end_rel + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

impl Node {
    /// Classification of this node.
    #[must_use]
    pub const fn kind(&self) -> NodeKind {
        self.kind
    }

    /// The verbatim source slice that produced this node.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source[self.span.start..self.span.end]
    }

    /// Structural view when this node is a [`NodeKind::CodeBlock`].
    #[must_use]
    pub fn as_code_block(&self) -> Option<CodeBlockView<'_>> {
        if let NodeMeta::CodeBlock {
            lang_span,
            args_span,
            content_span,
        } = &self.meta
        {
            Some(CodeBlockView {
                language: lang_span.map(|s| &self.source[s.start..s.end]),
                args: args_span.map(|s| &self.source[s.start..s.end]),
                content: &self.source[content_span.start..content_span.end],
            })
        } else {
            None
        }
    }

    /// Structural view when this node is a [`NodeKind::ListItem`].
    #[must_use]
    pub fn as_list_item(&self) -> Option<ListItemView<'_>> {
        if let NodeMeta::ListItem {
            indent,
            marker,
            checkbox,
            content_span,
        } = self.meta
        {
            Some(ListItemView {
                indent,
                marker,
                checkbox,
                content: &self.source[content_span.start..content_span.end],
            })
        } else {
            None
        }
    }
}

/// A parsed headline with its title, level, body, and (post-nesting)
/// children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headline {
    source: Arc<str>,
    header_span: Span,
    title_span: Span,
    todo_span: Option<Span>,
    priority_span: Option<Span>,
    tag_spans: Vec<Span>,
    properties: Option<Properties>,
    level: u8,
    body: Vec<Node>,
    children: Vec<Self>,
}

/// Parsed `:PROPERTIES:` ... `:END:` drawer attached to a [`Headline`].
/// Entry order is preserved from the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Properties {
    source: Arc<str>,
    drawer_span: Span,
    entries: Vec<PropertyEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PropertyEntry {
    key_span: Span,
    value_span: Span,
}

impl Properties {
    /// Lookup a property value by key name (case-sensitive). Returns the
    /// raw value slice without surrounding whitespace.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| &self.source[e.key_span.start..e.key_span.end] == key)
            .map(|e| &self.source[e.value_span.start..e.value_span.end])
    }

    /// True iff `key` is present in the drawer (case-sensitive).
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// True iff `key` is present (case-insensitive).
    #[must_use]
    pub fn contains_key_ignore_case(&self, key: &str) -> bool {
        self.get_ignore_case(key).is_some()
    }

    /// Shorthand for `get("ID")`. Invariant I2 pins stable ULID block IDs
    /// into this property.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.get("ID")
    }

    /// Number of entries in the drawer.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the drawer has zero entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Lookup case-insensitive on the property key.
    #[must_use]
    pub fn get_ignore_case(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| self.source[e.key_span.start..e.key_span.end].eq_ignore_ascii_case(key))
            .map(|e| &self.source[e.value_span.start..e.value_span.end])
    }

    /// Iterate `(key, value)` pairs in source order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(move |e| {
            (
                &self.source[e.key_span.start..e.key_span.end],
                &self.source[e.value_span.start..e.value_span.end],
            )
        })
    }

    /// Iterate keys in source order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .map(move |e| &self.source[e.key_span.start..e.key_span.end])
    }

    /// Iterate values in source order.
    pub fn values(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .map(move |e| &self.source[e.value_span.start..e.value_span.end])
    }
}

impl Headline {
    /// Nesting level (number of leading `*`).
    #[must_use]
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// The title text, excluding leading stars and the separating space,
    /// and the trailing newline.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.source[self.title_span.start..self.title_span.end]
    }

    /// Verbatim source of the header line (including trailing newline if
    /// any).
    #[must_use]
    pub fn header(&self) -> &str {
        &self.source[self.header_span.start..self.header_span.end]
    }

    /// Nodes in this headline's body (after header, before children or
    /// next sibling).
    #[must_use]
    pub fn body(&self) -> &[Node] {
        &self.body
    }

    /// Child headlines (populated once nesting lands).
    #[must_use]
    pub fn children(&self) -> &[Self] {
        &self.children
    }

    /// TODO keyword if present (e.g. `"TODO"`, `"DONE"`).
    #[must_use]
    pub fn todo(&self) -> Option<&str> {
        self.todo_span.map(|s| &self.source[s.start..s.end])
    }

    /// Priority letter if present. For `[#A]` returns `Some('A')`.
    #[must_use]
    pub fn priority(&self) -> Option<char> {
        let span = self.priority_span?;
        let slice = &self.source[span.start..span.end];
        slice.strip_prefix("[#")?.chars().next()
    }

    /// Trailing headline tags in source order.
    #[must_use]
    pub fn tags(&self) -> Vec<&str> {
        self.tag_spans
            .iter()
            .map(|s| &self.source[s.start..s.end])
            .collect()
    }

    /// The headline's `:PROPERTIES:` drawer if one was parsed.
    #[must_use]
    pub const fn properties(&self) -> Option<&Properties> {
        self.properties.as_ref()
    }

    /// True iff this headline starts with the `COMMENT` keyword
    /// (`* COMMENT Foo` form). Comment headlines are excluded from
    /// agenda views and code-block evaluation in Emacs Org.
    #[must_use]
    pub fn is_comment(&self) -> bool {
        let t = self.title();
        t == "COMMENT" || t.starts_with("COMMENT ")
    }

    /// True iff this headline carries the `:ARCHIVE:` tag.
    #[must_use]
    pub fn is_archived(&self) -> bool {
        self.tags().contains(&"ARCHIVE")
    }

    /// Sum of body bytes across this headline's body nodes (does not
    /// recurse into children).
    #[must_use]
    pub fn body_byte_count(&self) -> usize {
        self.body.iter().map(|n| n.span.end - n.span.start).sum()
    }

    /// Verbatim body source as a single contiguous slice. Returns `""`
    /// for headlines with no body.
    #[must_use]
    pub fn body_source(&self) -> &str {
        let Some(first) = self.body.first() else {
            return "";
        };
        let Some(last) = self.body.last() else {
            return "";
        };
        &self.source[first.span.start..last.span.end]
    }

    /// Total descendant headline count (children + grandchildren etc.).
    #[must_use]
    pub fn descendant_count(&self) -> usize {
        self.children.iter().map(|c| 1 + c.descendant_count()).sum()
    }

    /// Maximum depth in this subtree (own level + furthest descendant).
    #[must_use]
    pub fn max_depth(&self) -> u8 {
        self.children
            .iter()
            .map(Self::max_depth)
            .max()
            .unwrap_or(self.level)
            .max(self.level)
    }

    /// Verbatim source of the entire subtree rooted at this headline,
    /// including header, properties drawer, body, and recursive
    /// children.
    #[must_use]
    pub fn subtree_source(&self) -> &str {
        let start = self.header_span.start;
        let end = subtree_end(self);
        &self.source[start..end]
    }

    /// Subtree byte length (including header/drawer/body/children).
    #[must_use]
    pub fn subtree_byte_count(&self) -> usize {
        self.subtree_source().len()
    }

    /// Flatten this subtree (self + every descendant) into a Vec
    /// in depth-first source order.
    #[must_use]
    pub fn flatten(&self) -> Vec<&Self> {
        fn walk<'a>(h: &'a Headline, out: &mut Vec<&'a Headline>) {
            out.push(h);
            for c in h.children() {
                walk(c, out);
            }
        }
        let mut out: Vec<&Self> = Vec::new();
        walk(self, &mut out);
        out
    }

    /// True iff this headline has zero children.
    #[must_use]
    pub const fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    /// Number of ancestor levels above this headline. Roots return 0.
    #[must_use]
    pub const fn ancestor_count(&self) -> usize {
        self.level as usize - 1
    }

    /// Number of immediate children.
    #[must_use]
    pub const fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Iterate immediate child headlines.
    pub fn iter_children(&self) -> impl Iterator<Item = &Self> {
        self.children.iter()
    }

    /// Lookup child at index `idx`.
    #[must_use]
    pub fn child_at(&self, idx: usize) -> Option<&Self> {
        self.children.get(idx)
    }

    /// First child whose title equals `needle` (case-sensitive).
    #[must_use]
    pub fn child_by_title(&self, needle: &str) -> Option<&Self> {
        self.children.iter().find(|c| c.title() == needle)
    }

    /// First child whose `:ID:` property matches `id`.
    #[must_use]
    pub fn child_by_id(&self, id: &str) -> Option<&Self> {
        self.children
            .iter()
            .find(|c| c.properties().and_then(Properties::id) == Some(id))
    }

    /// True iff the headline has a header line that ends with `\n`.
    #[must_use]
    pub fn header_ends_with_newline(&self) -> bool {
        self.header().ends_with('\n')
    }

    /// True iff the headline has a body (any kind).
    #[must_use]
    pub const fn has_body(&self) -> bool {
        !self.body.is_empty()
    }

    /// True iff the headline has a properties drawer.
    #[must_use]
    pub const fn has_properties(&self) -> bool {
        self.properties.is_some()
    }

    /// Number of properties drawer entries (0 if no drawer).
    #[must_use]
    pub fn property_count(&self) -> usize {
        self.properties().map_or(0, Properties::len)
    }

    /// True iff this headline carries an `:ID:` property.
    #[must_use]
    pub fn has_id(&self) -> bool {
        self.properties()
            .is_some_and(|p| p.id().is_some())
    }

    /// `:ID:` property if set.
    #[must_use]
    pub fn id_property(&self) -> Option<&str> {
        self.properties().and_then(Properties::id)
    }

    /// First letter of priority cookie (`'A'` for `[#A]`), if any.
    #[must_use]
    pub fn priority_letter(&self) -> Option<char> {
        self.priority()
    }

    /// First child whose title contains `needle` (case-sensitive).
    #[must_use]
    pub fn child_by_title_substring(&self, needle: &str) -> Option<&Self> {
        self.children.iter().find(|c| c.title().contains(needle))
    }

    /// True iff this headline carries `tag`.
    #[must_use]
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags().contains(&tag)
    }

    /// True iff this headline has any tag at all.
    #[must_use]
    pub const fn has_any_tag(&self) -> bool {
        !self.tag_spans.is_empty()
    }

    /// Number of tags on this headline.
    #[must_use]
    pub const fn tag_count(&self) -> usize {
        self.tag_spans.len()
    }

    /// True iff this headline has a `:KEY:` entry in its drawer.
    #[must_use]
    pub fn has_property(&self, key: &str) -> bool {
        self.properties().is_some_and(|p| p.get(key).is_some())
    }

    /// True iff this headline has a `SCHEDULED:` planning timestamp.
    #[must_use]
    pub fn is_scheduled(&self) -> bool {
        self.planning().is_some_and(|p| p.scheduled.is_some())
    }

    /// True iff this headline has a `DEADLINE:` planning timestamp.
    #[must_use]
    pub fn has_deadline(&self) -> bool {
        self.planning().is_some_and(|p| p.deadline.is_some())
    }

    /// True iff this headline has a `CLOSED:` planning timestamp.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.planning().is_some_and(|p| p.closed.is_some())
    }

    /// True iff this headline has any planning line.
    #[must_use]
    pub fn has_planning(&self) -> bool {
        self.planning().is_some()
    }

    /// Reconstructed display string `[TODO] [#P] Title :tag1:tag2:`.
    /// Useful for UI rendering without going back to source bytes.
    #[must_use]
    pub fn display_title(&self) -> String {
        let mut out = String::new();
        if let Some(t) = self.todo() {
            out.push_str(t);
            out.push(' ');
        }
        if let Some(p) = self.priority() {
            use std::fmt::Write as _;
            let _ = write!(out, "[#{p}] ");
        }
        out.push_str(self.title());
        if !self.tag_spans.is_empty() {
            out.push_str("  :");
            out.push_str(&self.tags().join(":"));
            out.push(':');
        }
        out
    }

    /// All link targets in this headline's title and body (does not
    /// recurse into children). Returns owned strings since
    /// [`find_links`] yields borrowed slices into the header / body
    /// regions rather than the headline source as a whole.
    #[must_use]
    pub fn link_targets(&self) -> Vec<String> {
        let mut out: Vec<String> = find_links(self.title())
            .into_iter()
            .map(|l| l.target.to_owned())
            .collect();
        for n in &self.body {
            for l in find_links(&self.source[n.span.start..n.span.end]) {
                out.push(l.target.to_owned());
            }
        }
        out
    }

    /// Whitespace-separated word count over this headline's body (does
    /// not recurse into children).
    #[must_use]
    pub fn body_word_count(&self) -> usize {
        self.body
            .iter()
            .map(|n| {
                self.source[n.span.start..n.span.end]
                    .split_whitespace()
                    .count()
            })
            .sum()
    }

    /// Number of links in the body (does not recurse).
    #[must_use]
    pub fn body_link_count(&self) -> usize {
        self.body
            .iter()
            .map(|n| find_links(&self.source[n.span.start..n.span.end]).len())
            .sum()
    }

    /// Number of links in the title.
    #[must_use]
    pub fn title_link_count(&self) -> usize {
        find_links(self.title()).len()
    }

    /// Number of timestamps found in title + body.
    #[must_use]
    pub fn timestamp_count(&self) -> usize {
        let mut n = find_timestamps(self.title()).len();
        for node in &self.body {
            n += find_timestamps(&self.source[node.span.start..node.span.end]).len();
        }
        n
    }

    /// Number of progress cookies (`[N/M]` / `[N%]`) in title + body.
    #[must_use]
    pub fn cookie_count(&self) -> usize {
        let mut n = find_cookies(self.title()).len();
        for node in &self.body {
            n += find_cookies(&self.source[node.span.start..node.span.end]).len();
        }
        n
    }

    /// Number of footnote references in title + body.
    #[must_use]
    pub fn footnote_count(&self) -> usize {
        let mut n = find_footnotes(self.title()).len();
        for node in &self.body {
            n += find_footnotes(&self.source[node.span.start..node.span.end]).len();
        }
        n
    }

    /// Number of macro invocations in title + body.
    #[must_use]
    pub fn macro_count(&self) -> usize {
        let mut n = find_macros(self.title()).len();
        for node in &self.body {
            n += find_macros(&self.source[node.span.start..node.span.end]).len();
        }
        n
    }

    /// Total link count across title + body (does not recurse).
    #[must_use]
    pub fn link_count(&self) -> usize {
        self.title_link_count() + self.body_link_count()
    }

    /// Total link count over the entire subtree (self + descendants).
    #[must_use]
    pub fn subtree_link_count(&self) -> usize {
        self.link_count() + self.children.iter().map(Self::subtree_link_count).sum::<usize>()
    }

    /// Body word count over the entire subtree (self + descendants).
    #[must_use]
    pub fn subtree_word_count(&self) -> usize {
        self.body_word_count()
            + self
                .children
                .iter()
                .map(Self::subtree_word_count)
                .sum::<usize>()
    }

    /// Iterate body nodes whose kind matches `kind`.
    pub fn body_nodes_by_kind(&self, kind: NodeKind) -> impl Iterator<Item = &Node> {
        self.body.iter().filter(move |n| n.kind == kind)
    }

    /// Number of body nodes of a given kind.
    #[must_use]
    pub fn body_count_by_kind(&self, kind: NodeKind) -> usize {
        self.body_nodes_by_kind(kind).count()
    }

    /// Number of body paragraph nodes (does not recurse).
    #[must_use]
    pub fn body_paragraph_count(&self) -> usize {
        self.body_count_by_kind(NodeKind::Paragraph)
    }

    /// Number of body keyword (`#+KEY:`) nodes.
    #[must_use]
    pub fn body_keyword_count(&self) -> usize {
        self.body_count_by_kind(NodeKind::Keyword)
    }

    /// Number of blank lines in the body.
    #[must_use]
    pub fn body_blank_line_count(&self) -> usize {
        self.body_count_by_kind(NodeKind::BlankLine)
    }

    /// Number of children that have a TODO keyword.
    #[must_use]
    pub fn child_todo_count(&self) -> usize {
        self.children.iter().filter(|c| c.todo().is_some()).count()
    }

    /// Number of children that are leaves.
    #[must_use]
    pub fn child_leaf_count(&self) -> usize {
        self.children.iter().filter(|c| c.is_leaf()).count()
    }

    /// Iterate immediate children that have a TODO keyword.
    pub fn iter_child_todos(&self) -> impl Iterator<Item = &Self> {
        self.children.iter().filter(|c| c.todo().is_some())
    }

    /// Iterate immediate children with a given tag.
    pub fn iter_child_with_tag<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a Self> {
        self.children.iter().filter(move |c| c.has_tag(tag))
    }

    /// Iterate immediate children that are leaves.
    pub fn iter_child_leaves(&self) -> impl Iterator<Item = &Self> {
        self.children.iter().filter(|c| c.is_leaf())
    }

    /// First child whose TODO keyword equals `keyword`.
    #[must_use]
    pub fn child_by_todo(&self, keyword: &str) -> Option<&Self> {
        self.children.iter().find(|c| c.todo() == Some(keyword))
    }

    /// First child carrying `tag`.
    #[must_use]
    pub fn child_by_tag(&self, tag: &str) -> Option<&Self> {
        self.children.iter().find(|c| c.has_tag(tag))
    }

    /// Direct child whose priority letter equals `letter`.
    #[must_use]
    pub fn child_by_priority(&self, letter: char) -> Option<&Self> {
        self.children.iter().find(|c| c.priority() == Some(letter))
    }

    /// True iff any descendant carries `tag`.
    #[must_use]
    pub fn descendant_has_tag(&self, tag: &str) -> bool {
        self.children
            .iter()
            .any(|c| c.has_tag(tag) || c.descendant_has_tag(tag))
    }

    /// True iff any descendant has TODO `keyword`.
    #[must_use]
    pub fn descendant_has_todo(&self, keyword: &str) -> bool {
        self.children.iter().any(|c| {
            c.todo() == Some(keyword) || c.descendant_has_todo(keyword)
        })
    }

    /// First descendant carrying `tag` (depth-first).
    #[must_use]
    pub fn descendant_by_tag(&self, tag: &str) -> Option<&Self> {
        for c in &self.children {
            if c.has_tag(tag) {
                return Some(c);
            }
            if let Some(d) = c.descendant_by_tag(tag) {
                return Some(d);
            }
        }
        None
    }

    /// First descendant whose title equals `needle` (depth-first).
    #[must_use]
    pub fn descendant_by_title(&self, needle: &str) -> Option<&Self> {
        for c in &self.children {
            if c.title() == needle {
                return Some(c);
            }
            if let Some(d) = c.descendant_by_title(needle) {
                return Some(d);
            }
        }
        None
    }

    /// First descendant whose `:ID:` property equals `id`.
    #[must_use]
    pub fn descendant_by_id(&self, id: &str) -> Option<&Self> {
        for c in &self.children {
            if c.id_property() == Some(id) {
                return Some(c);
            }
            if let Some(d) = c.descendant_by_id(id) {
                return Some(d);
            }
        }
        None
    }

    /// True iff any descendant carries an `:ID:` property.
    #[must_use]
    pub fn descendant_has_id(&self) -> bool {
        self.children
            .iter()
            .any(|c| c.has_id() || c.descendant_has_id())
    }

    /// True iff any descendant has property `key`.
    #[must_use]
    pub fn descendant_has_property(&self, key: &str) -> bool {
        self.children
            .iter()
            .any(|c| c.has_property(key) || c.descendant_has_property(key))
    }

    /// True iff any descendant has a planning line.
    #[must_use]
    pub fn descendant_has_planning(&self) -> bool {
        self.children
            .iter()
            .any(|c| c.has_planning() || c.descendant_has_planning())
    }

    /// Count of descendants with TODO `keyword`.
    #[must_use]
    pub fn count_descendant_todos(&self, keyword: &str) -> usize {
        self.children
            .iter()
            .map(|c| {
                let me = usize::from(c.todo() == Some(keyword));
                me + c.count_descendant_todos(keyword)
            })
            .sum()
    }

    /// Count of descendants carrying `tag`.
    #[must_use]
    pub fn count_descendant_tagged(&self, tag: &str) -> usize {
        self.children
            .iter()
            .map(|c| usize::from(c.has_tag(tag)) + c.count_descendant_tagged(tag))
            .sum()
    }

    /// Number of comment lines in the body.
    #[must_use]
    pub fn body_comment_count(&self) -> usize {
        self.body_count_by_kind(NodeKind::Comment)
    }

    /// Number of code blocks in the body (does not recurse).
    #[must_use]
    pub fn body_code_block_count(&self) -> usize {
        self.body_count_by_kind(NodeKind::CodeBlock)
    }

    /// Number of list items in the body (does not recurse).
    #[must_use]
    pub fn body_list_item_count(&self) -> usize {
        self.body_count_by_kind(NodeKind::ListItem)
    }

    /// Number of table rows in the body (does not recurse).
    #[must_use]
    pub fn body_table_row_count(&self) -> usize {
        self.body_count_by_kind(NodeKind::TableRow)
    }

    /// Iterate every body line as a borrowed string slice.
    pub fn body_lines(&self) -> impl Iterator<Item = &str> {
        self.body.iter().flat_map(|n| {
            let s = &self.source[n.span.start..n.span.end];
            s.lines()
        })
    }

    /// Parse the planning line (`SCHEDULED:` / `DEADLINE:` / `CLOSED:`)
    /// that immediately follows the header line, if any. Returns `None`
    /// when no planning line is present.
    #[must_use]
    pub fn planning(&self) -> Option<PlanningView<'_>> {
        let after = self.header_span.end;
        let rest = &self.source[after..];
        let line_end = rest.find('\n').map_or(self.source.len(), |n| after + n);
        let line = &self.source[after..line_end];
        if !line.trim_start().starts_with("SCHEDULED:")
            && !line.trim_start().starts_with("DEADLINE:")
            && !line.trim_start().starts_with("CLOSED:")
        {
            return None;
        }
        Some(PlanningView {
            scheduled: extract_keyword_ts(line, "SCHEDULED:"),
            deadline: extract_keyword_ts(line, "DEADLINE:"),
            closed: extract_keyword_ts(line, "CLOSED:"),
        })
    }
}

/// Parsed planning line on a headline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanningView<'a> {
    /// `SCHEDULED:` timestamp (verbatim, including brackets) if present.
    pub scheduled: Option<&'a str>,
    /// `DEADLINE:` timestamp if present.
    pub deadline: Option<&'a str>,
    /// `CLOSED:` timestamp if present.
    pub closed: Option<&'a str>,
}

fn extract_keyword_ts<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let idx = line.find(keyword)?;
    let after = &line[idx + keyword.len()..];
    let after = after.trim_start();
    let (open, close) = if after.starts_with('<') {
        ('<', '>')
    } else if after.starts_with('[') {
        ('[', ']')
    } else {
        return None;
    };
    let close_idx = after.find(close)?;
    let _ = open;
    Some(&after[..=close_idx])
}

/// Failure mode while parsing an org document.
///
/// The current parser is total — any byte sequence is valid org from this
/// crate's point of view — but the `Result` return keeps the public
/// signature stable as richer parsing lands.
#[derive(Debug, Error)]
pub enum ParseError {}

/// Read and parse an org file from disk in one call.
#[allow(clippy::missing_errors_doc)]
pub fn parse_path(path: &std::path::Path) -> Result<OrgDoc, ParseFromPathError> {
    let src =
        std::fs::read_to_string(path).map_err(|e| ParseFromPathError::Io(e.to_string()))?;
    parse(&src).map_err(|_| ParseFromPathError::Parse)
}

/// Failure mode for [`parse_path`].
#[derive(Debug, Error)]
pub enum ParseFromPathError {
    /// Filesystem error.
    #[error("io: {0}")]
    Io(String),
    /// Parser rejected the source.
    #[error("parse failed")]
    Parse,
}

/// Parse an org document. See [`OrgDoc`] for the tree shape.
#[allow(clippy::must_use_candidate, clippy::too_many_lines)]
pub fn parse(src: &str) -> Result<OrgDoc, ParseError> {
    let source: Arc<str> = Arc::from(src);
    let mut preamble: Vec<Node> = Vec::new();
    let mut roots: Vec<Headline> = Vec::new();
    let mut paragraph: Option<Span> = None;

    let lines: Vec<(&str, Span)> = {
        let mut v = Vec::new();
        let mut cursor = 0usize;
        for line in src.split_inclusive('\n') {
            let s = Span {
                start: cursor,
                end: cursor + line.len(),
            };
            cursor += line.len();
            v.push((line, s));
        }
        v
    };

    let mut i = 0;
    while i < lines.len() {
        let (line, span) = lines[i];

        if let Some(head) = classify_heading(line, span) {
            flush_paragraph(
                &source,
                &mut paragraph,
                target_nodes(&mut roots, &mut preamble),
            );
            roots.push(Headline {
                source: Arc::clone(&source),
                header_span: head.header_span,
                title_span: head.title_span,
                todo_span: head.todo_span,
                priority_span: head.priority_span,
                tag_spans: head.tag_spans,
                properties: None,
                level: head.level,
                body: Vec::new(),
                children: Vec::new(),
            });
            i += 1;
            continue;
        }

        // Property drawer: immediately after a heading (body empty, no
        // drawer yet), a `:PROPERTIES:` / `:END:` block with `:KEY: value`
        // entries between is captured structurally and excluded from the
        // headline's body.
        if paragraph.is_none()
            && let Some(h) = roots.last()
            && h.body.is_empty()
            && h.properties.is_none()
            && trimmed_line(line) == ":PROPERTIES:"
            && let Some((drawer, next_i)) = scan_property_drawer(&lines, i, &source)
        {
            if let Some(h_mut) = roots.last_mut() {
                h_mut.properties = Some(drawer);
            }
            i = next_i;
            continue;
        }

        // Code block: `#+BEGIN_SRC [lang [args...]]` ... `#+END_SRC`. Case
        // insensitive on the directive. Content between is verbatim and
        // never classified as heading/drawer/etc. Unclosed blocks fall
        // through to the normal line classifier.
        if let Some((node, next_i)) = scan_code_block(&lines, i, &source) {
            flush_paragraph(
                &source,
                &mut paragraph,
                target_nodes(&mut roots, &mut preamble),
            );
            push_node(&mut roots, &mut preamble, node);
            i = next_i;
            continue;
        }

        if let Some((kind, meta)) = classify_special_line(line, span) {
            flush_paragraph(
                &source,
                &mut paragraph,
                target_nodes(&mut roots, &mut preamble),
            );
            push_node(
                &mut roots,
                &mut preamble,
                Node {
                    source: Arc::clone(&source),
                    kind,
                    span,
                    meta,
                },
            );
            i += 1;
            continue;
        }

        match classify_line(line) {
            LineKind::Paragraph => match &mut paragraph {
                Some(p) => p.end = span.end,
                None => paragraph = Some(span),
            },
            LineKind::Blank => {
                flush_paragraph(
                    &source,
                    &mut paragraph,
                    target_nodes(&mut roots, &mut preamble),
                );
                push_node(
                    &mut roots,
                    &mut preamble,
                    Node {
                        source: Arc::clone(&source),
                        kind: NodeKind::BlankLine,
                        span,
                        meta: NodeMeta::None,
                    },
                );
            }
            LineKind::Comment => {
                flush_paragraph(
                    &source,
                    &mut paragraph,
                    target_nodes(&mut roots, &mut preamble),
                );
                push_node(
                    &mut roots,
                    &mut preamble,
                    Node {
                        source: Arc::clone(&source),
                        kind: NodeKind::Comment,
                        span,
                        meta: NodeMeta::None,
                    },
                );
            }
            LineKind::Keyword => {
                flush_paragraph(
                    &source,
                    &mut paragraph,
                    target_nodes(&mut roots, &mut preamble),
                );
                push_node(
                    &mut roots,
                    &mut preamble,
                    Node {
                        source: Arc::clone(&source),
                        kind: NodeKind::Keyword,
                        span,
                        meta: NodeMeta::None,
                    },
                );
            }
        }
        i += 1;
    }
    flush_paragraph(
        &source,
        &mut paragraph,
        target_nodes(&mut roots, &mut preamble),
    );

    Ok(OrgDoc {
        source,
        preamble,
        roots: nest(roots),
    })
}

fn trimmed_line(line: &str) -> &str {
    line.strip_suffix('\n').unwrap_or(line).trim()
}

/// Scan `lines[start..]` for a well-formed `:PROPERTIES:` / `:END:`
/// drawer. Returns the drawer and the index of the line after `:END:`,
/// or `None` if the structure isn't a valid drawer.
fn scan_property_drawer(
    lines: &[(&str, Span)],
    start: usize,
    source: &Arc<str>,
) -> Option<(Properties, usize)> {
    let (_, start_span) = lines[start];
    let mut entries: Vec<PropertyEntry> = Vec::new();
    let mut j = start + 1;
    while j < lines.len() {
        let (ln, sp) = lines[j];
        let trim = trimmed_line(ln);
        if trim == ":END:" {
            let drawer_span = Span {
                start: start_span.start,
                end: sp.end,
            };
            return Some((
                Properties {
                    source: Arc::clone(source),
                    drawer_span,
                    entries,
                },
                j + 1,
            ));
        }
        if let Some(entry) = parse_property_entry(ln, sp) {
            entries.push(entry);
            j += 1;
            continue;
        }
        return None; // malformed
    }
    None
}

fn classify_special_line(line: &str, span: Span) -> Option<(NodeKind, NodeMeta)> {
    if let Some(meta) = classify_list_item(line, span) {
        return Some((NodeKind::ListItem, meta));
    }
    if classify_table_row(line) {
        return Some((NodeKind::TableRow, NodeMeta::None));
    }
    None
}

fn classify_list_item(line: &str, span: Span) -> Option<NodeMeta> {
    let body = line.strip_suffix('\n').unwrap_or(line);
    let indent = body.len() - body.trim_start_matches([' ', '\t']).len();
    let rest = &body[indent..];
    if rest.is_empty() {
        return None;
    }
    let bytes = rest.as_bytes();

    // Determine marker and its consumed length.
    let (marker, marker_len) =
        if bytes[0] == b'-' && bytes.get(1).is_some_and(|b| *b == b' ' || *b == b'\t') {
            (ListMarker::Dash, 1)
        } else if bytes[0] == b'+' && bytes.get(1).is_some_and(|b| *b == b' ' || *b == b'\t') {
            (ListMarker::Plus, 1)
        } else if bytes[0].is_ascii_digit() {
            // Ordered: digits followed by '.' or ')'.
            let mut j = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j >= bytes.len() {
                return None;
            }
            let delim = bytes[j];
            if delim != b'.' && delim != b')' {
                return None;
            }
            let after_delim = bytes.get(j + 1);
            if !matches!(after_delim, Some(b' ' | b'\t')) {
                return None;
            }
            let m = if delim == b'.' {
                ListMarker::OrderedDot
            } else {
                ListMarker::OrderedParen
            };
            (m, j + 1)
        } else {
            return None;
        };

    let after_marker = &rest[marker_len..];
    let ws = after_marker.len() - after_marker.trim_start_matches([' ', '\t']).len();
    let after_ws = &after_marker[ws..];

    // Optional checkbox.
    let (checkbox, after_cb_len) = if after_ws.len() >= 3
        && &after_ws[..1] == "["
        && &after_ws[2..3] == "]"
    {
        let mark = after_ws.as_bytes()[1];
        let cb = match mark {
            b' ' => Some(Checkbox::Unchecked),
            b'X' | b'x' => Some(Checkbox::Checked),
            b'-' => Some(Checkbox::Partial),
            _ => None,
        };
        match cb {
            Some(_) => {
                let after_box = &after_ws[3..];
                if after_box.is_empty() || after_box.starts_with(' ') || after_box.starts_with('\t')
                {
                    let skip_ws = after_box.len() - after_box.trim_start_matches([' ', '\t']).len();
                    (cb, 3 + skip_ws)
                } else {
                    (None, 0)
                }
            }
            None => (None, 0),
        }
    } else {
        (None, 0)
    };

    let content_rel_start = indent + marker_len + ws + after_cb_len;
    let content_end = indent + rest.len();
    let content_span = Span {
        start: span.start + content_rel_start,
        end: span.start + content_end,
    };

    Some(NodeMeta::ListItem {
        indent,
        marker,
        checkbox,
        content_span,
    })
}

fn classify_table_row(line: &str) -> bool {
    let body = line.strip_suffix('\n').unwrap_or(line);
    let trimmed = body.trim_start_matches([' ', '\t']);
    trimmed.starts_with('|')
}

fn scan_code_block(
    lines: &[(&str, Span)],
    start: usize,
    source: &Arc<str>,
) -> Option<(Node, usize)> {
    let (line, span) = lines[start];
    let body = line.strip_suffix('\n').unwrap_or(line);
    // Directive must start at column 0.
    let rest = body.strip_prefix("#+")?;
    // Match `begin_src` case-insensitive followed by space/EOL/colon.
    let directive_len = 9; // "begin_src" or "BEGIN_SRC"
    if rest.len() < directive_len {
        return None;
    }
    if !rest[..directive_len].eq_ignore_ascii_case("begin_src") {
        return None;
    }
    let after = &rest[directive_len..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }

    // Parse `lang` and `args` from header.
    let header_rest = after.trim_start_matches([' ', '\t']);
    let header_offset = (body.len() - header_rest.len()) + span.start;
    let (lang_span, args_span) = if header_rest.is_empty() {
        (None, None)
    } else {
        let lang_end = header_rest.find([' ', '\t']).unwrap_or(header_rest.len());
        let lang = &header_rest[..lang_end];
        let lang_span = if lang.is_empty() {
            None
        } else {
            Some(Span {
                start: header_offset,
                end: header_offset + lang.len(),
            })
        };
        let after_lang = &header_rest[lang_end..];
        let args_trim = after_lang.trim_start_matches([' ', '\t']);
        let args_offset = header_offset + (header_rest.len() - args_trim.len());
        let args_trimmed = args_trim.trim_end_matches([' ', '\t']);
        let args_span = if args_trimmed.is_empty() {
            None
        } else {
            Some(Span {
                start: args_offset,
                end: args_offset + args_trimmed.len(),
            })
        };
        (lang_span, args_span)
    };

    // Scan forward for `#+END_SRC` at column 0.
    let content_start = span.end;
    let mut j = start + 1;
    while j < lines.len() {
        let (ln, _) = lines[j];
        let ln_body = ln.strip_suffix('\n').unwrap_or(ln);
        if let Some(rest) = ln_body.strip_prefix("#+")
            && rest.eq_ignore_ascii_case("end_src")
        {
            let (_, end_span) = lines[j];
            let node_span = Span {
                start: span.start,
                end: end_span.end,
            };
            let content_span = Span {
                start: content_start,
                end: end_span.start,
            };
            return Some((
                Node {
                    source: Arc::clone(source),
                    kind: NodeKind::CodeBlock,
                    span: node_span,
                    meta: NodeMeta::CodeBlock {
                        lang_span,
                        args_span,
                        content_span,
                    },
                },
                j + 1,
            ));
        }
        j += 1;
    }
    None
}

fn parse_property_entry(line: &str, span: Span) -> Option<PropertyEntry> {
    let body = line.strip_suffix('\n').unwrap_or(line);
    let rest = body.strip_prefix(':')?;
    let colon_pos = rest.find(':')?;
    let key_str = &rest[..colon_pos];
    if key_str.is_empty()
        || !key_str
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let key_span = Span {
        start: span.start + 1,
        end: span.start + 1 + colon_pos,
    };
    let after_colon_pos = 1 + colon_pos + 1;
    let after_colon = &body[after_colon_pos..];
    let leading_ws = after_colon.len() - after_colon.trim_start_matches([' ', '\t']).len();
    let value_content = after_colon.trim_matches([' ', '\t']);
    let value_span = Span {
        start: span.start + after_colon_pos + leading_ws,
        end: span.start + after_colon_pos + leading_ws + value_content.len(),
    };
    Some(PropertyEntry {
        key_span,
        value_span,
    })
}

/// Restructure a flat list of headlines into a tree by level. A heading
/// with level N becomes a child of the nearest preceding heading with
/// level < N; otherwise it is a root. Level jumps (e.g. 1 → 3) are
/// accepted and the deeper heading is attached to the nearest
/// lower-level ancestor.
fn nest(flat: Vec<Headline>) -> Vec<Headline> {
    let mut roots: Vec<Headline> = Vec::new();
    for h in flat {
        attach(&mut roots, h);
    }
    roots
}

fn attach(siblings: &mut Vec<Headline>, h: Headline) {
    match siblings.last_mut() {
        Some(last) if last.level < h.level => attach(&mut last.children, h),
        _ => siblings.push(h),
    }
}

/// Serialise an org document back to its source text. Concatenates each
/// node's source slice, which guarantees I1 (byte-exact roundtrip) for
/// unedited documents by construction.
#[must_use]
pub fn print(doc: &OrgDoc) -> String {
    let mut out = String::with_capacity(doc.source.len());
    for n in &doc.preamble {
        out.push_str(doc.source_of(n.span));
    }
    for h in &doc.roots {
        print_headline(doc, h, &mut out);
    }
    out
}

fn print_headline(doc: &OrgDoc, h: &Headline, out: &mut String) {
    out.push_str(doc.source_of(h.header_span));
    if let Some(p) = &h.properties {
        out.push_str(doc.source_of(p.drawer_span));
    }
    for n in &h.body {
        out.push_str(doc.source_of(n.span));
    }
    for c in &h.children {
        print_headline(doc, c, out);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Blank,
    Comment,
    Keyword,
    Paragraph,
}

struct HeadInfo {
    header_span: Span,
    title_span: Span,
    todo_span: Option<Span>,
    priority_span: Option<Span>,
    tag_spans: Vec<Span>,
    level: u8,
}

const TODO_KEYWORDS: &[&str] = &["TODO", "DONE"];

#[allow(clippy::ptr_arg)]
fn target_nodes<'a>(
    roots: &'a mut Vec<Headline>,
    preamble: &'a mut Vec<Node>,
) -> &'a mut Vec<Node> {
    if let Some(last) = roots.last_mut() {
        &mut last.body
    } else {
        preamble
    }
}

#[allow(clippy::ptr_arg)]
fn push_node(roots: &mut Vec<Headline>, preamble: &mut Vec<Node>, node: Node) {
    target_nodes(roots, preamble).push(node);
}

fn flush_paragraph(source: &Arc<str>, paragraph: &mut Option<Span>, out: &mut Vec<Node>) {
    if let Some(span) = paragraph.take() {
        out.push(Node {
            source: Arc::clone(source),
            kind: NodeKind::Paragraph,
            span,
            meta: NodeMeta::None,
        });
    }
}

/// Recognise a heading line. A heading is `*+` at column 0 followed by
/// either end-of-line (empty title) or at least one space/tab (title
/// follows). If a title is present, the content between the stars and
/// the end of the line (before the newline) is further split into:
/// optional leading TODO keyword, optional `[#X]` priority, title, and
/// optional trailing `:tag:tag:` list.
fn classify_heading(line: &str, span: Span) -> Option<HeadInfo> {
    let body = line.strip_suffix('\n').unwrap_or(line);
    let stars = body.chars().take_while(|&c| c == '*').count();
    if stars == 0 {
        return None;
    }
    let after = &body[stars..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    let level = u8::try_from(stars).unwrap_or(u8::MAX);

    let ws_skip = after
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    // Content region: between the stars+space and the end of body.
    let content_start = span.start + stars + ws_skip;
    let content_end = span.start + stars + after.len();
    let content = &body[stars + ws_skip..stars + after.len()];

    // Strip trailing tags first (right-to-left).
    let (content_before_tags_end, tag_spans) =
        strip_trailing_tags(content, content_start, content_end);

    // After trimming trailing whitespace before tags, the TODO+priority+title
    // live in [content_start, title_trim_end).
    let pre_tags = &content[..(content_before_tags_end - content_start)];
    let title_trim_end = content_start + pre_tags.trim_end_matches([' ', '\t']).len();
    let working = &content[..(title_trim_end - content_start)];

    // Parse TODO keyword from left.
    let (todo_span, after_todo_str, after_todo_pos) = strip_leading_todo(working, content_start);

    // Parse priority `[#X]` from left.
    let (priority_span, after_prio_str, after_prio_pos) =
        strip_leading_priority(after_todo_str, after_todo_pos);

    // Remainder is the title; trim leading whitespace.
    let title_leading_ws =
        after_prio_str.len() - after_prio_str.trim_start_matches([' ', '\t']).len();
    let title_start = after_prio_pos + title_leading_ws;
    let title_end = title_trim_end;
    let title_span = Span {
        start: title_start,
        end: title_end,
    };

    Some(HeadInfo {
        header_span: span,
        title_span,
        todo_span,
        priority_span,
        tag_spans,
        level,
    })
}

/// Right-to-left strip a trailing `:tag:tag:` list from the content line.
/// Returns `(end_position_before_tags, tag_spans)` where `tag_spans`
/// cover the individual tag names (not the colons) in source order. The
/// tag list requires at least one whitespace character before its
/// opening `:` for validity.
fn strip_trailing_tags(
    content: &str,
    content_start: usize,
    content_end: usize,
) -> (usize, Vec<Span>) {
    let trimmed = content.trim_end_matches([' ', '\t']);
    if !trimmed.ends_with(':') || trimmed.len() < 2 {
        return (content_end, Vec::new());
    }
    let bytes = trimmed.as_bytes();
    let mut cursor = trimmed.len() - 1; // position of trailing ':'
    let mut tags_rev: Vec<(usize, usize)> = Vec::new();

    loop {
        // cursor points at a ':'. Walk leftward over tag-chars for the name.
        let name_end = cursor;
        let mut name_start = cursor;
        while name_start > 0 && is_tag_char(bytes[name_start - 1] as char) {
            name_start -= 1;
        }
        if name_start == name_end {
            // Empty name: the ':' at cursor has no tag chars before it.
            // Either the tag list is degenerate (abort and treat as title)
            // or we have already collected tags and hit the structure
            // boundary. If nothing collected, abort.
            if tags_rev.is_empty() {
                return (content_end, Vec::new());
            }
            break;
        }
        tags_rev.push((name_start, name_end));

        if name_start == 0 {
            // Tag list starts at content start — no preceding whitespace,
            // so this isn't a well-formed trailing tag block.
            return (content_end, Vec::new());
        }
        match bytes[name_start - 1] {
            b':' => {
                cursor = name_start - 1;
            }
            b' ' | b'\t' => break,
            _ => return (content_end, Vec::new()),
        }
    }

    if tags_rev.is_empty() {
        return (content_end, Vec::new());
    }

    // Earliest name_start is the last element of tags_rev.
    let first_name_start = tags_rev.last().map_or(content.len(), |t| t.0);
    // The opening ':' of the tag list sits at first_name_start - 1.
    let first_colon_pos = first_name_start.saturating_sub(1);
    let before_tags_trim = content[..first_colon_pos]
        .trim_end_matches([' ', '\t'])
        .len();

    tags_rev.reverse();
    let tag_spans = tags_rev
        .into_iter()
        .map(|(s, e)| Span {
            start: content_start + s,
            end: content_start + e,
        })
        .collect();
    (content_start + before_tags_trim, tag_spans)
}

const fn is_tag_char(c: char) -> bool {
    matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '@' | '#' | '%')
}

/// Try to strip a leading TODO keyword. Returns the matched span (if any)
/// plus the remaining substring and its absolute start position.
fn strip_leading_todo(working: &str, base: usize) -> (Option<Span>, &str, usize) {
    for &kw in TODO_KEYWORDS {
        if let Some(rest) = working.strip_prefix(kw)
            && (rest.is_empty() || rest.starts_with([' ', '\t']))
        {
            return (
                Some(Span {
                    start: base,
                    end: base + kw.len(),
                }),
                rest,
                base + kw.len(),
            );
        }
    }
    (None, working, base)
}

/// Try to strip a leading priority `[#X]` (after optional whitespace).
fn strip_leading_priority(working: &str, base: usize) -> (Option<Span>, &str, usize) {
    let ws = working.len() - working.trim_start_matches([' ', '\t']).len();
    let after_ws = &working[ws..];
    let prio_start = base + ws;
    if after_ws.len() >= 4 && after_ws.starts_with("[#") && after_ws.as_bytes()[3] == b']' {
        let inside = after_ws.as_bytes()[2];
        if inside.is_ascii_alphabetic() {
            return (
                Some(Span {
                    start: prio_start,
                    end: prio_start + 4,
                }),
                &after_ws[4..],
                prio_start + 4,
            );
        }
    }
    (None, working, base)
}

fn classify_line(line: &str) -> LineKind {
    let body = line.strip_suffix('\n').unwrap_or(line);

    if body.trim().is_empty() {
        return LineKind::Blank;
    }
    if let Some(rest) = body.strip_prefix("#+")
        && is_keyword_header(rest)
    {
        return LineKind::Keyword;
    }
    if let Some(rest) = body.strip_prefix('#')
        && (rest.is_empty() || rest.starts_with([' ', '\t']))
    {
        return LineKind::Comment;
    }
    LineKind::Paragraph
}

/// Recognise the body of a `#+KEY:` header (the text following `#+`).
fn is_keyword_header(rest: &str) -> bool {
    let mut chars = rest.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    for ch in chars {
        if ch == ':' {
            return true;
        }
        if !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-') {
            return false;
        }
    }
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn roundtrip(src: &str) {
        let parsed = parse(src).expect("parse is infallible");
        assert_eq!(print(&parsed), src, "roundtrip mismatch for {src:?}");
    }

    #[test]
    fn empty_input_parses_to_empty_doc() {
        let doc = parse("").expect("parse");
        assert!(doc.preamble().is_empty());
        assert!(doc.roots().is_empty());
        roundtrip("");
    }

    #[test]
    fn single_newline_is_one_blank_line() {
        let doc = parse("\n").expect("parse");
        assert_eq!(doc.preamble().len(), 1);
        assert_eq!(doc.preamble()[0].kind(), NodeKind::BlankLine);
        roundtrip("\n");
    }

    #[test]
    fn paragraph_coalesces_adjacent_lines() {
        let src = "a\nb\nc\n";
        let doc = parse(src).expect("parse");
        assert_eq!(doc.preamble().len(), 1);
        assert_eq!(doc.preamble()[0].kind(), NodeKind::Paragraph);
        roundtrip(src);
    }

    #[test]
    fn blank_between_paragraphs_splits_them() {
        let doc = parse("a\n\nb\n").expect("parse");
        assert_eq!(doc.preamble().len(), 3);
        assert_eq!(doc.preamble()[0].kind(), NodeKind::Paragraph);
        assert_eq!(doc.preamble()[1].kind(), NodeKind::BlankLine);
        assert_eq!(doc.preamble()[2].kind(), NodeKind::Paragraph);
    }

    #[test]
    fn comment_requires_column_zero() {
        assert_eq!(classify_line("# hi\n"), LineKind::Comment);
        assert_eq!(classify_line("#\n"), LineKind::Comment);
        assert_eq!(classify_line("#\ttab-comment\n"), LineKind::Comment);
        assert_eq!(classify_line("  # hi\n"), LineKind::Paragraph);
        assert_eq!(classify_line("#foo\n"), LineKind::Paragraph);
    }

    #[test]
    fn keyword_requires_column_zero_and_colon() {
        assert_eq!(classify_line("#+TITLE: x\n"), LineKind::Keyword);
        assert_eq!(classify_line("#+FILETAGS: :t:\n"), LineKind::Keyword);
        assert_eq!(classify_line("#+AUTHOR:\n"), LineKind::Keyword);
        assert_eq!(classify_line("#+TITLE x\n"), LineKind::Paragraph);
        assert_eq!(classify_line("  #+TITLE: x\n"), LineKind::Paragraph);
    }

    #[test]
    fn file_without_trailing_newline_roundtrips() {
        roundtrip("no trailing newline");
        roundtrip("#+TITLE: x");
        roundtrip("");
    }

    #[test]
    fn heading_basic_roundtrips() {
        roundtrip("* Hello\n");
        roundtrip("* First\n* Second\n* Third\n");
        roundtrip("*\n");
        roundtrip("**\n");
    }

    #[test]
    fn heading_with_body_roundtrips() {
        roundtrip("* Hello\nbody\n");
    }
}
