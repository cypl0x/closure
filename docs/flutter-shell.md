# Flutter shell (X1d) — external packaging project

The Flutter shell is **not** a workspace crate. The Flutter SDK (Dart
toolchain + `flutter` CLI) is not packaged hermetically in nixpkgs the
way the pure-Rust + pkg-config GUI stacks are, so vendoring it would
break I10 (hermetic, reproducible `nix flake check`). It therefore lives
as a separate project that _consumes_ closure, in one of two ways:

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

## Mobile (V12b)

closure has two mobile-capable surfaces, both reusing the existing kernel
without a new in-tree build:

1. **Responsive web (in-tree, hermetic).** `closure serve` and the
   single-file `closure export html` now emit a `width=device-width`
   viewport meta and a `@media (max-width: 40em)` layout, so the web shell
   is usable on a phone browser today — no app-store build, no native
   toolchain. This is the default mobile story and is covered by the web
   shell tests.

2. **Native mobile app (external, like X1d).** A Flutter app (iOS/Android)
   consumes the same surface — either a `WebView` over `closure serve` or
   `flutter_rust_bridge` over the `shell-core` ABI rendering the
   `ViewTree` (`Node`) natively. The Dart/Flutter SDK + the Xcode/Android
   NDK toolchains are **not hermetically nix-packaged**, so a native
   mobile build cannot live in the workspace under I10 — it is an external
   packaging project, exactly as the desktop Flutter shell (X1d). The
   `ViewTree` + `closure serve` are the stable contract it builds on; the
   Rust side stays hermetic and unchanged.

The responsive web path means "usable on a phone" needs no external build
at all; the native app is the optional, external polish tier.
