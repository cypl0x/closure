# goal: gpui daily-driver — close the gaps that cost time every day

Scope: `closure-shell-gpui` + the hermetic surface it renders (`closure-shell-core`,
plus registry commands in `closure-core` where a verb is missing). Ordered by
productivity/QoL per unit of work, not by subsystem tidiness.

Rules carried over from `goal-orchestrator.md`:

- Hermetic core first. Every item lands as tested state/logic in `closure-shell-core`
  (no window), then a thin gpui paint/event translation. A gpui-only item is a spec bug.
- I4: no chord is hardcoded in the shell. New verbs are registry commands with keymap
  entries per input mode; palette/which-key/`describe-key` read the registry.
- I8: every mutation goes through a registered command. Nothing gets `&mut Document`.
- I3: every mutation is one undo unit with a name a human recognises.
- TDD loop mandatory (research → RED → GREEN → gate → reflect → conventional commit).
- Gates per queue: `nix develop -c just check`, `just coverage` (floor 84, ratchet if it
  moves), `treefmt`. Heavy release builds wrapped in
  `systemd-run --user --scope -p MemoryMax=8G ... -j 4`.

Baseline (2026-07-28): 6.4k LOC gpui + 29k shell-core; 26 `ModalSurface`s; vim/doom
grammar, folds, tables, surround, dabbrev, palette, which-key, undo-tree pane, images,
sync/graph/journal/cron/llm panes. Missing = everything below.

---

## Q1 — buffers, jumps, recents (the single biggest daily loss)

Today the shell opens one file at a time and remembers only the last session. Every
cross-note movement is a fresh search.

- **B1 buffer list.** `open_buffers: Vec<BufferHandle>` in core (path, cursor, dirty,
  view mode, fold state). Opening a file pushes/reuses; closing pops. `buffer_list()`
  returns MRU-ordered rows. Commands: `buffer-next`, `buffer-prev`, `buffer-switch`
  (fuzzy picker over `buffer_list()`, reuses `fuzzy_score`), `buffer-close`,
  `buffer-close-others`.
- **B2 alternate buffer.** `alternate()` = previous buffer; command `buffer-alternate`.
  Doom `SPC ,`/`SPC ``; vim `C-^`; emacs `C-x b`.
- **B3 jumplist.** Ring of `(path, BlockId, cursor)` pushed on every non-local jump
  (search hit, link follow, backlink, agenda jump, palette open-file). `jump_back()` /
  `jump_forward()`; vim `C-o`/`C-i`, doom `SPC s b`-style entries in which-key.
  Property test: any jump sequence + N backs returns to the N-th prior position exactly.
- **B4 recent files.** Persisted MRU (already have session persistence — extend that
  file, no new format): `recent-files` picker, doom `SPC f r`.
- **B5 gpui tab strip.** Optional-by-config strip above the buffer showing open buffers,
  dirty dot, click to switch, middle-click to close. Rendering is a `ViewTree` node so
  tui/web get it free.

DoD: switch between 5 notes without touching search; `C-o` walks back through every jump
made in the session; tab strip golden-tested in `paint.rs`.

## Q2 — window splits

One buffer filling the window is wrong for a PKM: you read one note while writing another.

- **W1 layout tree** in core: `Layout::{Leaf(BufferHandle), Split{dir, ratio, a, b}}`,
  focused leaf id. Commands `split-right`, `split-below`, `window-close`,
  `window-only`, `window-focus-{left,down,up,right}`, `window-balance`,
  `window-resize-{wider,narrower}`. Vim `C-w s/v/c/o/hjkl`, doom `SPC w …`.
- **W2 per-leaf independent state**: cursor, viewport, fold set, mode. Two leaves on the
  _same_ file share the document and see each other's edits (same `Vault`), not the
  cursor.
- **W3 gpui paint**: recursive layout with draggable splitters; focused leaf border in
  the doom-vibrant accent. Hit-testing tested headlessly via the layout rects.
- **W4 outline+buffer default**: `Clickable` view becomes a preset layout (outline leaf
  left, buffer right) rather than a special mode — one code path.

DoD: `SPC w v`, edit right, `C-w h`, edit left, both write; ratio survives resize.

## Q3 — the org work-verbs that are missing

These are the actual daily verbs of org use. All exist in Emacs org; none exist here.

- **V1 refile** (`org-refile`): pick target headline via fuzzy picker over the whole
  vault (title + file path + ancestor breadcrumb), move subtree there as last child.
  Cross-file. One undo unit. Registry command `refile-subtree`. Doom `SPC m r`.
- **V2 archive** (`org-archive-subtree`): move subtree to `<file>_archive.org` under the
  same headline path, stamp `:ARCHIVE_TIME:`; and `archive-toggle-tag` (`:ARCHIVE:`).
  Respect `is_archived` in every query already using it.
- **V3 clock**: `clock-in` / `clock-out` / `clock-cancel` / `clock-goto`, writing real
  `CLOCK:` lines into the LOGBOOK drawer (parser already classifies clocking).
  Running clock shown in the status bar with elapsed time, live. `clock-report` =
  totals per headline for a day/week (reuse the existing clocking semantics).
- **V4 timestamps with a picker**: `schedule` / `deadline` / `timestamp-insert` open a
  calendar popup (month grid, arrows move day, `.`=today, `S-arrows` ±week, type a date
  string too). Repeaters (`+1w`, `.+1d`, `++1m`) parseable and rendered. Removing =
  `schedule` with empty input. This is the highest-friction missing piece for task work.
- **V5 TODO/priority/checkbox cycling**: `todo-cycle` forward/back over the configured
  keyword list (config already has `todo_keywords`), logging `- State "DONE" from "TODO"
