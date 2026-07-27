# GOAL: gpui polish queue 2 (orchestrated; protocol = goal-orchestrator.md)

Order: Q1 editor depth → Q2 fuzzy completion → Q3 agenda → Q4 LSP smoke.

## Q1 — editor depth ✅ DONE 2026-07-05 (ROADMAP: GPUI-DEPTH-Q1)

- E1 linewise VISUAL: `V` in Normal → `EditorMode::VisualLine` (new
  variant; exhaustive matches break shells at compile time — fix the
  gpui chip: "V·LINE", heading2 color). Selection = whole lines from
  anchor's line start to cursor's line end (incl. trailing newline
  when present); `y`/`d`/`x` fill a linewise register; `p` pastes below.
  Motions `j`/`k` extend; `Esc` → Normal.
- E2 counts: digits accumulate in Normal (`3dd`, `2j`, `4x`); `0` with
  an empty count is still line-home (vim rule). Count applies to
  h/j/k/l/x/dd/yy/p; cleared by Esc/any command.
- E3 editor-local undo: bounded snapshot stack (50) pushed before every
  mutating edit; `u` undoes, `C-r` redoes, in Normal. Independent of the
  vault undo (which stays bound to the outer `u` in Browse).
- E4 word motions `w`/`b` in Normal (+ Visual/VisualLine extension).

## Q2 — fuzzy completion ✅ DONE 2026-07-05 (ROADMAP: GPUI-DEPTH-Q2)

- F1 rank `body_completions` by `closure_query::fuzzy_score` against
  the prefix (desc), keywords tie-break first, then words; stable.
- F2 TAB accepts the active completion session in INSERT (session
  active → accept beats org-tempo; no session → tempo/indent as today).
  Popup shows ≤8 ranked candidates.

## Q3 — agenda in gpui ✅ DONE 2026-07-05 (ROADMAP: GPUI-DEPTH-Q3)

- A1 `agenda_context(shell, today: &str)` core state: rows grouped by
  date (sorted), each `(date, kind, title, is_today, is_overdue)`;
  hermetic tests with injected `today`.
- A2 gpui paints date group headers, SCHEDULED accent / DEADLINE error,
  today chip accented, overdue red; rows stay click-to-jump.

## Q4 — LSP smoke (user-driven finale) ✅ PREPARED 2026-07-05

Checklist + Doom eglot config: `docs/eglot-smoke.md`. User runs it on
the desktop; failures come back as leaves.
