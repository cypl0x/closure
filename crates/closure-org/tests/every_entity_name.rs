//! Every entity name this crate claims to know.
//!
//! `entity_char` is a seventy-way table and about a tenth of it had
//! ever been asked for. That is a bad shape to leave untested for a
//! reason peculiar to it: a wrong arm does not fail, it silently draws
//! a different character. `\deg` coming out as `∂` in somebody's notes
//! is not a crash, not a diagnostic, and not something they will
//! attribute to closure — they will assume they typed it wrong.
//!
//! The table is a subset on purpose. Its own doc says so: org has
//! hundreds, and "an unknown name is `None` rather than a guess — a
//! shell that cannot draw the glyph should leave the source alone and
//! say why, which is a better answer than a wrong character". So the
//! refusal is asserted as carefully as the answers.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use closure_org::entity_char;

/// Every name the table carries, with what it must produce.
const ENTITIES: &[(&str, &str)] = &[
    // Greek, lower case.
    ("alpha", "α"),
    ("beta", "β"),
    ("gamma", "γ"),
    ("delta", "δ"),
    ("epsilon", "ε"),
    ("zeta", "ζ"),
    ("eta", "η"),
    ("theta", "θ"),
    ("lambda", "λ"),
    ("mu", "μ"),
    ("pi", "π"),
    ("rho", "ρ"),
    ("sigma", "σ"),
    ("tau", "τ"),
    ("phi", "φ"),
    ("chi", "χ"),
    ("psi", "ψ"),
    ("omega", "ω"),
    // Greek, capitals — a separate arm each, and the pair that differs
    // only by case is exactly where a copy-paste error lands.
    ("Alpha", "Α"),
    ("Beta", "Β"),
    ("Gamma", "Γ"),
    ("Delta", "Δ"),
    ("Theta", "Θ"),
    ("Lambda", "Λ"),
    ("Pi", "Π"),
    ("Sigma", "Σ"),
    ("Phi", "Φ"),
    ("Psi", "Ψ"),
    ("Omega", "Ω"),
    // Punctuation and arrows.
    ("hellip", "…"),
    ("ldots", "…"),
    ("ndash", "–"),
    ("mdash", "—"),
    ("rightarrow", "→"),
    ("to", "→"),
    ("leftarrow", "←"),
    ("Rightarrow", "⇒"),
    ("Leftarrow", "⇐"),
    // Maths.
    ("times", "×"),
    ("div", "÷"),
    ("plusmn", "±"),
    ("leq", "≤"),
    ("geq", "≥"),
    ("neq", "≠"),
    ("approx", "≈"),
    ("infin", "∞"),
    ("infty", "∞"),
    ("sum", "∑"),
    ("prod", "∏"),
    ("radic", "√"),
    ("part", "∂"),
    ("nabla", "∇"),
    ("forall", "∀"),
    ("exist", "∃"),
    ("empty", "∅"),
    ("isin", "∈"),
    ("notin", "∉"),
    ("cap", "∩"),
    ("cup", "∪"),
    ("sub", "⊂"),
    ("sup", "⊃"),
    // Marks.
    ("deg", "°"),
    ("bull", "•"),
    ("copy", "©"),
    ("reg", "®"),
    ("trade", "™"),
    ("euro", "€"),
    ("pound", "£"),
    ("yen", "¥"),
    ("check", "✓"),
    ("star", "⋆"),
    ("dagger", "†"),
    ("sect", "§"),
    ("para", "¶"),
];

#[test]
fn every_name_produces_the_character_it_stands_for() {
    for (name, want) in ENTITIES {
        assert_eq!(entity_char(name), Some(*want), "\\{name} should be {want}");
    }
}

#[test]
fn the_case_of_a_greek_name_decides_the_case_of_the_letter() {
    // The pairs differing only by first letter are where a table like
    // this goes wrong, and `\Delta` arriving as `δ` is wrong in a way
    // that changes what a formula says.
    for (lower, upper) in [
        ("alpha", "Alpha"),
        ("beta", "Beta"),
        ("gamma", "Gamma"),
        ("delta", "Delta"),
        ("theta", "Theta"),
        ("lambda", "Lambda"),
        ("pi", "Pi"),
        ("sigma", "Sigma"),
        ("phi", "Phi"),
        ("psi", "Psi"),
        ("omega", "Omega"),
    ] {
        let l = entity_char(lower).expect(lower);
        let u = entity_char(upper).expect(upper);
        assert_ne!(l, u, "`{lower}` and `{upper}` produce the same glyph");
    }
}

#[test]
fn the_names_that_share_a_glyph_are_the_ones_that_should() {
    // Aliases org itself carries. Asserted so that dropping one is a
    // failing test rather than a name that silently stops working.
    for (a, b) in [
        ("hellip", "ldots"),
        ("rightarrow", "to"),
        ("infin", "infty"),
    ] {
        assert_eq!(
            entity_char(a),
            entity_char(b),
            "`{a}` and `{b}` are meant to be the same character"
        );
    }
}

#[test]
fn no_two_unrelated_names_produce_the_same_character() {
    // A duplicated glyph is how a mis-typed table arm hides: `\deg`
    // silently becoming `∂` still returns Some, and nothing downstream
    // can tell.
    let aliases: &[&[&str]] = &[
        &["hellip", "ldots"],
        &["rightarrow", "to"],
        &["infin", "infty"],
    ];
    let alias_of = |n: &str| aliases.iter().find(|g| g.contains(&n)).map(|g| g[0]);

    let mut seen: Vec<(&str, &str)> = Vec::new();
    for (name, glyph) in ENTITIES {
        if let Some((other, _)) = seen.iter().find(|(_, g)| g == glyph) {
            let same_group = alias_of(name).is_some() && alias_of(name) == alias_of(other);
            assert!(
                same_group,
                "`{name}` and `{other}` both produce {glyph}, and they are not aliases"
            );
        }
        seen.push((name, glyph));
    }
}

#[test]
fn a_name_the_table_does_not_carry_is_refused_rather_than_guessed() {
    // The documented refusal. Guessing would put a wrong character into
    // a note, which is worse than leaving `\aleph` on screen.
    for unknown in [
        "aleph",
        "beth",
        "notanentity",
        "",
        "ALPHA",
        "Alphaa",
        "alph",
        "rightarrows",
        "deg2",
        " alpha",
        "alpha ",
    ] {
        assert_eq!(
            entity_char(unknown),
            None,
            "`\\{unknown}` was given a character"
        );
    }
}

#[test]
fn a_capital_that_org_has_no_distinct_letter_for_is_not_invented() {
    // `\Epsilon` and `\Mu` look like Latin letters, so org does not
    // carry them and neither does this. Making one up would draw a
    // Latin `E` where the author wrote Greek.
    for absent in ["Epsilon", "Zeta", "Eta", "Mu", "Rho", "Tau", "Chi"] {
        assert_eq!(entity_char(absent), None, "`\\{absent}` was invented");
    }
}

#[test]
fn every_entity_is_a_single_visible_character() {
    // A table entry that was empty, or that carried a whole word, would
    // render as a gap or as prose in the middle of a formula.
    for (name, glyph) in ENTITIES {
        assert!(!glyph.is_empty(), "`{name}` maps to nothing");
        assert_eq!(
            glyph.chars().count(),
            1,
            "`{name}` maps to {glyph:?}, which is not one character"
        );
    }
}
