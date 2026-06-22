# closure dev gates. Every recipe is bare cargo — always wrap the call in
# a dev shell:  `nix develop -c just <recipe>`. The native-webview GUIs
# (gui-tauri / gui-gtk / gui-qt + their run-*) need the heavier shell:
# `nix develop .#webview -c just <recipe>`. (egui/gpui use the default
# shell.) The nix flake check sandbox has no network, so these cargo-based
# gates live here (registry reachable) rather than in flake `checks`.

# One-command gate: lint + tests (mirrors CI).
check:
    cargo clippy --workspace --tests -- -D warnings
    cargo nextest run --workspace

# Full-workspace line-coverage floor (ratchet toward 100%), including
# the CLI binary — `closure-cli/tests/cli.rs` spawns the real binary so
# main.rs is now genuinely covered (no exclusions). Fails under it.
# Floor ratcheted 82 → 84 (V10): the V10a render-snapshot harness + the
# new declarative surfaces made the render path hermetically reachable.
# The residual ~16% is non-hermetic by design — the ratatui draw/run
# loop, the curl/HTTP LLM paths, and the GUI window + web socket loops
# (recorded DROP under H2b); those need a live TTY/network/display and so
# cannot raise the *hermetic* gate. No coverage exclusions (no gaming).
coverage:
    cargo llvm-cov --workspace --fail-under-lines 84

# Parser fuzz/replay + property gate (I1/I5/I6) on stable.
fuzz:
    cargo test -p closure-org --test fuzz_replay --test properties

# Wasm/WASI target check: org + core are pure and build for wasm32.
wasm:
    cargo check --target wasm32-wasip1 -p closure-org -p closure-core

# X2a client-side kernel: build the wasm-bindgen surface for the browser
# target. Opt-in; the default `check` only compiles the pure core.
wasm-web:
    RUSTFLAGS='--cfg getrandom_backend="wasm_js"' cargo build -p closure-wasm --target wasm32-unknown-unknown --features wasm

# X2b full browser bundle: build the wasm, generate the wasm-bindgen
# `--target web` glue (CLI version pinned to the crate), and assemble a
# single self-contained client-side editor into target/wasm-web/editor.html.
wasm-web-bundle: wasm-web
    nix shell nixpkgs#wasm-bindgen-cli -c wasm-bindgen --target web --no-typescript --out-dir target/wasm-web target/wasm32-unknown-unknown/debug/closure_wasm.wasm
    cargo run -q -p closure-wasm --example build_editor -- target/wasm-web/closure_wasm.js target/wasm-web/closure_wasm_bg.wasm > target/wasm-web/editor.html
    @echo "wrote target/wasm-web/editor.html ($(wc -c < target/wasm-web/editor.html) bytes)"

# egui desktop shell build gate (opt-in; pulls eframe + system GL/X11/
# wayland/xkb libs from the flake). The window needs a display so it is
# NOT exercised here — this gate guarantees the feature still compiles.
# Launch it with:  just run-egui VAULT  (or the cargo run line below).
gui-egui:
    cargo build -p closure-cli --features egui

# Launch the egui desktop shell against a vault (needs a display).
run-egui vault:
    cargo run -p closure-cli --features egui -- egui {{vault}}

# gpui desktop shell build gate (opt-in; pulls Zed's gpui + the same
# GL/X11/wayland/xkb libs as egui, from the default devshell). The window
# needs a display so it is NOT exercised here — this gate guarantees the
# feature still compiles. Launch it with:  just run-gpui VAULT.
gui-gpui:
    cargo build -p closure-cli --features gpui

# Launch the gpui desktop shell against a vault (needs a display).
run-gpui vault:
    cargo run -p closure-cli --features gpui -- gpui {{vault}}

# Embedded wasm plugin runtime build + test gate (opt-in; pulls
# wasmtime + cranelift). Hermetic (WAT fixtures run in-process); kept
# out of the default `check` so that build stays light. Registry
# reachable here (justfile), unlike the network-less flake sandbox.
plugin-wasm:
    cargo build -p closure-plugin-host --features wasmtime
    cargo test -p closure-plugin-host --features wasmtime

# C1c wasm sandbox exec tier (opt-in; default build stays hermetic).
eval-wasm:
    cargo test -p closure-eval --features wasmtime --test wasm
    cargo test -p closure-store --features wasmtime --test babel

# V6 real tree-sitter highlighting (opt-in; pulls a C grammar). Default
# `check` keeps the dep-free KeywordHighlighter; this gate builds + tests
# the real grammar path.
tree-sitter:
    cargo test -p closure-tree-sitter --features tree-sitter
    cargo test -p closure-tui --features tree-sitter --test ts_highlight

# X3 live packet sniffer (opt-in; pulls pnet). Default stays dep-light;
# live capture needs CAP_NET_RAW at runtime (`closure sniff --live eth0`).
sniff-pcap:
    cargo build -p closure-cli --features pcap
    cargo test -p closure-sniffer --features pcap

# X1a native webview shell (opt-in; pulls wry + webkitgtk). Run under the
# webview devshell:  nix develop .#webview -c just gui-tauri
gui-tauri:
    cargo build -p closure-shell-tauri --features tauri

# Launch the native webview shell against a vault (needs a display).
#   nix develop .#webview -c just run-tauri VAULT
run-tauri vault:
    cargo run -p closure-shell-tauri --features tauri -- {{vault}}

# X1b native GTK4 shell (opt-in; pulls gtk4-rs + GTK4). Run under the
# webview devshell:  nix develop .#webview -c just gui-gtk
gui-gtk:
    cargo build -p closure-shell-gtk --features gtk

# Launch the GTK4 shell against a vault (needs a display).
#   nix develop .#webview -c just run-gtk VAULT
run-gtk vault:
    cargo run -p closure-shell-gtk --features gtk -- {{vault}}

# X1c native Qt6/QML shell (opt-in; needs Qt6 / qmake6). Run under the
# webview devshell:  nix develop .#webview -c just gui-qt
gui-qt:
    cargo build -p closure-shell-qt --features qt

# Launch the Qt6 shell against a vault (needs a display).
#   nix develop .#webview -c just run-qt VAULT
run-qt vault:
    cargo run -p closure-shell-qt --features qt -- {{vault}}

# Full local CI: everything above.
ci: check fuzz wasm coverage
