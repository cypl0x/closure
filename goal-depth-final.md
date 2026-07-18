# goal: depth-final — close every recorded gap, finish every shallow tier

Orchestrated per `goal-orchestrator.md` (Claude specs/verifies/commits, Grok implements
mechanical leaves; parser/CRDT/sync/eval leaves are Claude-only per escalation rules).
Source: full vision-vs-spec gap audit 2026-07-18 (Tier 2 recorded gaps + Tier 3 true gaps).

Standing rule for every queue: finish = tests green under the capped gate, spec.md revised
in the same commit, dated ROADMAP Decision, no invariant weakened. 7-step TDD via the
orchestrator loop. All builds memory-capped (`systemd-run --user --scope -p MemoryHigh=6G
-p MemoryMax=8G -- nix develop -c ... -j 4`).

## Q1 — gpui interaction depth (daily-use pain, grok-friendly)

The reference shell's remaining recorded mouse/editor gaps. Hermetic seams in
`closure-shell-core`, thin display-bound paint in `closure-shell-gpui`.

- **G1** in-word click column: `body_click` resolves the exact grapheme column inside the
  clicked word (per-glyph x-advance seam exposed from core as a pure function over the
  line text + a monospace/width callback; gpui supplies real glyph widths).
- **G2** drag text selection: mouse-down sets anchor, drag extends a charwise VISUAL
  selection (`body_drag(from, to)` pure in core; reuses `body_selection()` exclusive-range
  contract so the renderer paints unchanged).
- **G3** drag-and-drop row reordering in the window: wire the existing `DragReorder`
  gesture machine (G5c) to gpui mouse events; drop dispatches `move-subtree-up/down`
  through the registry (I8). Out of scope: cross-file moves.
- **G4** INSERT-burst undo: entering INSERT opens one editor-undo checkpoint; leaving
  INSERT (Esc/commit) closes it, so `u` in NORMAL undoes the whole burst (vim rule).
  Snapshot stacks stay bounded at 50.
- **G5** wheel scroll inside the body editor pane (viewport override like the outline's
  `scroll_by`, selection movement clears it).

## Q2 — kernel path-walking undo + undo-tree node jump  [CLAUDE-ONLY: I3 enforcement]

The recorded UndoHistory gap: the surface is read-only because the kernel cannot walk to
an arbitrary node.

- **U1** `closure-undo`: `path_between(from, to) -> Vec<Step>` (Step = Undo(edit) |
  Redo(edit)) over the branching tree; property test: walking the path from any node to
  any node then back reproduces the starting `Document` byte-exact.
- **U2** `closure-core`: `Command::JumpToUndoNode(id)` applies the path via the existing
  apply/undo primitives only (no new mutation surface, I8); itself appended to the tree
  so the jump is undoable (I3).
- **U3** shells: UndoHistory surface rows become actionable — Enter/click on a node jumps
  (registry command, chord in all five keymaps, I4). gpui + TUI render.

## Q3 — CRDT title 3-way merge  [CLAUDE-ONLY: CRDT]

Kill the last silent-loss point: concurrent title edits resolve LWW today.

- **T1** title register upgraded: keep LWW as the automatic default, but when both sides
  changed the title divergently from the causal base, record a `FieldConflict` (the V9
  machinery already models this for bodies) instead of discarding the loser silently.
- **T2** `ConflictApp` (V9b) lists title conflicts alongside body conflicts;
  resolve-ours/resolve-theirs route through rename (I3/I8). Convergence property tests:
  both replicas surface the identical conflict set regardless of merge order (I6).

## Q4 — markdown to full depth  [CLAUDE-LED: parser; Grok may write RED corpora]

- **M1** inline markup spans: emphasis, strong, code spans, links — classification only,
  span-preserving, roundtrip untouched by construction (I1); fuzz extended.
- **M2** setext headings classified (block classifier grows two-line lookahead; I1 holds —
  prove with new golden fixtures under `fixtures/md/`).
- **M3** md backlinks: `[](path.md#heading)` and wiki-style `[[target]]` resolve through
  `closure-query` backlink index read-only (no `:ID:` invention in md files — Decision:
  md identity = path+slug, org identity = ULID; the bridge maps between them).

## Q5 — org semantic zones  [CLAUDE-ONLY: parser + eval]

The remaining "preserved as opaque text" list becomes semantic, one construct at a time.
Verbatim roundtrip stays the invariant; semantics are accessors + commands over the
existing nodes (the D9 tables pattern).

- **O1** `#+RESULTS:` write-back: `closure-eval` output replaces/creates the RESULTS
  block under its src block via a registry command (undoable I3); re-eval is idempotent;
  golden fixtures for value/output/silent header args (subset: `:results value|output|silent`).