[ts]` when configured; `priority-up`/`priority-down`; `checkbox-toggle` on `- [ ]`
  items with `[/]`/`[%]` cookie recompute up the list tree.
- **V6 tag picker**: fuzzy over tags already in the vault, multi-select, writes the tag
  line (today: free-text `TagsEdit` buffer only).

DoD: a full capture → schedule → clock in → clock out → mark DONE → refile → archive
round trip, done from the keyboard, with each step undoable and byte-exact on disk.

## Q4 — reading long prose

- **P1 soft wrap.** Today: explicit "lines do not wrap" (`lib.rs:809`), because the
  viewport math is one-number-per-line. Introduce a visual-line model in core:
  `wrap_lines(text, cols) -> Vec<VisualLine{logical, start, end}>`, and make cursor
  motions/viewport/scroll operate on visual lines while all editing stays logical.
  `j`/`k` move visual lines, `gj`/`gk` logical (vim-faithful). Config `soft_wrap` +
  `wrap_column`. Property test: wrapping is a partition — concatenating visual lines
  reproduces the logical line byte-exactly.
- **P2 breadcrumb / sticky header.** Top strip showing the ancestor path of the cursor's
  headline; sticky pinned heading row while scrolling inside a subtree. Core:
  `ancestor_path(BlockId) -> Vec<String>`.
- **P3 narrow to subtree** (`org-narrow-to-subtree`, `SPC n`): buffer shows one subtree;
  widen restores. Narrowing is a view state, never a document mutation.
- **P4 reading typography pass**: heading scale already exists; add configurable content
  width (centred column), paragraph spacing, and list-indent guides.

## Q5 — search and replace

- **S1 in-buffer replace**: `:%s/foo/bar/g` and doom `SPC s r` with live match count and
  highlight-all; each replace-all is one undo unit.
- **S2 vault-wide replace**: results pane grouped by file, per-hit checkbox, apply
  writes through commands file-by-file, one undo unit per document.
- **S3 highlight all + `n`/`N`** for the last search across buffers; `*`/`#` search word
  under cursor.
- **S4 search scoping**: current buffer / current subtree / vault, remembered per query.

## Q6 — the file on disk is not only ours

Org compatibility means Emacs edits the same file while the window is open.

- **X1 file watch** (`notify` crate, feature-gated like other non-hermetic deps): on
  external change, if the buffer is clean → reload silently, preserving cursor by
  `BlockId`; if dirty → non-blocking conflict bar offering keep-mine / take-theirs /
  diff (reuse the CRDT conflict pane).
- **X2 crash safety**: periodic autosave of dirty buffers to `.closure/autosave/`, and
  recovery offer on next start. Never write the user's file without an explicit save.
- **X3 save semantics**: atomic write (temp + rename), preserve mode bits, `:w`/`C-s`
  write, `:wa`. Status bar shows saved/dirty/external-change state.

## Q7 — literate use inside the buffer

- **L1 run block from the buffer** (`C-c C-c`) with `#+RESULTS:` write-back inline, a
  spinner while running, and cancel. Backend + write-back already exist; only the
  in-buffer path and progress are missing.
- **L2 render results inline**: tables as tables, images as images (image support exists
  — extend to `#+RESULTS:` output), long output folded with a "N more lines" chip.
- **L3 block affordances**: language chip, run/edit-special/copy buttons on hover,
  `org-edit-special` already there — make it reachable from the mouse too.
