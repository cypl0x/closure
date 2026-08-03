// Shared by `build.rs` (via `include!`) and by the tests, so the rule
// has one owner. `build.rs` cannot depend on its own crate, and a
// second copy of this parser is exactly how the two would drift.

/// The paths a build script should re-run on, given `.git`'s location
/// and the contents of its `HEAD`.
///
/// `HEAD` alone is not enough and that was the defect: on a branch it
/// holds `ref: refs/heads/main` and does not change when a commit
/// lands or a `git pull` fast-forwards — only the *ref file* does. So
/// a build captured the hash once and reported it forever, which made
/// `build-info` confidently name a commit that was not what was
/// running. A version string that lies is worse than no version
/// string.
///
/// `packed-refs` is watched too: `git gc` moves refs into it and
/// deletes the loose file, after which the loose path never changes
/// again.
#[must_use]
pub fn watch_paths(git_dir: &str, head_contents: &str) -> Vec<String> {
    let mut out = vec![format!("{git_dir}/HEAD")];
    if let Some(reference) = head_contents.trim().strip_prefix("ref:") {
        out.push(format!("{git_dir}/{}", reference.trim()));
        out.push(format!("{git_dir}/packed-refs"));
    }
    out
}