- **O2** footnotes: definition/reference accessors + dead-footnote diagnostic in
  `closure-lsp` (reuse the L3 pull path).
- **O3** clocking: `CLOCK:` line accessor (start/end/duration), `Vault::clocked(range)`
  query, `closure clock report` CLI table. Clock-in/out commands (registry, timestamps
  through the existing timestamp printer, I1).
- **O4** export markup (`#+LATEX:`/`#+HTML:` etc.): stays verbatim — record the explicit
  scope Decision (export engines out of scope for the kernel; `to_org`/HTML export cover
  the vision's export want). No code, spec text only.

## Q6 — web tier becomes a full interactive editor

`WEB`/`TAURI` graduate from capture-form to the `INTERACTIVE_EDITOR_CAPABILITIES` bar.

- **W1** `closure serve` gains a JSON command endpoint: `POST /command {name, args}` →
  registry dispatch (I8) → fresh `view_to_json` reply. Auth: loopback-only bind + a
  per-session token in the page (Decision text; not a security boundary beyond localhost).
- **W2** the served page's inline JS renders the ViewTree JSON (the V13 renderer already
  exists) and maps keydown through a shipped keymap table (`chord_for_command` export so
  the chords stay honest, I4/D6 — the cross-shell chord test grows a web column).
- **W3** capability matrix flips: `WEB`/`TAURI` assert the interactive-editor bar in
  `closure shells`; hermetic proof = drive `respond` loop with a scripted command
  sequence and assert the vault mutated + view changed (the D4 pattern, no browser).
- **W4** wasm tier: `closure-wasm` grows `dispatch_command` so the single-HTML export
  edits offline through the same registry (feature-gated as today, I10).

## Q7 — LLM live tier finished

- **L1** per-model providers: `ollama_http(host, model)` body builder takes the model
  (boxed closure, the recorded deferral); add `anthropic()` + `openai()` builders over
  `HttpProvider` with the canonical wire shapes, mock-server tested (hermetic).
- **L2** `just llm-live` opt-in gate: end-to-end ask against a real local Ollama if the
  daemon answers on localhost, gracefully skipped otherwise (the iroh-gate pattern).
- **L3** key handling: `llm_key_env` per provider in typed config (I9), never logged,
  never persisted to the vault. Record-everything (`closure-record`) redacts prompts
  when `record_llm = false` (config default true, documented).

## Q8 — package ecosystem made real

- **P1** lockfile hashes upgrade FNV-1a → BLAKE3 (`Cid` already in-tree; lockfile format
  version bumps, old lockfiles re-resolve with a clear error — Decision + migration note).
- **P2** network registry source behind a `net-registry` feature: a registry is a URL
  serving the same `*.org` package files (`fetch` = curl-shell-out like `SystemClipboard`,
  no new deps in default build, I10). Hermetic tests over a local dir; `just pkg-net`
  gate spins a localhost static server.
- **P3** three real example packages in-repo under `fixtures/registry/` (a widget pack,
  a formula pack, a capture-template pack) — installable end-to-end, doubling as the
  golden corpus for resolve/lock.

## Q9 — record replay

- **R1** `closure history --replay <n>`: re-applies journal entries 1..n onto a fresh
  vault copy through the registry only (I8); property: replay of the full journal
  reproduces the current vault byte-exact (I1/I6) — this is the honesty test that the
  journal really captures everything.
- **R2** `--replay` dry-run mode prints the command sequence (self-doc tie-in).

## Q10 — P2P over the real internet  [CLAUDE-ONLY: sync]

- **N1** `iroh` feature crate (`closure-sync/iroh` or `closure-sync-iroh`): native iroh
  endpoint (not the external binary) carrying the existing signed+encrypted
  `SyncMessage` frames — the framing was designed for this drop-in. Default build never
  compiles it (I10).
- **N2** discovery: iroh node tickets as the pairing artifact (`closure sync ticket` /
  `closure sync join <ticket>`); ticket = plain text, storable in the vault.
- **N3** `just sync-iroh`: two-process same-host test through the iroh relay path
  (exercises NAT-traversal machinery without needing two hosts); a documented manual
  two-host checklist covers the rest. Honest recording: true cross-NAT CI is out of
  hermetic reach — Decision text says so.

## Q11 — live collaboration  [CLAUDE-ONLY: sync/CRDT]

Async merge exists; this adds the continuous session.

- **C1** session loop: `SyncSession::stream` exchanges ops continuously over the existing
  secure channel (poll/push per edit, not per whole-replica); hermetic loopback test:
  two peers typing interleaved converge live, block ids stable (I2).
