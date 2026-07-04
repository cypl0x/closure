# GOAL: Fable-orchestrated polish loop (Claude Code = architect/verifier, Grok = implementer)

This goal is executed by the **Claude Code (Fable 5) session itself**.
Claude does NOT write feature code in this loop. Claude plans, dispatches,
verifies, repairs-by-redispatch, reviews, and commits. Grok Build burns
the implementation tokens.

## Economic contract (why this shape)

- Fable tokens are scarce (Claude Pro quota): spend them ONLY on
  (a) leaf specification, (b) reading diffs, (c) judging test output,
  (d) small surgical fixes when redispatch would cost more than fixing.
- Grok tokens are free to the user: spend them on ALL bulk code/test
  writing, mechanical refactors, and first-draft repairs.
- Compute (builds/tests) is local: Claude runs every build itself,
  memory-capped — Grok is never allowed to run builds (OOM + trust).

## Roles

**Claude (this session) — the senior engineer:**
1. Owns the leaf queue (below) and the architecture invariants
   (docs/spec.md I1–I10). Splits/reorders leaves when reality demands.
2. Writes each dispatch prompt: exact files, exact API names verified
   against the current code (grep first — never let Grok guess),
   the RED test expectations, the out-of-scope fence.
3. Runs all verification (capped): targeted tests → `just check` →
   `cargo build -p closure-shell-gpui --features gpui`.
4. Reviews every Grok diff (`git diff`) for: invariant violations,
   test weakening, scope creep, unwrap/expect, hidden behaviour in the
   window layer. Reverts ruthlessly (`git checkout -- <file>`); a bad
   diff is redispatched with the review findings, not hand-fixed,
   unless the fix is < ~10 lines.
5. Commits (conventional message + spec.md revision + dated ROADMAP
   Decision — Claude writes these three, never Grok).
6. Maintains the session ledger: after each leaf, one status line
   (leaf, dispatches used, verdict).

**Grok (headless CLI) — the implementer:**
- Invoked per work item, non-interactive, from the repo root:
  `nix run github:numtide/llm-agents.nix#grok -- --always-approve -p "<DISPATCH>"`
  (run via Bash with `run_in_background: true`; a dispatch that takes
  longer than ~10 min is presumed wedged — kill and redispatch tighter).
- May ONLY edit files named in the dispatch. May not run cargo/nix/just.
- Gets one job per dispatch: either "write these failing tests" or
  "make exactly these tests pass" — never both in one call.

## The loop (per leaf)

```
SPEC     Claude greps the code, pins exact symbols/lines, writes the
         RED test list (names + assertions) for the leaf.
RED      Dispatch #1 → Grok appends the tests verbatim-faithful to the
         spec. Claude runs the targeted test file: MUST fail to compile
         or fail assertions. If it passes, the spec was vacuous — fix spec.
GREEN    Dispatch #2 → Grok implements against the named APIs only.
         Claude runs targeted tests.
REPAIR   On failure: up to 2 redispatches, each quoting the exact
         compiler/test output. After 2 failures Claude either fixes
         surgically (small) or re-splits the leaf (large). Never ship red.
GATE     Claude runs, memory-capped:
           systemd-run --user --scope -p MemoryHigh=6G -p MemoryMax=8G \
             -- nix develop -c just check
           …and the gpui feature build (same wrapper, -j 4). Zero new
           warnings allowed.
REVIEW   Claude reads the full diff. Checklist: I5 (no unwrap/expect),
         I7 (no logic in the window), I8 (mutations via Shell/run),
         I4 (new commands bound in all five keymaps), tests untouched
         except additions, no dependency changes, docs on new pub items.
COMMIT   Claude writes spec.md + ROADMAP Decision + conventional commit.
LEDGER   One status line to the user.
```

## Parallelism policy

- Default: ONE leaf in flight. Grok dispatches run in the background;
  Claude uses the wait time for the next leaf's SPEC step (pipelining,
  not parallelism — this is where the real speedup lives).
- Two Grok processes at once are allowed ONLY when both leaves are
  file-disjoint (e.g. a code leaf + a docs/test-fixture leaf), because
  both edit the same working tree. Never two leaves touching
  closure-shell-core simultaneously.
- Claude subagents are NOT used for implementation (they spend the
  same scarce quota); at most one Explore subagent when a SPEC needs a
  broad code search Claude cannot answer with 2–3 greps.
- Builds are strictly serialized (one capped scope at a time).

## Escalation rules (when Claude implements directly)

- The leaf touches parser (closure-org), CRDT/sync, eval/security, or
  any invariant's enforcement mechanism → Claude-only, no dispatch.
- Two REPAIR rounds failed and the fix is architectural.
- The change is smaller than the dispatch prompt would be.

## Leaf queue

Work `goal-gpui-polish.md` L1–L6 in order under this protocol. After
L6, propose the next queue to the user (candidates: VISUAL linewise
`V`, editor local undo, fuzzy-ranked completion popup, `closure lsp`
smoke against a real editor, agenda view in gpui) — do not start it
without their pick.

## Standing constraints (inherited, non-negotiable)

- 7-step TDD is realized BY this loop (SPEC/RED = steps 1–3,
  GREEN/REPAIR = 4, GATE = 5, REVIEW = 6, COMMIT = 7). No leaf skips a
  stage, no gate is ever weakened, failing tests are never deleted.
- Every cargo/just/nix build wrapped in the systemd-run memory cap.
- Grok never sees: Cargo.toml, flake.nix, keymap invariant tests,
  .github, fixtures corpora.
```
