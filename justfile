# closure dev gates. Run inside the nix dev shell: `nix develop -c just <recipe>`.
# The nix flake check sandbox has no network, so these cargo-based gates
# live here (registry reachable) rather than in flake `checks`.

# One-command gate: lint + tests (mirrors CI).
check:
    cargo clippy --workspace --tests -- -D warnings
    cargo nextest run --workspace

# Full-workspace line-coverage floor (ratchet toward 100%), including
# the CLI binary — `closure-cli/tests/cli.rs` spawns the real binary so
# main.rs is now genuinely covered (no exclusions). Fails under it.
coverage:
    cargo llvm-cov --workspace --fail-under-lines 82

# Parser fuzz/replay + property gate (I1/I5/I6) on stable.
fuzz:
    cargo test -p closure-org --test fuzz_replay --test properties

# Wasm/WASI target check: org + core are pure and build for wasm32.
wasm:
    cargo check --target wasm32-wasip1 -p closure-org -p closure-core

# egui desktop shell build gate (opt-in; pulls eframe + system GL/X11/
# wayland/xkb libs from the flake). The window needs a display so it is
# NOT exercised here — this gate guarantees the feature still compiles.
# Launch it with:  just run-egui VAULT  (or the cargo run line below).
gui-egui:
    cargo build -p closure-cli --features egui

# Launch the egui desktop shell against a vault (needs a display).
run-egui vault:
    cargo run -p closure-cli --features egui -- egui {{vault}}

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

# Full local CI: everything above.
ci: check fuzz wasm coverage
