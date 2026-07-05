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
  backend dep. **D9 — tables are queryable:** `OrgDoc::tables()` returns
  `TableView`s for every table in the document — preamble *and* headline
  bodies — with `data_rows()` yielding the trimmed cells (separators
  classified, not data). Recognition over the existing `TableRow` nodes, so
  I1 is untouched; this is the structured substrate the Notion-style
  database views read. Fuzz-guarded against panics (I5) in `tables.rs`.
- `closure-markdown` — CommonMark + GFM, byte-exact roundtrip (I1), same
  source-preserving span architecture as `closure-org`. Block classifier
  (per line, so I1 holds by construction): ATX headings, paragraphs, blank
  lines, list items, fenced code, and (D1) blockquotes, GFM tables, and
  thematic breaks. Proven by a proptest fuzz (`properties.rs`: I1 roundtrip
  + I5 no-panic + I6 determinism on random input, in `just fuzz`) and a
  golden corpus under `fixtures/md/`. `from_org`/`to_org` bridge the
  line-level subset. Inline markup (emphasis/links/code spans) and setext
  headings are a later increment — they do not affect the roundtrip, only
  finer classification.
- `closure-tree-sitter` — optional, for syntax-highlighting and code-block
  grammars inside `#+BEGIN_SRC` regions. Not used for primary parsing
  (I1 / I5 cost too high). The dep-free `KeywordHighlighter` is the
  hermetic default; a real grammar (`TsHighlighter`, V6) is opt-in behind
  the `tree-sitter` feature — it pulls C grammars (non-hermetic, like the
  GUI/pcap features), parses with genuine tree-sitter grammars and fills
  inter-token gaps with `Plain` so the `Highlighter` gap-free coverage
  contract still holds. `TsHighlighter::for_language` is the grammar
  registry, keyed by language name — bundled grammars (D5):
  `bash`/`sh`/`shell`, `rust`/`rs`, `python`/`py`, `json`. Node-kind
  mapping is by substring/suffix (`*comment*` → Comment;
  `*string*`/`number`/`*_literal` → Literal) so one classifier spans every
  grammar's differing node names. `just tree-sitter` builds/tests it; the
  default `just check` never compiles it (I10). `closure-tui` forwards the
  feature (`closure-tui/tree-sitter`): its file-view `pick_highlighter`
  prefers `TsHighlighter` for a bundled grammar when the feature is on,
  else `KeywordHighlighter` (V6b) — so code blocks get real, string-aware
  highlighting under the gate without changing the hermetic default.
  `closure_tui::render_snapshot(&Node)` (V10a) joins the `render_view`
  lines into one deterministic text snapshot — the headless render harness
  that golden-tests the shared renderer with no terminal, turning the
  "display-bound, pixel-unverified" caveat into real coverage of the
  render path.

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
- `closure-store` — vault loader, file watcher, atomic writes, indices,
  kill ring (cut/paste subtrees, a move so ids stay unique, I2). **D7 —
  OS-clipboard bridge:** the kill ring is the hub; a `Clipboard` adapter
  mirrors its top *out* (`mirror_ring_top_to_clipboard`) and pulls external
  text *in* (`pull_clipboard_to_ring`, which then pastes through the same
  span-preserving path). Additive — cut/paste never need a clipboard. The
  hermetic default is `MemoryClipboard`; `SystemClipboard` (behind the
  `clipboard` feature) shells out to the platform tool (wl-copy/xclip/
  pbcopy) with no extra crate, so the build stays hermetic (I10) — only the
  runtime needs the tool. Gate: `just clipboard`.
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
- `closure-sync` — file / git sync, **a real network transport over std
  TCP**, and an external iroh drop-in. Pluggable transport.
  **Authenticated frames (C3a):** each peer holds an ed25519
  keypair; `SyncMessage::to_signed_bytes` signs the version + replica
  payload and `from_signed_bytes` verifies it against the embedded key
  (rejecting tampering) and an optional trusted-peer set (rejecting a
  forged/unknown signer) _before_ the message reaches `apply_message`.
  **Transport encryption (C3b):** a Noise NN channel (`NoiseChannel`,
  pure-Rust `snow`: x25519 + ChaCha20-Poly1305 + BLAKE2s) encrypts the
  wire so the replica never travels in plaintext; `connect_and_sync_secure`
  / `serve_once_secure` handshake the socket then exchange the C3a-signed
  frames over the encrypted channel (confidentiality from Noise,
  authenticity from the inner signatures). **Real wire transport (D3):**
  `TcpSyncTransport` runs the protocol over an actual socket —
  `serve_once`/`connect_and_sync` (plain) and `serve_once_secure`/
  `connect_and_sync_secure` (authenticated + encrypted). Tested over
  `127.0.0.1` loopback (`tcp.rs`, `encrypt.rs`, `p2p_i2.rs`): two peers with
  divergent vaults converge, every block id is preserved verbatim across
  the network merge (I2, no regeneration), and an untrusted signer is
  rejected on the wire. Loopback is hermetic so it runs in the default
  suite; `just sync-net` is the explicit network gate and also exercises
  the external `IrohTransport` (gracefully skipped when the `iroh` binary
  is absent). The same `SyncMessage` framing keeps iroh/QUIC a drop-in.
  **Content addressing
  (V5a / D2):** `Cid::of(bytes)` is a stable, cryptographic content id —
  a 256-bit BLAKE3 digest, prefixed `b3` (pure-Rust `blake3`, hermetic;
  the value is opaque so the algorithm can change again without an API
  change); `BlockStore` keys blobs by `Cid` — `put` dedups, `verify`
  re-hashes on read and, because BLAKE3 is collision-resistant, a tampered
  blob provably cannot reuse its `Cid`. This is the IPFS-style substrate
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
  vision's "configure that (live) too". **D4 — the full loop, hermetic:**
  `OpenAiWireProvider` is a dep-free mock of the OpenAI chat-completions
  *wire* — it encodes each prompt into a real OpenAI request body and
  decodes the scripted reply out of a canonical OpenAI response envelope
  (`openai_response_json` ↔ `extract_openai_content`), so the dep-free JSON
  round trip is exercised with no curl/socket. `tests/view_loop.rs` drives
  the end-to-end story: the model reads the `ViewTree` via the
  permission-gated render tool, then mutates the vault — the change flowing
  **only** through `Shell::capture` (a registry command, I8) — and the
  re-rendered view observably reflects it. Live BYOK/HTTP (`CurlProvider`/
  `HttpProvider`) stays the opt-in, non-hermetic tier.
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
  render a kind is a compile-/test-time fact, not a runtime surprise. The
  `caps` module (V11) extends this to _capabilities_: a sealed
  `Capability` marker per capability + per-shell `Supports<C>` impls mean
  `capability_gate::<S, C>()` compiles iff shell `S` declares `C`, so a
  shell invoking an unsupported capability is a compile error (a
  `compile_fail` doctest proves the negative) — the Yesod rule applied to
  the shell/capability matrix. For accessibility (V12a), every `Node`
  derives a semantic `aria_role` (region/list/group/textbox/listbox/
  status/note) and, where natural, an `aria_label` (pane title, input
  label, widget name); the web shell emits `role`/`aria-label` attributes
  from them so a screen-reader can navigate the rendered tree. For mobile
  (V12b) the web page is responsive (a `width=device-width` viewport +
  `@media` layout), usable on a phone browser with no native build; a
  native mobile app stays an external Flutter project (Dart/Xcode/NDK are
  non-hermetic, like X1d) — see `docs/flutter-shell.md`.
  The vocabulary is grown for richer GUI surfaces (GUI-UX): `Node::Split
  { direction: SplitDir, panes }` (G1a) is a multi-pane layout — the
  foundation for a real editor surface (sidebar + main + detail). Like
  every kind it is exhaustively matched by `kind`/`aria_role`/
  `view_to_json`/`serialize_view` and both `render_view`s, so a renderer
  that omits it does not compile; the hermetic guarantee is the pane
  *set + order + axis* (golden-tested), not pixels — those stay the
  embedder's display-bound job. `Node::Modal { title, body }` (G1b) is a
  titled overlay layer (command palette / confirm dialog / prompt) —
  `aria_role` `dialog`, the title as `aria_label`; the web shell emits
  `role="dialog"` and the embedder paints the dim/float. `Node::Toast {
  level: ToastLevel, text }` (G1c) is a transient, severity-classed
  notification — `ToastLevel` (Info/Success/Warning/Error) drives both the
  CSS class and the ARIA live-region politeness (`alert` for warn/error,
  `status` for info/success), the substrate G7 fills with async outcomes.
  Theming is declarative + typed (G2): `closure_shell_core::Theme` is a
  palette (`ColorRole` slots) + spacing + typography as data, resolved
  from the free-form `config.theme` string (`Theme::from_name`:
  light/high-contrast/dark) — three built-ins, no runtime stylesheet
  parsing. Each shell maps the tokens to its native layer: the web shell
  emits `:root` CSS custom properties (`theme_css_variables`), the TUI
  resolves a `ColorRole` to a ratatui `Color::Rgb` (`theme_color`). A
  malformed colour resolves to black, never a panic (I5); resolution +
  token values are hermetic, only the pixels are the embedder's.
  Rows carry presentation as data (G5a): `RowView` has an `icon`
  (TODO-status glyph) + `badges` (tags / priority chips), populated by
  `browse_view` from the headline, and every renderer (tui/web/gtk/qt +
  JSON/snapshot) draws them — empty by default, so existing goldens are
  unchanged. Interaction is state, not per-shell ad-hoc (G5b):
  `Interactions` tracks focused / hovered / pressed / disabled element
  indices and `state_of(i)` resolves an `ElementState` under a fixed
  precedence (`Disabled > Active > Focused > Hovered > Normal`); a shell
  paints the focus ring / hover / pressed / dimmed styling from this one
  tested machine — the pixels are the embedder's. Notion affordances are
  commands/state (G5c): `slash_menu(query, mode)` is the "/" command menu
  (fuzzy-filtered `PaletteItemView`s with chords), `block_insert_action`
  is the block "+" (the `add-sibling` command), and `DragReorder` +
  `reorder_indices` model drag-to-reorder as a pure gesture whose drop
  maps to a registry move (I8) — every affordance reduces to a kernel
  command, none is shell-private. **Fold toggle (F1):** the outline hides
  a folded headline's descendants in both the launcher `App` and the modal
  `ModalApp` row walks; fold state is the org-standard
  `:VISIBILITY: folded` property on the headline itself — written through
  the registry (`SetProperty`, I8, undoable I3), so it persists between
  program runs in plain text and is honoured by Emacs org-mode. Unfold
  writes `:VISIBILITY: all`. A live query searches *into* folds (org
  isearch behaviour); a folded row carries a `▸` badge. `toggle-fold` is
  bound in all five keymaps (I4: `z`, Emacs `C-c z`), in the palette
  (`fold`), and on `C-f` in the launcher. The command palette is polished + shared
  (G6): `command_palette(query, mode)` groups every command into ordered
  sections (Navigate/Edit/Mode/App), fuzzy-ranks within each, drops empty
  ones, and gives each `PaletteEntry` a human description + its chord;
  `serialize_palette` is the deterministic snapshot a shell renders — one
  source, so the which-key/palette is identical across GUIs. Async
  feedback is a typed surface (G7): `Feedback` is a queue of `notify`
  (severity) + `progress` (labelled, updated in place) for long ops
  (sync/eval/llm), and `to_nodes()` renders each as a `Node::Toast` (G1c)
  — so every shell already shows notifications + progress with no
  per-shell code. The cross-shell mapping is golden-pinned (G8):
  `closure-cli/tests/visual_golden.rs` renders ONE canonical `ViewTree`
  (covering every `NodeKind`, self-guarded) through all four mappings —
  tui text (exact golden), web HTML, gtk `widget_tree`, qt `qml_view` —
  pinning the structure each produces + determinism. Pixels stay
  unverifiable, but the `ViewTree`→native mapping is now regression-locked
  as far as hermetically possible. The UI capability matrix (G9) now spans
  five columns — `MIN`/`TUI`/`WEB`/`GTK`/`QT` (`*_NODE_KINDS` consts +
  `ui_matrix_table`): after G3/G4 the native shells render the *full*
  `ViewTree`, so every column except `MIN` covers `ALL_NODE_KINDS`, each
  backed by an exhaustive `match` (a new kind is a compile error in every
  renderer). The runtime `Capability` matrix (`closure shells`) carries the
  GUI surfaces too (P7): `Palette`/`Theme`/`Feedback` join the enum, an
  `INTERACTIVE_EDITOR_CAPABILITIES` bar names the full interactive-editor
  set, and `GTK`/`QT` columns are added. The native `ViewTree` editors
  (`TUI`/`GTK`/`QT`) meet the bar (asserted); `WEB`/`TAURI` are the
  capture-form web tier (no full `Edit`); `GPUI`/`EGUI` are interactive
  editors whose themed/feedback *window* wiring is the remaining polish.
  GTK4 (G3) is no longer a read-only list: `closure_shell_gtk` consumes
  the shared `App`/`Shell` and renders the full `ViewTree` via
  `widget_tree` — a hermetic, golden-tested GTK4 widget-tree descriptor
  exhaustive over every `Node` kind (so it tracks the G1 vocabulary at
  compile time). Editing routes through the shared `Shell` (I8), proven by
  a headless capture-changes-the-tree test; the windowed `run` builds the
  same structure with real `gtk4` widgets (display-bound, feature-gated).
  The GTK window is now interactive (P2): an `EventControllerKey`
  translates each GDK key to a `KeyEvent` and the list repaints from
  `next_frame` (= `widget_tree(App::dispatch(…))`) — capture/rename/delete
  edit the vault in the window. `next_frame` is the hermetic seam (tested),
  the GDK translation + repaint are display-bound.
  Qt6/QML (G4) is the same story: `closure_shell_qt::qml_view` renders the
  shared `ViewTree` to a `QtQuick.Controls`/`Layouts` document (exhaustive
  over every `Node` kind), edits route through the shared `Shell` (I8,
  headless test), and `run` loads it in a real `qmetaobject` window. The
  legacy `qml_document` list path is retained. The Qt window is now
  interactive (P3): a `Bridge` `QObject` exposes `on_key` to QML
  (`Keys.onPressed`) + a `frame` property; each key runs `next_frame`
  (`qml_view(App::dispatch(…))`) and republishes `frame`. `next_frame` is
  the hermetic seam (tested); the `QObject` bridge is build-verified under
  `.#webview`.
  Both native windows apply the shared theme (P5):
  `closure_shell_gtk::theme_css` maps the palette to a GTK4 CSS string the
  window loads into a `CssProvider`, and `closure_shell_qt::theme_qml` maps
  it to QML `property color` decls the host document binds (window `color`,
  text colour). The mappings are hermetic; gpui/egui/web/tui already
  consume the tokens (`Theme::color` rgb / CSS vars / ratatui colour, G2).
  Feedback + interaction states reach every window from the shared machines
  (P6): `with_feedback(base, &Feedback)` composes the typed queue onto a
  `ViewTree` as `Node::Toast` nodes (which every shell already renders,
  G1c/G8), and `ElementState::class` is the stable paint token
  (`focused`/`hovered`/`active`/`disabled`) each shell maps to its native
  focus-ring / hover / dimming.
  Every shell's window drives ONE shared input step (GUI-PARITY P1):
  `App::dispatch(shell, &KeyEvent) -> Node` applies a typed `KeyEvent`
  (key + ctrl + typed char) via the mode-aware `on_key` (mutating through
  the registry, I8) and returns the refreshed `ViewTree`. A shell using
  only `dispatch` can edit the vault; key handling is the tested core, not
  per-shell logic. Proven headlessly (a full capture round-trip + persist
  through `dispatch` alone).
