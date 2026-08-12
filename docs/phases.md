# closure — delivery phases

Phases are sequenced so each ships a usable product. No phase introduces a
hook that earlier phases didn't already define. Every invariant in
`spec.md` must be green at every phase boundary.

| Phase | Ships                                                                 | Acceptance                                                                    | Invariants gated on  |
| ----- | --------------------------------------------------------------------- | ----------------------------------------------------------------------------- | -------------------- |
| M0    | Bootstrap: flake, workspace, CI, vision-scope spec, fixtures.         | `nix flake check` green; `cargo check --workspace` green.                     | I10                  |
| M1    | `closure-org` parser + printer (core subset), fuzz, golden corpus.    | All fixtures roundtrip; proptests at `cases=2048`; fuzz 60s clean.            | I1, I5, I6           |
| M2    | `closure-core` kernel: `Document`, `Command`, registry, `Edit`, undo. | Command registry green; proptest `apply + undo == identity`.                  | I2, I3, I4           |
| M3    | `closure-store` + `closure-query` + `closure-tui` + `closure-cli`.    | First user-visible release; `closure tui` opens a vault.                      | I7                   |
| M4    | `closure-config` + `closure-input` + `closure-whichkey`.              | Typed config loads; all 5 input modes usable end-to-end.                      | I9                   |
| M5    | `closure-eval` + first evaluator backend (shell / python / wasm).     | Org code-block executes in a sandboxed backend; Coda-style formulas evaluate. | I8                   |
| M6    | `closure-crdt` + `closure-sync` file/git.                             | Two vaults on one host merge without rebasing.                                | I2 holds under merge |
| M7    | `closure-llm` + `closure-mcp`.                                        | LLM mutates via commands only; MCP server exposes registry.                   | I8                   |
| M8    | `closure-shell-egui` + `closure-shell-web`.                           | Same vault opens in TUI, egui, web; edits roundtrip across.                   | I7                   |
| M9    | `closure-sync` P2P + collaboration.                                   | Two hosts sync without central server.                                        | I2, I3               |
| M10+  | Remaining shells, `closure-sniffer`, `closure-plugin-host`, LSP/ACP.  | Each ships independently; kernel API unchanged.                               | I7, I8               |

| M11 | Kernel + gpui depth: composition with arguments, databases that group and relate, org conformance published as a rate. | `closure conformance` reports its number; `just gates` green with no warnings. | I11, I12 |

Spec freezes at the close of each phase. Breaking an earlier phase's
acceptance forces either a fix or an explicit spec revision in the same
commit.

M11 is where I11 and I12 arrive, and they are of a different kind from
the ten before them. I1–I10 were decided before there was code to hold
them to; these two were decided because the code had grown a question
nobody had answered — what a block is, and how slow is too slow. That
is the honest order for an invariant that is about a system rather than
about a plan, and `closure-cli/tests/architecture_is_true.rs` now
checks that every invariant this table gates on is one the spec
actually defines.
