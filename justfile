# closure dev gates. Every recipe is bare cargo — always wrap the call in
# a dev shell:  `nix develop -c just <recipe>`. The native-webview GUIs
# (gui-tauri / gui-gtk / gui-qt + their run-*) need the heavier shell:
# `nix develop .#webview -c just <recipe>`. (egui/gpui use the default
# shell.) The nix flake check sandbox has no network, so these cargo-based
# gates live here (registry reachable) rather than in flake `checks`.

# Reclaim disk without throwing away the build.
#
# `target/` reached 75G on 2026-08-12 and the root filesystem hit 98%,
# at which point the session stopped: commands could not write their
# output, the linker had nowhere to put temporaries, and a gpui link
# failed with an error that looked real and was not.
#
# In order of what it costs to lose. `incremental` was 12G of the 75
# and buys only a slower next build, so it goes first and almost always
# suffices. The release profile is 3.6G and is rebuilt by one command.
# `cargo clean` is last because rebuilding gpui from scratch is twenty
# minutes, and it is what everybody reaches for first.
reclaim:
    @echo "before: $(df -h --output=avail / | tail -1) free"
    rm -rf target/debug/incremental target/release/incremental
    @echo "after:  $(df -h --output=avail / | tail -1) free"
    @echo "still tight? \`rm -rf target/release\` (3.6G, one rebuild), then \`cargo clean\` (all of it)."

# Every gate a change has to pass, in one command.
#
# `check` mirrors CI, and CI does not build gpui — so a change to the
# window can be green four times over and still ship a broken test. That
# happened on 2026-08-04: a keymap change made `C-Enter` mean something
# else, and the only test that noticed lives in `gpui-window`, which is
# not part of `check` and which I was running by hand and forgot.
#
# The two clippy passes are both needed and are not the same run: code
# behind `gpui` is not linted by the `gpui-test` build, and warnings have
# shipped through that gap before.
#
# The last line is treefmt, because a gate that differs from CI is a
# gate that lets CI fail for a reason nobody ran into locally.
gates:
    cargo test --workspace -j 4
    cargo clippy --workspace --all-targets -j 4
    cargo clippy -p closure-shell-gpui --features gpui-test --all-targets -j 4
    cargo clippy -p closure-shell-gpui --features gpui --all-targets -j 4
    cargo test -p closure-shell-gpui --features gpui-test -j 4
    cargo fmt --all -- --check
    # …and the formatter CI actually runs. `cargo fmt` sees Rust;
    # treefmt sees all 585 files, including the markdown in docs/. Two
    # lines of it went unnoticed through ~200 commits and then failed
    # `nix flake check` *before* the tests and clippy ran, so the whole
    # push had no signal at all for the sake of an emphasis marker.
    nix fmt -- --fail-on-change

# One-command gate: lint + tests (mirrors CI).
check:
    cargo clippy --workspace --tests -- -D warnings
    cargo nextest run --workspace

# Full-workspace line-coverage floor (ratchet toward 100%), including
# the CLI binary — `closure-cli/tests/cli.rs` spawns the real binary so
# main.rs is now genuinely covered (no exclusions). Fails under it.
# Floor ratcheted 82 → 84 (V10): the V10a render-snapshot harness + the
# new declarative surfaces made the render path hermetically reachable.
# D8 measurement after Depth IV: Lines 84.33% (Regions 80.46, Functions
# 81.74). The floor stays at 84 — already at ceiling-minus-margin. Depth
# IV added ~2k lines of NEW product code (markdown GFM blocks, the
# clipboard module, the OpenAI-wire provider, org table access, gpui
# arms) alongside their tests, so the hermetic ceiling moved only
# 84.0 → 84.33; there is no integer headroom left to claim. The residual
# ~16% is non-hermetic by design — the ratatui draw/run loop (closure-tui
# 63%), the curl/HTTP LLM paths, and the GUI window + web socket loops
# (recorded DROP under H2b); those need a live TTY/network/display and so
# cannot raise the *hermetic* gate. No coverage exclusions, no
# #[coverage(off)] (no gaming) — raising the integer would require exactly
# that, or testing the display/network loops that cannot run hermetically.
#
# 2026-08-11: 84 → 86. Lines 86.07% (Regions 83.37, Functions 83.48).
# The paragraph above was right about the ceiling *at the time* and
# wrong as a prediction: the kernel+gpui run added widget parameters,
# typed inputs, slots, cycle and depth errors, grouping, relations,
# rollups, repeaters, repeat-on-done, the conformance matrix, where-is
# and the pane fixes — every one of them driven by a test first, which
# is why ~2 points arrived rather than a fraction. The residual is
# still the same non-hermetic ~14%: the ratatui draw loop, the
# curl/HTTP paths, the GUI window and socket loops.
coverage:
    cargo llvm-cov --workspace --fail-under-lines 86