- **L4 table editing polish**: auto-realign on leave, `S-TAB` back-field, row/column
  move, formula row (`#+TBLFM:`) evaluated through `closure-eval`.

## Q8 — self-documentation (Emacs-grade discoverability)

- **D1 `describe-key`** (`SPC h k`): press a chord, get command name, doc, source keymap.
- **D2 `describe-command`** (`SPC h c`) and `describe-mode`: registry doc strings
  rendered in a pane; every command gets a one-line doc (registry field, enforced by a
  test that fails on an empty doc).
- **D3 generated manual**: a buffer built from the registry + config schema at runtime —
  the vault's own `closure.org` help file, always true because it is generated.
- **D4 tutorial in gpui**: the existing tutorial text becomes an interactive pane
  (`SPC h t`) that follows the active input mode.
- **D5 which-key completeness**: every pane/surface reachable by a documented chord —
  today several panes are palette-only.

## Q9 — appearance and settings, live

- **A1 font size / zoom**: `C-+` / `C--` / `C-0`, persisted; affects the buffer, not the
  chrome (and a separate chrome scale).
- **A2 theme switch live** (`SPC t t`), including a light doom variant; theme read from
  config, changeable without restart.
- **A3 settings pane**: typed config rendered as an editable form; writes back to the
  vault's `config.org` block; invalid input rejected at edit time (I9), not at reload.
- **A4 input-mode switch live** (`SPC t m`) between vim/doom/emacs/helix/notion, with the
  which-key popup reflecting it immediately.

## Q10 — batch and repeat

- **R1 multi-select in the outline** (the deferred S4): VISUAL-LINE over heading rows,
  then promote/demote/move/refile/archive/todo-cycle apply to every selected row as one
  undo unit.
- **R2 macros**: `q<reg>` record, `q` stop, `@<reg>` / `@@` replay, `<count>@` — replay
  is a command sequence through the registry, so it is journalable and undoable.
- **R3 named registers**: `"a y`, `"a p`, `:reg` listing; system clipboard as `"+`.

## Q11 — agenda that you can work from

- **G1 interactive rows**: `t` cycles TODO, `S`/`D` reschedule/deadline via the Q3-V4
  picker, `RET` jumps to source (pushes the Q1 jumplist), `r` refiles, `z` clock-in.
- **G2 day/week/month spans**, `f`/`b` to move span, `.` today, filter by tag/priority.
- **G3 sticky agenda**: recomputed incrementally on vault change, not rebuilt per key.

## Q12 — databases you can edit

- **T1 cell edit** in `DbView` → property/title/tag write through commands.
- **T2 view builder UI**: add/remove column, filter, sort from the pane; the result is
  written back as the `#+BEGIN: closure-view` params it came from (round-trips).
- **T3 group-by / board layout** over one column (the kanban shape), rows draggable
  between groups → property write.

## Q13 — it stays fast when the vault is big

- **N1 async vault load/save** off the UI thread with a progress toast; window is usable
  while loading.
- **N2 incremental reparse**: only the edited document reparses per keystroke (a perf
  commit exists — verify with a bench, then hold the line with a regression test).
- **N3 virtualised rendering** for outline/search/db panes (render the visible window
  only) — needed once a vault has 10k+ headlines.
- **N4 budget test**: keystroke → paint under the recorded dispatch budget on the seeded
  10k-note vault; fails CI if exceeded.

---

## Order

Q1 → Q3 → Q2 → Q5 → Q6 → Q4 → Q8 → Q7 → Q9 → Q11 → Q10 → Q13 → Q12.

Rationale: buffers/jumps (Q1) and the org verbs (Q3) are used dozens of times an hour and
are wholly absent; splits (Q2) unlock two-note work; replace (Q5) and external-change
safety (Q6) are the next friction/risk pair; wrap (Q4) is a big correctness-shaped change
so it follows the cheap wins; discoverability (Q8) compounds every later queue.

## Not in this goal

Mobile/touch shells, relations/rollups (kernel work, separate goal), real MCP framing,
sniffer capture, text-CRDT for live typing collaboration, multi-cursor. Parser/CRDT
changes escalate out of this goal rather than being made inside it.

## Per-queue definition of done

1. Core state/logic unit-tested without a window; property test where an invariant exists.
2. gpui paint covered by a `paint.rs` golden or an `interaction.rs` event test.
3. Every new verb is a registry command with a doc string, a per-mode chord, and a
   palette entry — no exceptions (I4/I8).
4. `just check`, `just coverage`, `treefmt` green; `ROADMAP.org` entry flipped to DONE
   with the decisions recorded; conventional commit.
