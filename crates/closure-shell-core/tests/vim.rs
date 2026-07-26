//! The vim grammar of the body editor: `[count]operator[count]motion`,
//! text objects, and the Normal/Visual vocabulary muscle memory
//! expects (`diw`, `caw`, `dt,`, `ge`, `A`, `J`, …).
//!
//! These drive [`BodyEditor`] directly — the shells (gpui, TUI) both
//! feed it the same stroke names, so one grammar covers both (I4).

use closure_shell_core::{BodyEditor, EditorMode, editor_hint};

/// A Normal-mode editor over `text` with the cursor at byte 0.
fn ed(text: &str) -> BodyEditor {
    let mut e = BodyEditor::new();
    e.load(text.to_owned());
    e.to_normal();
    e.set_cursor_byte(0);
    e
}

/// Feed one stroke per char (the single-char vim vocabulary).
fn feed(e: &mut BodyEditor, keys: &str) {
    for c in keys.chars() {
        e.modal_key(&c.to_string());
    }
}

/// Cursor as a byte offset — what the range assertions address.
const fn at(e: &BodyEditor) -> usize {
    e.cursor_byte()
}

// === Text objects. ===

#[test]
fn diw_deletes_the_inner_word() {
    let mut e = ed("hello world");
    feed(&mut e, "diw");
    assert_eq!(e.text(), " world");
    assert_eq!(at(&e), 0);
}

#[test]
fn diw_from_inside_the_word_still_takes_the_whole_word() {
    let mut e = ed("hello world");
    e.set_cursor_byte(3);
    feed(&mut e, "diw");
    assert_eq!(e.text(), " world");
}

#[test]
fn diw_on_the_second_word_leaves_the_first() {
    let mut e = ed("hello world");
    e.set_cursor_byte(8);
    feed(&mut e, "diw");
    assert_eq!(e.text(), "hello ");
}

#[test]
fn daw_takes_the_word_and_its_trailing_space() {
    let mut e = ed("hello world");
    feed(&mut e, "daw");
    assert_eq!(e.text(), "world");
}

#[test]
fn daw_on_the_last_word_takes_the_leading_space() {
    let mut e = ed("hello world");
    e.set_cursor_byte(6);
    feed(&mut e, "daw");
    assert_eq!(e.text(), "hello");
}

#[test]
fn ciw_changes_the_word_and_enters_insert() {
    let mut e = ed("hello world");
    feed(&mut e, "ciw");
    assert_eq!(e.text(), " world");
    assert_eq!(e.mode(), EditorMode::Insert);
    e.insert_str("bye");
    assert_eq!(e.text(), "bye world");
}

#[test]
fn yiw_yanks_the_word_without_deleting() {
    let mut e = ed("hello world");
    feed(&mut e, "yiw");
    assert_eq!(e.text(), "hello world");
    feed(&mut e, "$p");
    assert_eq!(e.text(), "hello worldhello");
}

#[test]
fn iw_on_whitespace_takes_the_whitespace_run() {
    let mut e = ed("a   b");
    e.set_cursor_byte(2);
    feed(&mut e, "diw");
    assert_eq!(e.text(), "ab");
}

#[test]
fn inner_word_stops_at_punctuation() {
    let mut e = ed("foo.bar");
    feed(&mut e, "diw");
    assert_eq!(e.text(), ".bar");
}

#[test]
fn inner_big_word_swallows_punctuation() {
    let mut e = ed("foo.bar baz");
    feed(&mut e, "diW");
    assert_eq!(e.text(), " baz");
}

#[test]
fn di_quote_deletes_inside_the_quotes() {
    let mut e = ed("say \"hi there\" ok");
    e.set_cursor_byte(7);
    feed(&mut e, "di\"");
    assert_eq!(e.text(), "say \"\" ok");
}

#[test]
fn da_quote_deletes_the_quotes_too() {
    let mut e = ed("say \"hi\" ok");
    e.set_cursor_byte(6);
    feed(&mut e, "da\"");
    assert_eq!(e.text(), "say  ok");
}

#[test]
fn di_paren_deletes_inside_the_parens() {
    let mut e = ed("f(a, b) end");
    e.set_cursor_byte(3);
    feed(&mut e, "di(");
    assert_eq!(e.text(), "f() end");
}

#[test]
fn da_paren_deletes_the_parens_too() {
    let mut e = ed("f(a, b) end");
    e.set_cursor_byte(3);
    feed(&mut e, "da(");
    assert_eq!(e.text(), "f end");
}