# Parser fuzz/replay + property gate (I1/I5/I6) on stable, for every
# first-class format (org + markdown).
fuzz:
    cargo test -p closure-org --test fuzz_replay --test properties
    cargo test -p closure-markdown --test properties

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

# The gpui *window* driven headlessly, over gpui's stub platform. Kept
# out of `check` because it pulls Zed's ~570-crate GPU stack, which the
# hermetic gate must not depend on (I10) — but this is the only thing
# that runs the render path, the key path and the input-method handler
# at all, so run it whenever the window changes.
gpui-window:
    cargo test -p closure-shell-gpui --features gpui-test -j 4

# Launch the gpui desktop shell against a vault (needs a display).
run-gpui vault:
    cargo run -p closure-cli --features gpui -- gpui {{vault}}

# Release build of the reference gpui shell. This is the one to use:
# the debug build runs gpui's GPU stack unoptimised and feels an order
# of magnitude slower than the shipped shell. `just run-gpui` is the
# debug launcher and is for debugging only.
#
# Heavy (~570 transitive crates). On a desktop with an aggressive
# systemd-oomd, prefer `just gpui-release-scoped`, which caps the build
# so the terminal survives it.
gpui-release:
    cargo build --release -p closure-cli --features gpui -j 4

# The same build inside a capped systemd scope: oomd then kills the
# scope rather than the terminal that started it. Run this one from
# OUTSIDE a dev shell — it enters one itself.
gpui-release-scoped:
    systemd-run --user --scope -p MemoryHigh=6G -p MemoryMax=8G -- \
      nix develop -c cargo build --release -p closure-cli --features gpui -j 4

# Build (if needed) and launch the RELEASE gpui shell against a vault.
#   nix develop -c just run-gpui-release ~/vault
run-gpui-release vault: gpui-release
    ./target/release/closure gpui {{vault}}

# Launch the already-built release shell without rebuilding — the fast
# path once `gpui-release` has run once. Still needs the dev shell: the
# binary links the GL/X11/xkbcommon libraries it provides.
#   nix develop -c just gpui ~/vault
gpui vault:
    ./target/release/closure gpui {{vault}}

# The same shell forced onto the software rasteriser (Mesa's lavapipe),
# which the dev shell ships as $CLOSURE_SOFTWARE_ICD. A machine with no
# usable GPU falls back to it on its own; this recipe is for the other
# case — a GPU that is there and wrong (a driver too old for gpui, a
# passthrough that isn't) — where the fallback never triggers. Slow,
# and it opens.
#   nix develop -c just gpui-software ~/vault
gpui-software vault:
    VK_DRIVER_FILES="$CLOSURE_SOFTWARE_ICD" VK_ICD_FILENAMES="$CLOSURE_SOFTWARE_ICD" \
      ./target/release/closure gpui {{vault}}

# Launch the release shell over a generated vault of the given size, to
# eyeball scroll/typing latency against something big. The vault is
# written under target/perf-vault (regenerated each run, deterministic).
gpui-bigvault files="200" heads="60": gpui-release
    rm -rf target/perf-vault
    cargo run --release -q -p closure-shell-core --example gen_vault -- \
      target/perf-vault {{files}} {{heads}}
    ./target/release/closure gpui target/perf-vault

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

# D7 real OS-clipboard adapter (opt-in; no extra crate — shells out to the
# platform tool at runtime). Default build keeps the in-memory clipboard;
# this gate builds + tests the SystemClipboard process round trip.
clipboard:
    cargo test -p closure-store --features clipboard --test clipboard --test clipboard_system

# D3 real network sync transport. The std-TCP loopback path is hermetic
# (127.0.0.1, no external network) so it also runs in the default suite;
# this recipe is the explicit network gate, and it additionally exercises
# the external IrohTransport (gracefully skipped when the `iroh` binary is
# absent). Two peers converge a divergent vault over a real socket with
# authenticated + Noise-encrypted frames, ids preserved (I2).
sync-net:
    cargo test -p closure-sync --test tcp --test encrypt --test p2p_i2 --test transport

# Full local CI: everything above.
ci: check fuzz wasm coverage

# Opt-in LIVE LLM gate (Q7-L2): real end-to-end ask against a local
# Ollama daemon; skips gracefully when absent. Never part of `check`.
llm-live:
    CLOSURE_LLM_LIVE=1 cargo test -p closure-llm --test live -- --nocapture

# Q8-P2 gate: network-registry fetch tests (loopback server + curl).
pkg-net:
    cargo test -p closure-plugin-host --test net_registry -- --nocapture

# Q12-B2: release-mode latency numbers over the generated big vault.
bench:
    cargo test --release -p closure-shell-core --test perf -- --nocapture
