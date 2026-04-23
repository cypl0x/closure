# closure — vision-scope spec

_Status: draft. Freezes at end of each milestone; see `phases.md`._

closure is a local-first, offline-first, plain-text PKM / notebook / wiki system. This document is the system contract: every subsystem, its hook, and the invariants it must preserve. Delivery is phased (`phases.md`); the architecture is not.

If a feature doesn't fit the invariants below, the feature is wrong — not the invariants. If the invariants don't fit the feature, the spec is explicitly revised in the same commit. No silent drift.

## Invariants

Each invariant is enforced by at least one automated check.

### I1 — Byte-exact roundtrip on the golden corpus

For every file `s` under `fixtures/` for its format:

```
format::print(format::parse(s)?) == s
```

Byte-for-byte. No normalisation, no whitespace coalescing. Applies to every
first-class format (`closure-org`, future `closure-markdown`, …). If a file
cannot roundtrip it is either a parser bug to fix or the corpus entry is
revised with the reason documented in the same commit.

### I2 — Stable block IDs survive parse/print and CRDT merges

Every headline carries a property-drawer entry `:ID: <ULID>`. On parse,
existing IDs are preserved verbatim. On print, IDs are emitted in the same
position they occupy in the input. Commands that edit a block's content do
not regenerate its ID. Commands that split a block keep the original ID on
the first half and allocate a fresh ULID for the new half. A future CRDT
layer addresses blocks by `BlockId` only; no merge regenerates IDs.

### I3 — Every mutation is undoable

No code outside `closure-core::commands` mutates `Document`. Every
registered `Command` produces an `Edit` appended to the undo-tree.
Property-tested: any sequence `apply(c1..cn); undo n` reproduces the
original `Document`.

### I4 — Every command has a keybinding entry

The which-key popup, command palette, and doc generator read from the
command registry directly. There is no hand-maintained keybinding table.
Input modes (Emacs / vim / Doom / helix / Notion-mouse-block) consume the
registry; they do not define bindings independently.

### I5 — No panics in kernel crates (closure-org, closure-core)

Malformed input returns `Err`, never panics. Enforced by:

- `#![forbid(unsafe_code)]` in every crate.
- `clippy::unwrap_used` and `clippy::expect_used` denied in workspace lints.
- Fuzz target on every parser's `parse`, run against the committed corpus.

### I6 — Determinism

Given identical input, `parse`, `print`, and every query return identical
output. Property-tested.

### I7 — Kernel-agnostic shells

Every shell (TUI, egui, gpui, web, Tauri, Flutter, GTK, Qt, Slint,
self-contained HTML) consumes only the stable `closure-core` public API.
Shells never import `closure-org` directly. Shells never address content by
byte offset or line number. Enforced by a visibility boundary: byte
offsets and source spans are `pub(crate)` inside parser crates only.

### I8 — Command-registry as only side-effect surface

LLMs, MCP / LSP / ACP / A2A bridges, cron, wasm plugins, formula
evaluators, and Babel backends mutate `Document` only through registered
commands. No subsystem gets a privileged `&mut Document`. Adapter crates
(`closure-llm`, `closure-mcp`, `closure-cron`, `closure-plugin-host`) are
thin translators from their protocol to registry commands.

### I9 — Config validation at load, not at use

Config grammar is a typed schema (CUE-like dependent-type style). Config
errors fail the load, not the feature. `closure-config` owns the schema;
all crates read config only through typed handles.

### I10 — Deterministic, hermetic, reproducible builds

`nix flake check` is green in CI. `cargo build` is offline-reproducible
within the devshell. Every shell produces a self-contained artifact for its
target.

## Layers

The system partitions into six layers. Every crate belongs to exactly one
layer. Every vision item is assigned to exactly one layer.

### Layer 0 — Foundation

- `closure-spec` — executable spec: golden fixtures, property-test
  harnesses, invariant checks. Imported by every crate's test tree.
- `closure-config` — typed config (I9); org-code-block-as-config loader.
- `closure-util` — ULIDs, crate-internal span types, error types, shared
  primitives.

### Layer 1 — Parsers

- `closure-org` — Emacs org-mode, byte-exact roundtrip (core subset first,
  see "Layer 1 — closure-org parser scope" below). Span-preserving
  hand-written recursive descent over line cursors. No external parser
  backend dep.
- `closure-markdown` — CommonMark + GFM, byte-exact roundtrip, same
  architecture. Later phase.
- `closure-tree-sitter` — optional, for syntax-highlighting and code-block
  grammars inside `#+BEGIN_SRC` regions. Not used for primary parsing
  (I1 / I5 cost too high).

