# closure — architecture

One-page layer diagram + the three decisions that would force a v1.0
break.

## Layer diagram

```
 L6 Input modes    closure-input  closure-whichkey
 ─────────────────────────────────────────────────────────────────────
 L5 Shells         closure-shell-core   closure-tui   closure-cli
                   closure-shell-gpui   closure-shell-egui
                   closure-shell-web    closure-shell-tauri
                   closure-shell-gtk    closure-shell-qt
                   closure-shell-slint  closure-wasm
                   closure-ffi
 ─────────────────────────────────────────────────────────────────────
 L4 Adapters (I8)  closure-llm   closure-mcp    closure-lsp
                   closure-acp   closure-a2a    closure-cron
                   closure-plugin-host   closure-sniffer
                   closure-record        closure-jsonrpc
 ─────────────────────────────────────────────────────────────────────
 L3 Evaluation     closure-eval  closure-crdt   closure-sync
 ─────────────────────────────────────────────────────────────────────
 L2 Kernel         closure-core  closure-store  closure-query
                   closure-undo
 ─────────────────────────────────────────────────────────────────────
 L1 Parsers        closure-org   closure-markdown   closure-tree-sitter
 ─────────────────────────────────────────────────────────────────────
 L0 Foundation     closure-config
```

Every crate in `crates/` appears above and nothing appears that is not
there, held by `closure-cli/tests/architecture_is_true.rs`. It used to
name `closure-spec`, `closure-util` and `closure-flutter`, none of
which exist, and omit `closure-shell-core`, which is the largest crate
in the workspace and the thing every shell is built from — wrong in
both directions at once, which is worse than no diagram, because this
is the page somebody reads first.

`closure-shell-core` is where the layer boundary actually is. Every
shell above it is a renderer of the `ViewTree` it derives; that is what
makes a shell replaceable and what I7 is about.

Arrows point up: each layer depends only on the layers below it.

- **I7 firewall**: L5 shells consume L2 only. They never reach into L1.
- **I8 firewall**: L4 adapters mutate L2 through the command registry
  only. No `&mut Document` leaks to an adapter.
- **Span firewall**: byte offsets exist inside L1 parsers (`pub(crate)`
  only). Nothing above L1 sees a span.

## What forces a v1.0 break

Only these three changes force breaking the kernel API:

1. Switching away from plain-text on-disk representation.
2. Removing `BlockId` as the unit of edit addressing.
3. Introducing a synchronous global mutable state path that commands
   bypass.

Every design decision in closure is tested against "does this rule out
any of the above?" If yes, the decision is wrong and must be revised
before merging.
