# closure — architecture

One-page layer diagram + the three decisions that would force a v1.0
break.

## Layer diagram

```
 ┌─────────────────────────────────────────────────────────────────────┐
 │ L6 Input modes      closure-input   closure-whichkey                │
 ├─────────────────────────────────────────────────────────────────────┤
 │ L5 Shells           closure-tui   closure-cli   closure-shell-egui  │
 │                     closure-shell-web  -gpui  -tauri  -flutter  -qt │
 ├─────────────────────────────────────────────────────────────────────┤
 │ L4 Adapters (I8)    closure-llm  closure-mcp  closure-lsp  -acp     │
 │                     closure-cron  closure-plugin-host  -sniffer     │
 ├─────────────────────────────────────────────────────────────────────┤
 │ L3 Evaluation       closure-eval   closure-crdt   closure-sync      │
 ├─────────────────────────────────────────────────────────────────────┤
 │ L2 Kernel           closure-core   closure-store   closure-query    │
 │                     closure-undo                                    │
 ├─────────────────────────────────────────────────────────────────────┤
 │ L1 Parsers          closure-org   closure-markdown  -tree-sitter    │
 ├─────────────────────────────────────────────────────────────────────┤
 │ L0 Foundation       closure-spec   closure-config   closure-util    │
 └─────────────────────────────────────────────────────────────────────┘
```

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