### Layer 1 — closure-org parser scope (v0.1 subset)

Carry-forward of the original v0.1 subset. All constructs roundtrip
byte-exact.

In scope:

- Headlines (`*`, `**`, …), TODO keywords, priorities (`[#A]`), tags
  (`:work:urgent:`).
- Lists (unordered `-` / `+`, ordered `1.` / `1)`, nested, checkbox
  `- [ ] / [X] / [-]`).
- Property drawers (`:PROPERTIES:` / `:END:`) including `:ID:`.
- Logbook drawers (`:LOGBOOK:` / `:END:`) — content verbatim.
- Code blocks (`#+BEGIN_SRC lang ... #+END_SRC`) — content verbatim,
  no execution in this layer.
- Timestamps (active `<...>`, inactive `[...]`, ranges).
- Links `[[target][description]]`.
- Tables (`| a | b |` + `|---|---|`).
- Inline markup: `*bold*`, `/italic/`, `=code=`, `~verbatim~`, `+strike+`,
  `_under_`.
- Keywords (`#+TITLE:`, `#+FILETAGS:`, …) verbatim.
- Comments (`# ...`) verbatim.

Preserved as opaque text (no semantics yet):

- Babel execution, `#+RESULTS:` handling semantics.
- Export markup, `#+LATEX:` / `#+HTML:` specials.
- Agenda, clocking (`CLOCK:` verbatim).
- Footnotes semantics.

### Layer 2 — Kernel

- `closure-core` — `Document`, `BlockId`, command registry, event bus,
  keybinding trie, `Edit` log. Frontend-agnostic (I7). Forbids direct
  mutation outside `commands::`.
- `closure-store` — vault loader, file watcher, atomic writes, indices.
- `closure-query` — tree / tag / full-text / backlink queries, Notion-style
  database views.
- `closure-undo` — branching undo-tree persisted per vault.

### Layer 3 — Evaluation

- `closure-eval` — sandboxed evaluator for code blocks. Formulas
  (Coda-style), Babel-style execution, cron job bodies all live here.
  Language backends (shell, python, rust-script, wasm) are plugin crates.
- `closure-crdt` — wraps `Document` with Automerge or Yrs. `Edit` becomes
  a CRDT op. No API changes to `closure-core`.
- `closure-sync` — file / git sync first; IPFS or iroh P2P later. Pluggable
  transport.

### Layer 4 — Adapters (I8)

- `closure-llm` — BYOK, local, Claude / OpenAI / compatible. LLM invokes
  registry commands; it gets query access but no `&mut Document`.
- `closure-mcp`, `closure-lsp`, `closure-acp`, `closure-a2a` — protocol
  bridges; each translates messages to registry commands.
- `closure-cron` — cron scheduler triggering commands.
- `closure-plugin-host` — wasm plugin ABI, semver-pinned core API,
  sandboxed.
- `closure-sniffer` — mitmproxy-like, own binary. Shares `closure-config`
  and the command registry. Not linked into other shells.

### Layer 5 — Shells (I7)

- `closure-tui` — ratatui + crossterm. Primary first shell.
- `closure-cli` — the `closure` binary (`tui`, `check`, `fmt`, `parse`,
  `query`, `serve`).
- `closure-shell-egui` — native desktop (egui via eframe).
- `closure-shell-gpui` — native desktop (Zed's gpui). Alternative,
  evaluated alongside egui.
- `closure-shell-web` — self-contained single HTML bundle and
  localhost web-app (`closure serve`).
- `closure-shell-tauri`, `-flutter`, `-gtk`, `-qt`, `-slint` — stubs
  defined for API-contract testing; bodied on demand.

### Layer 6 — Input modes

- `closure-input` — Emacs / vim / Doom / helix / Notion-mouse-block modes.
  Each mode is a keybinding trie + mode state machine over the single
  command registry. Chord syntax supports `<SPC> f f`, literal `SPC`,
  `<C-c> <C-x>` (tempo-style).
- `closure-whichkey` — auto-generated from the registry (I4).

## What forces a v1.0 break

Only these would force a breaking release:

- Switching away from plain-text on-disk representation.
- Removing `BlockId` as the unit of edit addressing.
- Introducing a synchronous global mutable state path that commands
  bypass.

Every design decision is tested against "does this rule out anything
above?" If yes, the decision is wrong.

## CRDT-readiness note

The kernel stays CRDT-ready even before `closure-crdt` ships:

- Block identity is a ULID, not a byte offset or path.
- `Edit` values reference blocks by ID.
- No command addresses content by file position.

A future `closure-crdt` crate wraps `Document` and reinterprets `Edit` as a
CRDT op without changing shells or parsers.