- `closure-tui` — ratatui + crossterm. Primary first shell.
- `closure-cli` — the `closure` binary (`tui`, `check`, `fmt`, `parse`,
  `query`, `serve`).
- `closure-shell-egui` — native desktop (egui via eframe).
- `closure-shell-gpui` — native desktop (Zed's gpui). **The reference
  GUI shell (Decision 2026-07-04).** The window hosts the `ModalApp`
  command surface (not the type-to-filter launcher): Browse keys are
  commands resolved against the active mode's keymap with pending-chord
  which-key completions, `/` opens the search overlay that owns
  type-to-filter, and the footer chords are therefore *honest* — what
  the bar shows is exactly what the key does. Every mouse affordance
  dispatches a registry-backed command through the same `ModalApp::run`
  entry the chords use (I8): row click selects, the fold arrow toggles
  `toggle-fold`, which-key chips run their command, palette rows run the
  clicked entry (`palette_click`), and the detail fields open their
  editor (title → `rename`, meta → `toggle-todo`, tags → `edit-tags`,
  properties → `edit-property`, body → `edit-body`). Structural
  editing (Q6, 2026-07-05): `promote`/`demote`/`move-subtree-up`/
  `move-subtree-down`/`add-heading` act on the selected row through
  Shell passthroughs of the kernel commands (I8); moves stop at file
  and parent boundaries, the selection follows the moved heading, and
  the chords are `M-h/l/k/j/RET` (org-authentic `M-<arrows>` in the
  emacs map). Colours come from
  the shared `Theme` tokens resolved from `config.org`
  (`resolve_theme`), the startup mode from `input_mode`
  (`resolve_input_mode`); hover/panel shades are derived by a pure
  `mix_u32` blend — no hardcoded palette. The pure helpers are
  hermetically tested (`tests/helpers.rs`); the window itself is
  display-bound (feature `gpui`, build-verified + manual smoke).
  **Editor depth (2026-07-04):** the org-edit-special surface is a
  vim-modal editor — `BodyEditor` in shell-core holds a real
  unicode-safe cursor; INSERT types at the cursor (`Esc` → NORMAL),
  NORMAL navigates (`h/j/k/l/0/$`), edits (`i/a/o/x`) and `Esc`
  cancels; `C-Enter` commits from either mode (I8 `set_body`). TAB in
  INSERT runs org-tempo (`<s`→`#+BEGIN_SRC …`, `<e/<q/<c/<C/<v`),
  otherwise soft-indents. `C-n`/`C-p` cycle completion over org
  keywords + dabbrev words mined from the vault (`body_completions`;
  the prefix matches case-insensitively, the candidate keeps its case).
  Completion is fuzzy (Q2, 2026-07-05): candidates match by
  `closure_query::fuzzy_score` subsequence, ranked score-descending
  (keywords beat vault words on ties, then alphabetical); a session
  holds the top 8 and TAB *accepts* the applied candidate (ends the
  session, beating org-tempo; without a session TAB stays
  tempo/indent).
  NORMAL grew the vim vocabulary (2026-07-04b): `v` opens a charwise
  VISUAL selection (inclusive; motions extend, `y` yanks, `d`/`x`
  delete), `dd`/`yy` cut/copy the line, `p` pastes (linewise below the
  line, charwise after the cursor) — one register shared with the
  INSERT readline chords `C-a`/`C-e`/`C-b`/`C-f`/`C-d`/`C-k`/`C-u`/
  `C-w`/`C-y`, plus the desktop-standard set (Q5, 2026-07-05):
  `ctrl+backspace` kills the word back, `ctrl/alt+←/→` jump word
  starts. The renderer reads the selection through
  `ModalApp::body_selection()` (exclusive byte range, `None` outside
  Visual) and paints it exactly, span-split at the selection edges.
  Editor depth Q1 (2026-07-05): `V` opens linewise VISUAL
  (`EditorMode::VisualLine`; whole-line selection, linewise `y`/`d`/`x`
  with paste-below register semantics); digits accumulate vim counts
  applied to motions and edits (`3w`, `2dd`, `5x`; `Esc` clears a
  pending count first); `u`/`C-r` are editor-local undo/redo (snapshot
  stacks bounded at 50, checkpointed before every mutating
  Normal/Visual edit, independent of the vault-level undo; INSERT
  bursts are a recorded gap); `w`/`b` word motions (simple
  skip-word-skip-whitespace rule, count-aware, cross lines, clamp).
  Leaving INSERT steps the cursor back onto the last typed char (vim
  rule); after `dd` on the last line the cursor parks on the line
  above (paste-back symmetry).
  Backlink rows are click targets (`backlink_click`, the Enter jump's
  mouse path). The window renders the shared `Feedback` queue (G7) as
  a toast strip fed by `status_toast` over status-line changes
  (failures error, destructive successes warn, chatter silent). The
  wheel scrolls the viewport, not the cursor: `ModalApp::scroll_by`
  sets a clamped override that `view_window` prefers until any
  selection movement clears it. The Search overlay's context line is
  core state (`search_context`: glyph, query, caret, pluralized live
  match count); the editor pane draws a line-number gutter with the
  current line accented.
  The gpui pane renders `highlight_body` spans — `#+` meta, drawer
  lines, and src-block content classified through the shared
  `closure_tree_sitter::Highlighter` contract (keyword tier hermetic,
  real grammars behind the `tree-sitter` feature) — plus a caret bar,
  INSERT/NORMAL chip, and the completion popup. Row TODO chips are
  clickable (`toggle-todo`), and outline rows colour by level like
  doom-vibrant's outline faces. The default theme is `doom-vibrant`
  (the user's colorscheme; `Theme::doom_vibrant`, `ColorRole` grew
  `Heading2`/`Heading3`/`Code`).
- `closure-shell-web` — self-contained single HTML bundle and
  localhost web-app (`closure serve`). `closure-wasm` (X2) is the
  client-side upgrade: a wasm-bindgen surface over the kernel
  (`reformat`/`headline_titles`) that `inline_wasm_editor` embeds into
  the export so the single HTML file re-parses edits in the browser,
  offline. Opt-in `wasm` feature; `just wasm-web-bundle` builds it; the
  default build never pulls wasm-bindgen (I10). `export_view_html` (V13)
  is the declarative export: the vault's browse `ViewTree` is embedded as
  JSON (`view_to_json`) and rebuilt client-side by an inline vanilla-JS
  renderer that sets each node's ARIA role — the same declarative `Node`
  description every other shell renders, in one self-contained file with
  no server and no toolchain.
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
  The Tauri webview hosts the LIVE editor (P4): `run` serves the vault on
  `127.0.0.1:8787` (the web shell's `respond` loop — `POST /capture`
  round-trips to the registry, I8) and loads that URL, instead of the
  read-only `export_html` snapshot. `interactive_page` is the hermetic
  proof: the served root carries the capture form + search and differs from
  the static export; the window is build-verified under `.#webview`.
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
  `<C-c> <C-x>` (tempo-style). **D6 conformance:** all five modes bind the
  *same* command set (only chords differ), proven by a matrix that every
  command yields an `Action` (hence a non-empty chord, by construction) in
  every mode; and the chord *shown* for a given (mode, command) is
  identical across three independent shell render paths — TUI
  (`render_snapshot`), web (`render_view`), and the gpui which-key
  (`App::palette_results`) — all sourced from `chord_for_command`
  (`closure-cli/tests/cross_shell_chords.rs`). A shell that hardcodes or
  diverges fails the test.
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