#[test]
fn paren_object_works_from_the_bracket_itself() {
    let mut e = ed("f(a) end");
    e.set_cursor_byte(1);
    feed(&mut e, "di(");
    assert_eq!(e.text(), "f() end");
}

#[test]
fn b_and_capital_b_alias_the_bracket_objects() {
    let mut e = ed("f(a) [x] {y}");
    e.set_cursor_byte(2);
    feed(&mut e, "dib");
    assert_eq!(e.text(), "f() [x] {y}");
    e.set_cursor_byte(10);
    feed(&mut e, "diB");
    assert_eq!(e.text(), "f() [x] {}");
}

#[test]
fn bracket_objects_nest_to_the_innermost_pair() {
    let mut e = ed("a(b(c)d)e");
    e.set_cursor_byte(4);
    feed(&mut e, "di(");
    assert_eq!(e.text(), "a(b()d)e");
}

#[test]
fn dip_deletes_the_paragraph_linewise() {
    let mut e = ed("a\nb\n\nc");
    feed(&mut e, "dip");
    assert_eq!(e.text(), "\nc");
}

#[test]
fn dap_takes_the_trailing_blank_lines() {
    let mut e = ed("a\nb\n\nc");
    feed(&mut e, "dap");
    assert_eq!(e.text(), "c");
}

// === Operator + motion. ===

#[test]
fn dw_deletes_to_the_next_word_start() {
    let mut e = ed("one two three");
    feed(&mut e, "dw");
    assert_eq!(e.text(), "two three");
}

#[test]
fn dw_stops_at_the_end_of_the_line() {
    let mut e = ed("one\ntwo");
    feed(&mut e, "dw");
    assert_eq!(e.text(), "\ntwo", "dw never joins lines");
}

#[test]
fn d_dollar_deletes_to_the_line_end_inclusive() {
    let mut e = ed("hello world");
    e.set_cursor_byte(5);
    feed(&mut e, "d$");
    assert_eq!(e.text(), "hello");
}

#[test]
fn d_zero_deletes_back_to_the_line_start() {
    let mut e = ed("hello world");
    e.set_cursor_byte(6);
    feed(&mut e, "d0");
    assert_eq!(e.text(), "world");
}

#[test]
fn d_caret_deletes_back_to_the_first_non_blank() {
    let mut e = ed("  hello world");
    e.set_cursor_byte(8);
    feed(&mut e, "d^");
    assert_eq!(e.text(), "  world");
}

#[test]
fn de_deletes_through_the_word_end() {
    let mut e = ed("one two");
    feed(&mut e, "de");
    assert_eq!(e.text(), " two");
}

#[test]
fn db_deletes_back_a_word() {
    let mut e = ed("one two");
    e.set_cursor_byte(4);
    feed(&mut e, "db");
    assert_eq!(e.text(), "two");
}

#[test]
fn cw_changes_only_the_word_not_the_space() {
    let mut e = ed("one two");
    feed(&mut e, "cw");
    assert_eq!(e.text(), " two");
    assert_eq!(e.mode(), EditorMode::Insert);
}

#[test]
fn counts_multiply_on_both_sides_of_the_operator() {
    let mut a = ed("a b c d");
    feed(&mut a, "d2w");
    assert_eq!(a.text(), "c d");

    let mut b = ed("a b c d");
    feed(&mut b, "2dw");
    assert_eq!(b.text(), "c d");

    let mut c = ed("a b c d e");
    feed(&mut c, "2d2w");
    assert_eq!(c.text(), "e", "2 × 2 words");
}

#[test]
fn dj_and_dk_are_linewise() {
    let mut e = ed("one\ntwo\nthree");
    feed(&mut e, "dj");
    assert_eq!(e.text(), "three");
}

#[test]
fn dgg_and_dg_delete_linewise_to_the_buffer_ends() {
    let mut e = ed("a\nb\nc");
    e.modal_key("j");
    feed(&mut e, "dG");
    assert_eq!(e.text(), "a");

    let mut e2 = ed("a\nb\nc");
    e2.modal_key("j");
    feed(&mut e2, "dgg");
    assert_eq!(e2.text(), "c");
}

#[test]
fn yank_motion_then_paste_round_trips() {
    let mut e = ed("one two");
    feed(&mut e, "yw");
    feed(&mut e, "$p");
    assert_eq!(e.text(), "one twoone ");
}

