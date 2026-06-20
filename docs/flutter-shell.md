# Flutter shell (X1d) — external packaging project

The Flutter shell is **not** a workspace crate. The Flutter SDK (Dart
toolchain + `flutter` CLI) is not packaged hermetically in nixpkgs the
way the pure-Rust + pkg-config GUI stacks are, so vendoring it would
break I10 (hermetic, reproducible `nix flake check`). It therefore lives
as a separate project that *consumes* closure, in one of two ways:

## Option A — thin webview over `closure serve` (recommended, lowest cost)

`closure serve <vault>` exposes the vault over localhost (the same
routes the web shell and the Tauri/wry shell X1a use). A Flutter app
embeds a `WebView` (`webview_flutter`) pointed at that URL, or ships the
self-contained `closure export html` page as a bundled asset. No FFI,
no Rust↔Dart bindings — the kernel stays a black box behind HTTP, and
every mutation still flows through the command registry (I8).

```
closure serve ~/vault --addr 127.0.0.1:8787 &
# Flutter app: WebView(initialUrl: 'http://127.0.0.1:8787')
```

## Option B — `flutter_rust_bridge` FFI to `closure-shell-core`

For a native (non-webview) Flutter UI, generate Dart bindings over a
small C ABI exposed by a `cdylib` wrapper crate around
`closure-shell-core` (the shared, headless-tested state machine the
TUI/egui/gpui shells consume — I7). Dart calls `rows`, `select`,
`begin_capture`, etc.; the wrapper translates to `Shell`/`Vault`
commands (I8). This keeps the Rust side hermetic; only the Dart/Flutter
build is external.

## Why external is correct here

- I10: the default `nix develop` + `just check` stay free of the Dart
  toolchain; reproducibility is preserved.
- I7: the Flutter UI consumes a stable closure surface (HTTP routes or
  the `shell-core` ABI); it never reaches into the kernel.
- Reversible: if a hermetic Flutter/Dart nix story matures, this can
  become a feature-gated crate without changing the closure side.

The capability matrix (`closure shells`) keeps Flutter as a comparison
entry; this document is its "build" deliverable.