- **C2** presence: a `Presence` CRDT-adjacent ephemeral map (peer → focused BlockId +
  cursor) — explicitly NOT persisted, NOT in the undo tree; carried as a distinct frame
  type so `apply_message` ignores it for state.
- **C3** shells: `Node::Widget`-tier presence rendering — remote-peer cursor/row badge in
  the ViewTree (new `RowView` badge, empty default so goldens hold); gpui paints it.

## Q12 — performance made measurable

The vision claims "almost no input lag"; make it a number with a gate.

- **B1** vault generator: `closure-spec` gains `gen_vault(files, headlines_per_file)`
  (deterministic, seeded, I6) — the 10k-file fixture without committing 10k files.
- **B2** criterion benches (dev-dep, bench profile only): parse, print, `all_headlines`,
  fuzzy query, backlink index build on the generated 10k vault. `just bench` +
  committed baseline JSON; regression = human-reviewed diff, not a hard gate (perf CI
  on shared hardware lies — Decision).
- **B3** input-latency harness: time `App::dispatch` (the one shared input seam, P1)
  per-key over a scripted 1k-keystroke session on the 10k vault; assert p99 under a
  budget (start honest: measure, then pin). This is hermetic — the display swap is the
  embedder's, but the kernel path is ours to bound.
- **B4** fix what B2/B3 expose, largest first (index reuse, avoided reparses, query
  memoization). Scope-fenced: no architectural change without a spec revision.

## Q13 — Slint shell bodied

- **S1** `closure-shell-slint` follows the gtk/qt recipe exactly: hermetic
  `slint_view(&Node) -> String` (a `.slint` document) exhaustive over every `NodeKind`
  (compile-error on new kinds), edits through shared `Shell`/`dispatch` (I8), G8 golden
  gains a fifth column, feature-gated window build under `.#webview` (I10).
  This also closes the vision's "declarative/type-level UI (Slint?)" question with
  working code on both answers: our ViewTree AND a Slint renderer of it.

## Q14 — display-bound verification (best-effort, honest)

- **V1** xvfb screenshot gate `just gui-shot`: launch gpui window on a temp vault under
  a virtual display, capture a PNG, compare structural properties (non-blank, expected
  dimensions, theme background colour at corners) — NOT pixel-perfect diffing (font
  rendering varies). Opt-in, non-hermetic, documented.
- **V2** manual smoke checklist `docs/gui-smoke.md`: one page, ten steps, run before any
  release tag. Decision records this as the accepted residual.

## Q15 — mobile tier

- **P1** PWA: web export gains a manifest + offline service worker (inline, no toolchain)
  so the served/exported page installs on a phone home screen and works offline against
  the wasm tier (Q6-W4). Hermetic test: manifest present + valid JSON structure.
- **P2** Flutter stays the external path — refresh `docs/flutter-shell.md` against the
  Q6 command endpoint (it can now edit, not just view). No workspace crate (I10 Decision
  stands).

## Q16 — closure: coverage, spec, matrix

- **F1** coverage ratchet: after Q1–Q15, re-measure; raise the floor to whatever the new
  hermetic-reachable number is (expect 85+ once web-tier editing and undo-walk land).
- **F2** spec.md: every queue's Decision folded in; the "recorded gaps" inventory in
  ROADMAP updated — a gap is either CLOSED (points at the queue) or RESIDUAL (points at
  the Decision that accepts it). No unlabeled gaps remain.
- **F3** `closure shells` + `closure ui-matrix` reflect the final capability truth
  (Slint column, web interactive tier, presence badge kind).

## Order

Q1 → Q2 → Q3 → Q6 → Q4 → Q5 → Q9 → Q7 → Q8 → Q12 → Q10 → Q11 → Q13 → Q15 → Q14 → Q16.

Rationale: daily-use editor pain first (Q1–Q2), last silent data loss second (Q3), then
the biggest capability jump (Q6 web editing) which Q15/Q11 build on; parser depth (Q4–Q5)
before things that consume it (Q9 replay needs RESULTS write-back semantics stable);
perf (Q12) before P2P/collab so regressions are visible; display verification (Q14) after
all windows stop changing; Q16 seals.

Escalation per orchestrator: Q2, Q3, Q5, Q10, Q11 are Claude-only (parser/CRDT/sync/undo
enforcement). Q1, Q6–Q9, Q12-B1/B2, Q13, Q15 are Grok-dispatchable leaves. Q12-B4 and
Q14 are Claude-led. Grok never sees: Cargo.toml, flake.nix, keymap invariant tests,
fixtures corpora, .github.
