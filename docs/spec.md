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
  (I1 / I5 cost too high). The dep-free `KeywordHighlighter` is the
  hermetic default; a real grammar (`TsHighlighter`, V6) is opt-in behind
  the `tree-sitter` feature — it pulls a C grammar (non-hermetic, like the
  GUI/pcap features), parses with a genuine tree-sitter grammar (bash), and
  fills inter-token gaps with `Plain` so the `Highlighter` gap-free
  coverage contract still holds. `just tree-sitter` builds/tests it; the
  default `just check` never compiles it (I10). `closure-tui` forwards the
  feature (`closure-tui/tree-sitter`): its file-view `pick_highlighter`
  prefers `TsHighlighter` for a bundled grammar when the feature is on,
  else `KeywordHighlighter` (V6b) — so code blocks get real, string-aware
  highlighting under the gate without changing the hermetic default.

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
  database views, and **composable widgets** (V2): `expand_widgets` expands
  every `#+BEGIN: closure-widget :name X` dynamic block in place — its body
  is a template that may reference other widgets via `{{name}}`, resolved
  recursively with cycle detection (`WidgetError::Cycle`/`Unknown`). Like
  the `closure-view` database block, only the body between `BEGIN`/`END` is
  regenerated; every other byte is preserved verbatim (I1). This is the
  vision's "compose existing blocks into new blocks/widgets". Definitions
  are vault-wide (V2b): `vault_widget_defs` / `vault_widget_names` collect
  every `:name` across the vault, `expand_doc_widgets(vault, path)` resolves
  a file's `{{ref}}`s against all of them, `closure widgets` lists them, and
  cyclic/unknown references surface as `closure-lsp` diagnostics
  (`DiagnosticCode::Widget`, reusing the L3 pull path). A widget renders in
  any shell as a `Node::Widget { name, content }` ViewTree node (V2c):
  `expand_named_widget(vault, name)` resolves one widget's content, which
  every renderer (`closure-tui`, `closure-shell-web`) displays — so the
  same composite block drops into any file, in any shell.
- `closure-undo` — branching undo-tree persisted per vault.

### Layer 3 — Evaluation

- `closure-eval` — sandboxed evaluator for code blocks. Formulas
  (Coda-style), Babel-style execution, cron job bodies all live here.
  Language backends (shell, python, rust-script, wasm) are plugin crates.
  **Default-deny (C1a):** no code block executes unless its language is
  listed in the vault's typed `eval_trust` allowlist (`config.org`,
  validated at load — I9). An empty or unreadable policy runs nothing.
  This holds on every execution path (`Vault::eval_block`, `closure
eval`); a pulled/synced vault cannot run code without explicit local
  trust. **Resource bounds (C1b):** every trusted execution runs under
  `Bounds` — a wall-clock deadline (default 10s; runaway → killed,
  `Timeout`) and a per-stream output cap (default 10 MiB; flood → child
  killed, output truncated), with the child in its own process group.
  **Wasm sandbox tier (C1c):** the `wasm` backend (opt-in `wasmtime`
  feature) runs a module under wasmtime with **no host imports** and a
  finite fuel budget — a module that needs any import fails to
  instantiate, and a runaway loop traps on out-of-fuel. This is the
  genuinely sandboxed tier (no host surface, no process spawn, true
  containment); the shell/python backends remain the opt-in "trusted"
  tier still subject to the C1b process bounds.
- `closure-crdt` — also exposes 3-way conflict detection (V9): `conflicts(
base, ours, theirs)` returns the `FieldConflict`s (title/body) both sides
  changed divergently relative to a common base — instead of letting LWW
  silently pick a winner — so a shell can offer a real resolution choice.
  Pure + deterministic; the auto-merge stays the default, this is the
  user-facing inspection layer. `closure_shell_core::ConflictApp` (V9b)
  renders the conflicts as a `ViewTree` and applies the user's
  ours/theirs choice through the vault command path (rename/set-body —
  undoable, I3/I8), removing each resolved conflict; the `resolve-ours`/
  `resolve-theirs` chords come from the keymap (V1 rule).
