//! Which optional features this binary was built with.
//!
//! "Emacs like system-configuration-options? … the
//! `system-configuration-features` variable lists features enabled at
//! compile time … Should we implement this? Or does it compromise
//! reproducability?"
//!
//! It does not, by exactly the argument that admits the commit hash:
//! the feature set is a property of *what* was built, not of when, so
//! two builds from one source with one feature set stay one binary.
//!
//! It lives here rather than in `closure-core` because cargo features
//! are per crate. `closure-core` has none of its own, so asking it
//! what the *binary* was built with can only ever answer "nothing" —
//! the question belongs to the crate that owns the feature flags, and
//! that is this one.

fn main() {
    let mut features: Vec<String> = std::env::vars()
        .filter_map(|(key, _)| {
            key.strip_prefix("CARGO_FEATURE_")
                .map(|f| f.to_lowercase().replace('_', "-"))
        })
        .collect();
    features.sort();
    println!("cargo::rustc-env=CLOSURE_FEATURES={}", features.join(","));
}
