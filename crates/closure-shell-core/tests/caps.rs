//! V11: type-level shell capabilities. A shell can only invoke a
//! capability it statically declares — `capability_gate::<S, C>()`
//! compiles iff `S: Supports<C>`. The *negative* (an unsupported call
//! fails to compile) is proven by a `compile_fail` doctest on
//! `capability_gate`; here we assert the positive cases.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use closure_shell_core::caps::{Browse, Edit, ReadOnlyWebShell, Search, TuiShell, capability_gate};

#[test]
fn tui_supports_browse_edit_search() {
    // Each call only type-checks because TuiShell: Supports<that cap>.
    capability_gate::<TuiShell, Browse>();
    capability_gate::<TuiShell, Edit>();
    capability_gate::<TuiShell, Search>();
}

#[test]
fn readonly_web_supports_browse_and_search_but_not_edit() {
    capability_gate::<ReadOnlyWebShell, Browse>();
    capability_gate::<ReadOnlyWebShell, Search>();
    // `capability_gate::<ReadOnlyWebShell, Edit>()` would NOT compile —
    // see the compile_fail doctest on `capability_gate`.
}
