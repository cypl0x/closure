# closure dev gates. Run inside the nix dev shell: `nix develop -c just <recipe>`.
# The nix flake check sandbox has no network, so these cargo-based gates
# live here (registry reachable) rather than in flake `checks`.

# One-command gate: lint + tests (mirrors CI).
check:
    cargo clippy --workspace --tests -- -D warnings
    cargo nextest run --workspace

# Library line-coverage floor (ratchet toward 100%). Excludes the
# closure-cli binary glue (cmd_* wrappers are thin IO/print shims,
# exercised end-to-end, not unit-coverage targets — see ROADMAP
# Decisions). Fails under the threshold.
coverage:
    cargo llvm-cov --workspace --ignore-filename-regex 'closure-cli/src/main\.rs' --fail-under-lines 85

# Parser fuzz/replay + property gate (I1/I5/I6) on stable.
fuzz:
    cargo test -p closure-org --test fuzz_replay --test properties

# Wasm/WASI target check: org + core are pure and build for wasm32.
wasm:
    cargo check --target wasm32-wasip1 -p closure-org -p closure-core

# Full local CI: everything above.
ci: check fuzz wasm coverage
