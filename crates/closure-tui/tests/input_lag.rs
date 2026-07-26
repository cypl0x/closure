//! Input-lag budget: keystroke handling on a large vault stays well
//! under an interactive threshold. `App::handle_stroke` is pure
//! in-memory, so cost must not scale with a re-walk of the vault.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::Instant;

use closure_tui::{App, HeadlineRecord};

fn big_app(n: usize) -> App {
    let paths: Vec<PathBuf> = (0..n).map(|i| PathBuf::from(format!("f{i}.org"))).collect();
    let mut app = App::new(paths);
    let headlines: Vec<HeadlineRecord> = (0..n)
        .map(|i| HeadlineRecord {
            path: PathBuf::from(format!("f{}.org", i % 100)),
            id: format!("id-{i}"),
            title: format!("Headline number {i} about things"),
            body: format!("body line for {i}\nsecond line {i}\n"),
            ..HeadlineRecord::default()
        })
        .collect();
    app.set_headlines(headlines);
    app.set_sources(
        (0..n)
            .map(|i| {
                (
                    PathBuf::from(format!("f{i}.org")),
                    format!("* H{i}\nbody {i}\n"),
                )
            })
            .collect(),
    );
    app
}

#[test]
fn navigation_keystroke_is_fast_on_a_large_vault() {
    let mut app = big_app(1000);
    let start = Instant::now();
    for _ in 0..1000 {
        app.handle_stroke("j");
    }
    let per = start.elapsed() / 1000;
    assert!(
        per.as_millis() < 16,
        "navigation must beat a 16ms frame budget, was {per:?}"
    );
}

#[test]
fn headline_search_keystroke_stays_interactive() {
    let mut app = big_app(2000);
    app.handle_stroke("s");
    let start = Instant::now();
    for c in "headline".chars() {
        app.handle_stroke(&c.to_string());
    }
    let per = start.elapsed() / 8;
    assert!(
        per.as_millis() < 50,
        "fuzzy search keystroke over 2k headlines was {per:?}"
    );
}
