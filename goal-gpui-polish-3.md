# goal: gpui polish 3 — user backlog 2026-07-05

Orchestrated per `goal-orchestrator.md` (Claude specs/verifies/commits, grok implements
mechanical leaves). Source: user's org TODO dump (notes.org "closure" subtree).

## Q5 — insert-mode ergonomics ✅ N1+N2 DONE 2026-07-05 (N3 pre-existing; N4 folded into Q6/TAB later)

Standard text-field behaviour the user gets everywhere on KDE, inside INSERT mode:

- **N1** `ctrl+backspace` → delete word backwards.
- **N2** `ctrl+left` / `ctrl+right` (and `alt+left/right`) → jump word backwards/forwards
  (reuses Q1-E4 `word_forward`/`word_backward`).
- **N3** readline gaps: verify `C-a`/`C-e` present (they are); add `C-w` (delete word back,
  alias of N1), `C-u` (kill to line start) if missing.
- **N4** `tab` in NORMAL mode on a heading row → cycle fold (org TAB); in INSERT at line
  start of a heading → demote, `shift+tab` promote (org M-arrows analogue).

## Q6 — structural editing & movement ✅ S1–S3 DONE 2026-07-05 (S4 multiselect deferred)

- **S1** `alt+h/l` → promote/demote heading at cursor (level −/+ 1, clamped 1..N).
- **S2** `alt+j/k` → move subtree down/up among siblings.
- **S3** `alt+enter` → insert new heading same level below; `ctrl+enter` → insert child
  heading; `ctrl+shift+enter` → new TODO heading.
- **S4** Multiselect: VISUAL-LINE selection spanning heading rows + `alt+h/j/k/l` applies
  promote/demote/move to every selected heading (batch through command registry, I8).

## Q7 — completion & highlighting depth

- **C1** dabbrev scope → whole file (all rows' bodies + titles), not just current buffer.
  `C-n`/`C-p` cycle (vim-style) in INSERT.
- **C2** completion popup auto-opens after typing delay (≥3 word chars, debounce; hermetic
  core exposes `completion_should_popup(elapsed_ms)`, gpui drives the timer).
- **C3** syntax highlighting for body text in the *read-only* rows pane (reuse
  `highlight_body` spans when rendering non-edited body preview).

## Q8 — M-x command palette ✅ DONE 2026-07-05 (ROADMAP: GPUI-MX)

- **M1** core: `palette_open()` listing every registry command with its mapped keybinding
  (from closure-input keymap — I4: single source of truth), fuzzy-filtered
  (closure-query::fuzzy_score), enter runs via `run_command` (I8).
- **M2** gpui: `alt+x` / `M-x` opens centred popup, doom-vibrant styled, shows
  `command-name    key` rows, arrows/C-n/C-p navigate.

## Q9 — which-key popup ✅ DONE 2026-07-05 (ROADMAP: GPUI-WHICHKEY)

- **W1** core: `which_key_groups()` → grouped, column-ready entries (key, label, group)
  from the live keymap (I4), replacing flat bottom strip.
- **W2** gpui: bottom popup panel after leader-key delay, multi-column like Doom's
  (screenshot reference), doom-vibrant chips.

## Q10 — mouse & interaction depth

- **D1** drag-and-drop row reordering (drag heading row → drop between siblings →
  `move_subtree` command; I8).
- **D2** better mouse: double-click word select, drag to select in body editor,
  click sets cursor (verify existing), wheel in body editor scrolls body.

## Q11 — undo visualization

- **U1** editor-local undo already linear (`u`/`C-r`, Q1-E3). Add `undo_tree_context()`
  (list of snapshots w/ cursor + preview line) + M-x command `undo-history` rendering a
  vertical list popup; picking one restores. (Full branching tree = later milestone.)

## Answered questions (no code)

- **Release build**: `systemd-run --user --scope -p MemoryHigh=6G -p MemoryMax=8G -- nix develop -c cargo build --release -p closure-cli --features gpui -j 4`; binary at `target/release/closure`. Justfile recipe `run-gpui-release` to add.
- **Web**: `nix develop -c just wasm-web-bundle` → `target/wasm-web/editor.html` (self-contained). **Tauri**: `nix develop .#webview -c just run-tauri VAULT`.
- **doomemacs-core harvest**: `/home/wap/dev/doomemacs-core` is the doom *core* only — no
  `modules/config/default/+evil-bindings.el` there. Canonical org/evil bindings taken from
  upstream doomemacs knowledge (M-h/l promote/demote, M-j/k move subtree, M-RET heading,
  C-RET child, TAB cycle, zc/zo/za folds, gw/ge words) and baked into Q5/Q6 specs directly.
- **undo-tree**: see Q11.

## Order

Q5 → Q6 → Q8 → Q9 → Q7 → Q10 → Q11. Each queue: SPEC → RED (grok) → GREEN (grok) →
gate (capped) → review → commit. Escalate to Claude: S4 batch semantics, D1 hit-testing,
anything touching parser/CRDT.
