# GOAL: polish the gpui reference shell (leaf queue for Grok Build)

You are working on **closure** (Rust workspace, NixOS host). The gpui
shell (`crates/closure-shell-gpui`, feature `gpui`) is the reference
GUI. Your job: work through the LEAF QUEUE below, top to bottom, one
leaf per commit. Nothing else.

## BINDING RULES — violating any of these makes the work worthless

1. **Strict 7-step TDD, every leaf, no exceptions:**
   research APIs in the actual code (never assume) → write the unit
   test(s) → run them and watch them FAIL → implement → verify (tests,
   clippy, fmt) → reflect/improve → commit. Never skip a step, never
   comment out or weaken a failing test, never lower a gate.
2. **Memory-capped builds only.** This machine's OOM killer kills the
   terminal. EVERY cargo/just invocation must be wrapped:
   `systemd-run --user --scope -p MemoryHigh=6G -p MemoryMax=8G -- nix develop -c <cmd> -j 4`
3. **The gate is** `just check` **(wrapped as above). It must be GREEN
   before every commit.** The gpui window code additionally must build:
   `... -- nix develop -c cargo build -p closure-shell-gpui --features gpui -j 4`
   with zero new warnings.
4. **Architecture split (I7/I8, see docs/spec.md):** all _behaviour_
   (state, cursors, commands) lives in `closure-shell-core` and is
   hermetically tested in `crates/closure-shell-core/tests/`. The gpui
   window only translates events and paints; it gets NO logic that a
   test cannot reach. Every mutation goes through `Shell`/`ModalApp::run`
   (registry commands) — never a shell-private write.
5. Each commit: conventional-commit message, PLUS the matching
   `docs/spec.md` revision PLUS a dated `** Decision (YYYY-MM-DD) …`
   entry under the current milestone in `ROADMAP.org` — same commit.
6. **Out of scope — do not touch:** `closure-org` (parser), CRDT/sync
   crates, protocol crates (lsp/mcp/acp/a2a), `Cargo.toml` dependency
   additions, gpui version bumps, keymap invariant tests, existing
   test deletions/weakenings, `.github`, `flake.nix`.
7. Free keybinding chords only: `crates/closure-input/tests/keymap.rs`
   enforces same-command-set across all five modes and no duplicate
   chords — if you add a command, bind it in ALL FIVE keymaps.

## LEAF QUEUE (do in order; stop when the queue is empty)

### L1 — Paint the VISUAL selection in the editor pane

State exists (`BodyEditor` mode `Visual`, private `selection()`).
Add a public accessor `ModalApp::body_selection() -> Option<(usize, usize)>`
(byte range, `None` outside Visual) with hermetic tests (select "he"
of "hello" → `Some((0,2))`-style, inclusive semantics as in
`selection()`). Then paint: in `editor_pane` (closure-shell-gpui),
spans overlapping the selection get `co.selection` background. Reuse
the caret span-splitting pattern.

### L2 — Backlinks: click a row to jump

`ModalApp` Backlinks surface: keyboard Enter jumps, mouse doesn't.
Add `pub fn backlink_click(&mut self, shell: &Shell, i: usize)` doing
exactly what Enter does (look at `on_backlinks_key`); hermetic test
(click → selected row jumps to the linking headline, surface Browse).
Wire the gpui backlink `list_row` listener to it.

### L3 — Toasts: render the shared Feedback queue in the gpui window

`closure_shell_core::Feedback` + `with_feedback` exist (G7/P6) and are
tested. Give `GpuiView` a `Feedback` field; `App`-level notifications
(command results that currently only hit the status bar: save/delete/
undo failures) push `notify(...)`. Paint toasts top-right (severity →
Error/Warning/Success colors), newest first, max 3. Hermetic part:
a `ModalApp` change is NOT needed — do not add one; only push+paint in
the window plus a small pure helper if required (test it).

### L4 — Viewport scroll decoupled from the cursor

Wheel currently moves the selection. Add `scroll: usize` offset state
to `ModalApp` with `pub fn scroll_by(&mut self, delta: i32, shell: &Shell)`
and make `view_window(page)` respect it (clamped; selection stays
visible when it moves — keep the existing keep-on-screen rule as tests
pin it). Hermetic tests first. gpui wheel handler calls `scroll_by`.

### L5 — Line numbers in the editor pane

Display-only: number gutter in `editor_pane`, `co.muted`, current line
`co.accent`. No state change, no new test needed beyond the existing
goldens staying green (gate must stay green).

### L6 — Match count in the search overlay

Search context line shows `⌕ query▏ · N matches`. The count is
`rows(shell).len()` under an active query. Hermetic test on the
context/status string source in shell-core (add a small pure fn if the
string is built in the window today; behaviour-in-core rule applies).

## Verification recap (run before EVERY commit)

```
systemd-run --user --scope -p MemoryHigh=6G -p MemoryMax=8G -- nix develop -c just check
systemd-run --user --scope -p MemoryHigh=6G -p MemoryMax=8G -- nix develop -c cargo build -p closure-shell-gpui --features gpui -j 4
```

Both green/clean, or do not commit. If a leaf turns out to require
touching an out-of-scope area, STOP that leaf, record a note in
ROADMAP.org under Decisions, and move to the next leaf.