#[test]
fn an_unknown_motion_cancels_the_operator() {
    let mut e = ed("abc");
    feed(&mut e, "dz");
    assert_eq!(e.text(), "abc", "dz is not a command");
    feed(&mut e, "x");
    assert_eq!(e.text(), "bc", "and the editor is back to Normal");
}

#[test]
fn escape_cancels_a_pending_operator() {
    let mut e = ed("abc");
    e.modal_key("d");
    assert_eq!(e.pending_stroke(), Some('d'));
    e.modal_key("escape");
    assert_eq!(e.pending_stroke(), None);
    feed(&mut e, "x");
    assert_eq!(e.text(), "bc");
}

// === Find-char motions. ===

#[test]
fn dt_deletes_up_to_the_char() {
    let mut e = ed("foo bar");
    feed(&mut e, "dtb");
    assert_eq!(e.text(), "bar");
}

#[test]
fn df_deletes_through_the_char() {
    let mut e = ed("foo bar");
    feed(&mut e, "dfb");
    assert_eq!(e.text(), "ar");
}

#[test]
fn f_moves_and_semicolon_repeats() {
    let mut e = ed("a.b.c");
    feed(&mut e, "f.");
    assert_eq!(at(&e), 1);
    feed(&mut e, ";");
    assert_eq!(at(&e), 3);
    feed(&mut e, ",");
    assert_eq!(at(&e), 1, "comma reverses the last find");
}

#[test]
fn capital_f_searches_backwards() {
    let mut e = ed("a.b.c");
    e.set_cursor_byte(4);
    feed(&mut e, "F.");
    assert_eq!(at(&e), 3);
    feed(&mut e, "T.");
    assert_eq!(at(&e), 2, "T stops after the match");
}

#[test]
fn find_does_not_cross_the_line() {
    let mut e = ed("abc\nxbz");
    feed(&mut e, "fz");
    assert_eq!(at(&e), 0, "no z on this line, cursor stays");
}

// === Word motions with vim's word classes. ===

#[test]
fn w_stops_at_punctuation_runs() {
    let mut e = ed("foo.bar baz");
    feed(&mut e, "w");
    assert_eq!(at(&e), 3, "the dot is its own word");
    feed(&mut e, "w");
    assert_eq!(at(&e), 4);
    feed(&mut e, "w");
    assert_eq!(at(&e), 8);
}

#[test]
fn capital_w_skips_to_the_next_blank_delimited_word() {
    let mut e = ed("foo.bar baz");
    feed(&mut e, "W");
    assert_eq!(at(&e), 8);
}

#[test]
fn e_lands_on_the_word_end() {
    let mut e = ed("one two");
    feed(&mut e, "e");
    assert_eq!(at(&e), 2);
    feed(&mut e, "e");
    assert_eq!(at(&e), 6);
}

#[test]
fn capital_e_lands_on_the_big_word_end() {
    let mut e = ed("foo.bar baz");
    feed(&mut e, "E");
    assert_eq!(at(&e), 6);
}

#[test]
fn capital_b_walks_back_over_big_words() {
    let mut e = ed("foo.bar baz");
    e.set_cursor_byte(8);
    feed(&mut e, "B");
    assert_eq!(at(&e), 0);
}

// === Line and buffer motions. ===

#[test]
fn gg_and_capital_g_jump_to_the_buffer_ends() {
    let mut e = ed("a\nb\nc");
    feed(&mut e, "G");
    assert_eq!(e.cursor_line_col().0, 2);
    feed(&mut e, "gg");
    assert_eq!(e.cursor_line_col().0, 0);
}

#[test]
fn a_count_before_capital_g_picks_the_line() {
    let mut e = ed("a\nb\nc\nd");
    feed(&mut e, "3G");
    assert_eq!(e.cursor_line_col().0, 2, "1-based line 3");
}

#[test]
fn caret_moves_to_the_first_non_blank() {
    let mut e = ed("   indented");
    feed(&mut e, "$^");
    assert_eq!(at(&e), 3);
}

#[test]
fn braces_move_by_paragraph() {
    let mut e = ed("a\nb\n\nc\nd");
    feed(&mut e, "}");
    assert_eq!(e.cursor_line_col().0, 2, "the blank line");
    feed(&mut e, "}");
    assert_eq!(e.cursor_line_col().0, 4, "buffer end");
    feed(&mut e, "{");
    assert_eq!(e.cursor_line_col().0, 2);
}

