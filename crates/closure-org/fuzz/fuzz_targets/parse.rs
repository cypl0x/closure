#![no_main]
// Fuzz target for closure-org parser (I1/I5/I6).
// Written FIRST per TDD for Quality "Fuzz targets run in CI (60s budget)".
// Run via: nix develop -c cargo fuzz run parse -- -max_total_time=60
// (from crate dir or adjusted for workspace).

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Never panic on arbitrary input (I5); parse returns Err or ok Document.
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = closure_org::parse(s);
    }
});