- `closure-crdt` — wraps `Document` as a set of CRDT replicas keyed by
  `BlockId`. `Edit` becomes a CRDT op. No API changes to `closure-core`.
  Shipped model: the title is a per-block last-writer-wins register
  (carrying vector-clock logical time); the **body is a character-level
  RGA** (`BodyCrdt`, C2b) so two replicas editing the _same_ body
  concurrently both keep their edits and converge to the same text
  regardless of merge order. Hand-rolled, no external CRDT dep — Automerge
  / Yrs drag large/partly-async dependency trees that fight I10's
  hermetic, dep-minimal build (2026-06-19 char-CRDT Decision). Both sit
  behind the same `Edit` / `BlockId` surface with no `closure-core` API
  change. Residual LWW point: a _title_ edited concurrently on two
  replicas still resolves last-writer-wins (titles are short labels, not
  collaborative prose). See the CRDT-readiness note below.
- `closure-sync` — file / git sync first; IPFS or iroh P2P later. Pluggable
  transport. **Authenticated frames (C3a):** each peer holds an ed25519
  keypair; `SyncMessage::to_signed_bytes` signs the version + replica
  payload and `from_signed_bytes` verifies it against the embedded key
  (rejecting tampering) and an optional trusted-peer set (rejecting a
  forged/unknown signer) _before_ the message reaches `apply_message`.
  **Transport encryption (C3b):** a Noise NN channel (`NoiseChannel`,
  pure-Rust `snow`: x25519 + ChaCha20-Poly1305 + BLAKE2s) encrypts the
  wire so the replica never travels in plaintext; `connect_and_sync_secure`
  / `serve_once_secure` handshake the socket then exchange the C3a-signed
  frames over the encrypted channel (confidentiality from Noise,
  authenticity from the inner signatures). The same `SyncMessage` framing
  keeps a future iroh/QUIC transport a drop-in. **Content addressing
  (V5a):** `Cid::of(bytes)` is a stable, dep-free content id (FNV-1a,
  prefixed `b1`; a `sha256` CID can be added behind a feature without an
  API change); `BlockStore` keys blobs by `Cid` — `put` dedups, `verify`
  re-hashes on read to detect tampering. This is the IPFS-style substrate
  the content-address sync (V5b) exchanges over. The `BlockProvider` trait
  (has/get/put/cids) abstracts the store — in-memory (`BlockStore`) and
  filesystem (`FsBlockStore`) impls ship; an IPFS/iroh network provider is
  a future impl behind the same trait (external/feature-gated, hermetic
  core). `sync_providers(a, b)` copies each blob one side lacks so both
  converge to the union; content addressing makes it order-independent and
  transfer-verifiable.

### Layer 4 — Adapters (I8)

- `closure-llm` — BYOK, local, Claude / OpenAI / compatible. LLM invokes
  registry commands; it gets query access but no `&mut Document`. Beyond
  the data tools (`list-files`/`read`/`search`/`view-state`), the
  `view-render` tool (V3a) returns a serialised snapshot of the rendered
  **ViewTree** (`closure_shell_core::serialize_view` over `browse_view`) —
  so the agent can see _what is on screen_ (panes, selection, visible
  rows, fields), the differentiator over assistants that only touch data.
  Read-only. Render access is governed by `LlmPermissions` (V3b): it is
  **off by default** (opt-in), can be granted in `llm_tools` config, and is
  revocable/grantable at runtime via `toggle_render` — the
  `toggle-llm-render` command, bound in every input mode's keymap so it
  shows in which-key (I4). The user controls render exposure live, per the
  vision's "configure that (live) too".
- `closure-mcp`, `closure-lsp`, `closure-acp`, `closure-a2a` — protocol
  bridges; each translates messages to registry commands.
  - `closure-mcp` serves `initialize` + `tools/list` + `tools/call`, and
    (V8a) `resources/list` + `resources/read` (vault files as MCP
    resources, `file://` uris) and `prompts/list` + `prompts/get`
    (capture/ask templates as MCP prompts), over the dep-free JSON subset;
    `initialize` advertises the `resources`/`prompts` capabilities.
  - `closure-acp` `agent/card` lists the agent's `capabilities`, and
    `agent/negotiate` returns the intersection of a client's proposed
    capabilities with the supported set (V8b). `closure-a2a` delegated
    tasks carry a lifecycle `TaskState` (submitted → working →
    done/failed; a tool result starting `ERROR` fails) via the `Task`
    state machine, and `task/delegate` returns the resulting `state` so a
    caller can track progress.
  - `closure-lsp` serves Content-Length-framed JSON-RPC over stdio. Beyond
    `documentSymbol` + go-to-definition it answers `textDocument/hover`:
    over an `id:` link it previews the target headline (title + a
    `file › ancestors › title` breadcrumb resolved through the vault),
    over a headline it reports `level · id · TODO · :tags:` (L1). It also
    answers `textDocument/completion`, context-sensitive over the source
    line up to the cursor: an unterminated `[[id:` completes to vault ids
    (owning title as `detail`), a headline's keyword slot completes the
    configured `todo_keywords`, and a trailing `:tag:` region completes
    known vault tags (L2). `textDocument/diagnostic` (pull model) reports
    ranged problems: dead `id:` links, duplicate `:ID:` values across the
    vault, and `closure-config` block validation errors mapped back to
    their document line (L3). `textDocument/references` lists the
    definition + every `id:` link to a headline across the vault, and
    `textDocument/rename` retitles the owning headline (L4). All positions
    map over source text (zero-based line/character); read-only methods
    take `&Vault` via `handle_message`, while `rename` takes `&mut Vault`
    via `handle_message_mut` and routes through the command registry
    (undoable, I3/I8). Links are id-based, so references survive a rename.
    closure is server-authoritative — `rename` applies + persists on the
    server and returns `null` rather than a client `WorkspaceEdit`. The
    bridge speaks real Content-Length-framed JSON-RPC: `serve` loops over
    frames dispatching through `handle_message_mut` (`initialized` is a
    no-op, `exit` stops the loop), exposed as `closure lsp <vault>` over
    stdio (L5). Pure + hermetic — every method is a function over the
    vault, tested without an editor process.