// === The rest of the Normal vocabulary. ===

#[test]
fn capital_a_appends_at_the_line_end() {
    let mut e = ed("ab");
    feed(&mut e, "A");
    assert_eq!(e.mode(), EditorMode::Insert);
    e.insert_str("!");
    assert_eq!(e.text(), "ab!");
}

#[test]
fn capital_i_inserts_at_the_first_non_blank() {
    let mut e = ed("  ab");
    e.set_cursor_byte(3);
    feed(&mut e, "I");
    e.insert_str("!");
    assert_eq!(e.text(), "  !ab");
}

#[test]
fn capital_o_opens_a_line_above() {
    let mut e = ed("b");
    feed(&mut e, "O");
    assert_eq!(e.mode(), EditorMode::Insert);
    e.insert_str("a");
    assert_eq!(e.text(), "a\nb");
}

#[test]
fn capital_d_and_capital_c_reach_the_line_end() {
    let mut d = ed("hello world");
    d.set_cursor_byte(5);
    feed(&mut d, "D");
    assert_eq!(d.text(), "hello");

    let mut c = ed("hello world");
    c.set_cursor_byte(5);
    feed(&mut c, "C");
    assert_eq!(c.text(), "hello");
    assert_eq!(c.mode(), EditorMode::Insert);
}

#[test]
fn s_substitutes_the_char_and_capital_s_the_line() {
    let mut s = ed("abc");
    feed(&mut s, "s");
    assert_eq!(s.text(), "bc");
    assert_eq!(s.mode(), EditorMode::Insert);

    let mut big = ed("one\ntwo");
    feed(&mut big, "S");
    assert_eq!(big.text(), "\ntwo", "the line is emptied, not removed");
    assert_eq!(big.mode(), EditorMode::Insert);
}

#[test]
fn capital_x_deletes_the_char_before_the_cursor() {
    let mut e = ed("abc");
    e.set_cursor_byte(2);
    feed(&mut e, "X");
    assert_eq!(e.text(), "ac");
    assert_eq!(at(&e), 1);
}

#[test]
fn r_replaces_the_char_under_the_cursor() {
    let mut e = ed("abc");
    feed(&mut e, "rz");
    assert_eq!(e.text(), "zbc");
    assert_eq!(e.mode(), EditorMode::Normal, "r does not enter insert");
}

#[test]
fn tilde_toggles_case_and_advances() {
    let mut e = ed("abc");
    feed(&mut e, "~");
    assert_eq!(e.text(), "Abc");
    assert_eq!(at(&e), 1);
    feed(&mut e, "~");
    assert_eq!(e.text(), "ABc");
}

#[test]
fn capital_j_joins_the_next_line_with_a_space() {
    let mut e = ed("one\ntwo");
    feed(&mut e, "J");
    assert_eq!(e.text(), "one two");
}

#[test]
fn capital_j_does_not_double_an_existing_space() {
    let mut e = ed("one\n   two");
    feed(&mut e, "J");
    assert_eq!(e.text(), "one two");
}

#[test]
fn capital_p_pastes_before() {
    let mut e = ed("one\ntwo");
    feed(&mut e, "yy");
    feed(&mut e, "j");
    feed(&mut e, "P");
    assert_eq!(e.text(), "one\none\ntwo");
}

#[test]
fn capital_p_pastes_charwise_before_the_cursor() {
    let mut e = ed("ab");
    feed(&mut e, "ylP");
    assert_eq!(e.text(), "aab");
}

#[test]
fn indent_operators_shift_the_line() {
    let mut e = ed("ab");
    feed(&mut e, ">>");
    assert_eq!(e.text(), "  ab");
    feed(&mut e, "<<");
    assert_eq!(e.text(), "ab");
}

#[test]
fn indent_takes_a_motion_and_a_count() {
    let mut e = ed("a\nb\nc");
    feed(&mut e, "2>>");
    assert_eq!(e.text(), "  a\n  b\nc");
}

// === Visual mode. ===

#[test]
fn visual_iw_selects_the_word() {
    let mut e = ed("hello world");
    feed(&mut e, "viw");
    assert_eq!(e.visual_selection(), Some((0, 5)));
    feed(&mut e, "d");
    assert_eq!(e.text(), " world");
}

#[test]
fn visual_ip_selects_the_paragraph() {
    let mut e = ed("a\nb\n\nc");
    feed(&mut e, "vip");
    feed(&mut e, "d");
    assert_eq!(e.text(), "\nc");
}

#[test]
fn visual_c_changes_the_selection() {
    let mut e = ed("hello world");
    feed(&mut e, "viwc");
    assert_eq!(e.text(), " world");
    assert_eq!(e.mode(), EditorMode::Insert);
}

#[test]
fn visual_o_swaps_the_ends() {
    let mut e = ed("abcdef");
    e.set_cursor_byte(2);
    feed(&mut e, "vll");
    assert_eq!(at(&e), 4);
    feed(&mut e, "o");
    assert_eq!(at(&e), 2, "cursor jumps to the anchor");
    feed(&mut e, "h");
    feed(&mut e, "d");
    assert_eq!(e.text(), "af", "the selection grew leftwards");
}

#[test]
fn visual_p_replaces_the_selection_with_the_register() {
    let mut e = ed("one two");
    feed(&mut e, "yiw");
    e.set_cursor_byte(4);
    feed(&mut e, "viwp");
    assert_eq!(e.text(), "one one");
}

#[test]
fn visual_switches_between_charwise_and_linewise() {
    let mut e = ed("one\ntwo");
    feed(&mut e, "vV");
    assert_eq!(e.mode(), EditorMode::VisualLine);
    feed(&mut e, "v");
    assert_eq!(e.mode(), EditorMode::Visual);
}

#[test]
fn visual_capital_d_and_capital_c_are_linewise() {
    let mut e = ed("one\ntwo\nthree");
    feed(&mut e, "vjD");
    assert_eq!(e.text(), "three");
}

// === Regressions: the pre-existing vocabulary keeps working. ===

#[test]
fn dd_and_yy_still_work() {
    let mut e = ed("one\ntwo\nthree");
    feed(&mut e, "jdd");
    assert_eq!(e.text(), "one\nthree");
    feed(&mut e, "yyp");
    assert_eq!(e.text(), "one\nthree\nthree");
}

#[test]
fn cc_clears_the_line_and_enters_insert() {
    let mut e = ed("one\ntwo");
    feed(&mut e, "cc");
    assert_eq!(e.text(), "\ntwo");
    assert_eq!(e.mode(), EditorMode::Insert);
    assert_eq!(at(&e), 0);
}

#[test]
fn the_chord_in_progress_reads_back_for_the_status_line() {
    let mut e = ed("abc");
    assert_eq!(e.pending_chord(), "");
    e.modal_key("2");
    assert_eq!(e.pending_chord(), "2");
    e.modal_key("d");
    assert_eq!(e.pending_chord(), "2d", "the count survives the operator");
    e.modal_key("3");
    assert_eq!(e.pending_chord(), "2d3");
    e.modal_key("i");
    assert_eq!(e.pending_chord(), "2d3i");
    e.modal_key("escape");
    assert_eq!(e.pending_chord(), "");
}

#[test]
fn a_find_and_a_replace_also_show_as_pending() {
    let mut e = ed("abc");
    e.modal_key("d");
    e.modal_key("t");
    assert_eq!(e.pending_chord(), "dt");
    e.modal_key("c");
    assert_eq!(e.pending_chord(), "");

    let mut r = ed("abc");
    r.modal_key("r");
    assert_eq!(r.pending_chord(), "r");
}

#[test]
fn the_hint_line_advertises_what_the_editor_actually_does() {
    // Both shells paint this, so a chord named here works in both.
    let normal = editor_hint(EditorMode::Normal);
    for chord in ["diw", "caw", "dt"] {
        assert!(
            normal.contains(chord),
            "NORMAL hint {normal:?} never mentions {chord}"
        );
    }
    assert!(editor_hint(EditorMode::Insert).contains("Esc"));
    assert!(editor_hint(EditorMode::Visual).contains("iw"));
    assert_eq!(
        editor_hint(EditorMode::Visual),
        editor_hint(EditorMode::VisualLine),
        "both visual modes take the same operators"
    );
}

#[test]
fn u_undoes_a_text_object_delete_as_one_edit() {
    let mut e = ed("hello world");
    feed(&mut e, "diw");
    assert_eq!(e.text(), " world");
    feed(&mut e, "u");
    assert_eq!(e.text(), "hello world");
}