- `closure-cron` — cron scheduler triggering commands.
- `closure-plugin-host` — wasm plugin ABI, semver-pinned core API,
  sandboxed. Hosts the package ecosystem (V4): a `Package` (name, version,
  `dep`s with version requirements, provided `command`s) is declared in a
  `closure-package` `key = value` block — plain text, no JSON/YAML — and a
  lockfile (`name version hash` lines) pins resolved versions + content
  hashes. Both round-trip byte-exact; the lockfile renders sorted (I6).
  `resolve(root, available)` walks the transitive dependency graph over a
  local package set (no network), checks `>=X.Y.Z` / exact version
  requirements, detects cycles, and emits a deterministic, declaration-
  order-independent lockfile with FNV-1a content hashes (V4b). A local
  registry directory of `*.org` package files is loaded by `load_packages`
  (`extract_package_block` pulls each file's `closure-package` block);
  `closure pkg list <registry>` enumerates them and `closure pkg lock
<manifest> <registry>` resolves + writes `closure.lock` (V4c). All
  hermetic — a path registry, no network (a network source is a future
  feature-gated extension).
- `closure-sniffer` — mitmproxy-like, own binary. Shares `closure-config`
  and the command registry. Not linked into other shells. The packet
  decoder (`parse_candidate`, Ethernet→IPv4→TCP/UDP) is dependency-free
  and hermetically tested; live capture (`PcapBackend`, `pnet` raw
  sockets) is opt-in behind the `pcap` feature and needs `CAP_NET_RAW`
  at runtime (X3). `closure sniff --live <iface>` drives it; the mock
  stays the hermetic default. The interactive surface is a headless
  `closure_shell_core::SnifferApp` (V7): a pure state machine over the
  capture trait — live event list, cursor, substring filter, and per-flow
  allow/block toggles that mutate the blocklist rules — unit-tested without
  a terminal, the same pattern as the launcher `App`, and rendered as a
  `ViewTree` by a shell (V7b): `SnifferApp::view(mode)` builds the flow
  list + a block/allow detail pane whose actions carry their chords
  (`block-flow`/`allow-flow`, bound in every keymap), and `closure sniff
--tui <candidate>` renders it via `closure_tui::render_view` (hermetic;
  live capture stays `pcap`-gated).

### Layer 5 — Shells (I7)

- `closure-shell-core` — the dep-free engine the shells share. Holds the
  pure `App`/`ModalApp` state machines **and the declarative `ViewTree`**
  (V1): `App::view(&Shell) -> Node` derives a pure description of the
  screen (panes, headline rows, detail fields, palette, input buffers,
  which-key hints) that every embedder renders — the Flutter
  engine/embedder split, one description and many renderers. Every
  _actionable_ node carries an `Action`, and `Action` has no constructor
  that omits the chord (`Action::new` returns `None` when the active mode
  binds none), so the "every UI element shows its keybinding" rule is
  type-enforced rather than convention. `view` is a pure function of
  state → deterministic (I6) and testable without a display. Both
  `closure-tui` (`render_view -> Vec<String>` text lines) and
  `closure-shell-web` (`render_view -> String` HTML) render the same
  `Node` tree (V1b) — the proof that the description is decoupled from
  the embedder; egui/gpui adapters follow the same `render(tree)` entry.
  Actionable nodes carry their chord into the rendered output (`[..]` /
  `<kbd>`), and the renderers are hermetic (golden-testable, no display).
  Which `Node` kinds a shell renders is itself data — `NodeKind`,
  `ALL_NODE_KINDS`, the per-shell `*_NODE_KINDS` consts, and
  `ui_matrix_table` (printed by `closure ui-matrix`) give the type-level
  UI venn/diff (V1c), the sibling of the `closure shells` capability
  matrix. A renderer's `match` over `Node` is exhaustive, so adding a
  kind without handling it is a compile error — a shell that does not
  render a kind is a compile-/test-time fact, not a runtime surprise.
- `closure-tui` — ratatui + crossterm. Primary first shell.
- `closure-cli` — the `closure` binary (`tui`, `check`, `fmt`, `parse`,
  `query`, `serve`).
- `closure-shell-egui` — native desktop (egui via eframe).
- `closure-shell-gpui` — native desktop (Zed's gpui). Alternative,
  evaluated alongside egui.
- `closure-shell-web` — self-contained single HTML bundle and
  localhost web-app (`closure serve`). `closure-wasm` (X2) is the
  client-side upgrade: a wasm-bindgen surface over the kernel
  (`reformat`/`headline_titles`) that `inline_wasm_editor` embeds into
  the export so the single HTML file re-parses edits in the browser,
  offline. Opt-in `wasm` feature; `just wasm-web-bundle` builds it; the
  default build never pulls wasm-bindgen (I10).
- `closure-shell-tauri` — native webview desktop shell (X1a): hosts the
  web shell's `export_html` page in a `wry`/`tao` window (Tauri's webview
  foundation). Opt-in `tauri` feature; the default build never pulls the
  webview stack (I10). The HTML payload (`page`) is hermetically tested;
  the window is display-bound (build-verified under `nix develop
.#webview`, launch is manual).
- `closure-shell-gtk` — native GTK4 desktop shell (X1b): the vault's
  headlines as a scrollable `gtk4-rs` list. Opt-in `gtk` feature; the
  default build never pulls GTK (I10). The list content (`rows`) is
  hermetically tested; the window is display-bound (build-verified under
  `nix develop .#webview`, launch is manual).
- `closure-shell-qt` — native Qt6/QML desktop shell (X1c): the vault's
  headlines in a QtQuick `ListView` via `qmetaobject`. Opt-in `qt`
  feature; the default build never pulls Qt (I10). `rows` + the QML
  document (`qml_document`) are hermetically tested; the window is
  display-bound (build-verified under `nix develop .#webview`, launch is
  manual).
- Flutter (X1d) — external packaging project, not a workspace crate
  (the Dart/Flutter SDK is not hermetically nix-packaged; I10). It
  consumes closure over `closure serve` HTTP or a `flutter_rust_bridge`
  FFI to `closure-shell-core`. See `docs/flutter-shell.md`.
- `-slint` — stub defined for API-contract testing; bodied on demand.

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

## Built-to-last kernel primitives (the "LISP-7" idea)

The kernel is defined by a minimal set of primitives. Every feature (views,
formulas, LLM tools, sync, etc.) reduces to these. This is the "well thought
basics" and "spec that has well thought basics" from the vision (analogous to
a LISP with 7 primitives).

The primitives (documented here as the contract; implemented across closure-org,
closure-core, closure-store, closure-query, closure-undo, closure-crdt, etc.):

- **parse**: turn plain text (org) into a Document (span-preserving, I1).
- **print**: turn Document back to exact source text (I1).
- **apply**: a Command mutates a Document, producing an Edit on the undo tree (I3, I8).
- **undo**: reverse the last Edit (branching undo tree, I3).
- **query**: read-only access (backlinks, fuzzy, agenda, views, all_headlines, etc.).
- **snapshot**: capture a point-in-time Replica for CRDT (with logical/vector time).
- **merge**: combine replicas (LWW per field + vector clocks for causality; apply back via commands, I2).

The rule: if a proposed feature cannot be expressed using only these (plus stable BlockId addressing and the command registry as the only mutation surface), the feature is wrong or the spec must be revised in the same commit.

This section is the "Built-to-last kernel spec" (ROADMAP item). The invariants I1–I10 and the layer firewalls ensure nothing bypasses the primitives.

(Tests in closure-org, closure-core, closure-crdt, etc. exercise these primitives and the "reduces to" rule via the golden, proptest, and merge fixtures.)
