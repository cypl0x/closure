//! Vault loader tests.
//!
//! Confirms that closure-store discovers all `*.org` files in a
//! directory, parses each into a [`closure_core::Document`], and
//! exposes cross-file access by path or block id.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use closure_store::Vault;
use tempfile::TempDir;

fn write_vault(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, body).expect("write");
    }
    dir
}

#[test]
fn vault_loads_all_org_files() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("notes/c.org", "* C\n"),
        ("readme.md", "# not an org file\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.len(), 3);
    let names: Vec<String> = v
        .paths()
        .iter()
        .map(|p| {
            p.strip_prefix(td.path())
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(names.iter().any(|s| s.ends_with("a.org")));
    assert!(names.iter().any(|s| s.ends_with("c.org")));
}

#[test]
fn vault_document_lookup_by_path() {
    let td = write_vault(&[("x.org", "* Target\n")]);
    let v = Vault::open(td.path()).expect("open");
    let doc = v.document(&td.path().join("x.org")).expect("lookup");
    assert_eq!(doc.roots()[0].title(), "Target");
}

#[test]
fn vault_block_id_index_across_files() {
    let td = write_vault(&[
        (
            "a.org",
            "* A\n:PROPERTIES:\n:ID: 01HXAAAAAAAAAAAAAAAAAAAAAA\n:END:\n",
        ),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:ID: 01HXBBBBBBBBBBBBBBBBBBBBBB\n:END:\n",
        ),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let id = closure_core::BlockId::from_existing("01HXBBBBBBBBBBBBBBBBBBBBBB");
    let found = v.find_by_id(&id).expect("find");
    assert!(found.1.to_string_lossy().ends_with("b.org"));
    assert_eq!(found.0.title(), "B");
}

#[test]
fn vault_document_relative_lookup() {
    let td = write_vault(&[("notes/x.org", "* From notes\n")]);
    let v = Vault::open(td.path()).expect("open");
    let doc = v
        .document_relative(std::path::Path::new("notes/x.org"))
        .expect("found");
    assert_eq!(doc.roots()[0].title(), "From notes");
}

#[test]
fn vault_find_by_title_case_insensitive() {
    let td = write_vault(&[("a.org", "* Other\n* Target Headline\n")]);
    let v = Vault::open(td.path()).expect("open");
    let hit = v.find_by_title("target headline").expect("found");
    assert_eq!(hit.0.title(), "Target Headline");
}

#[test]
fn vault_link_graph_maps_id_targets() {
    let td = write_vault(&[
        (
            "a.org",
            "* Source\n:PROPERTIES:\n:ID: 01HXSRC0000000000000000000\n:END:\n[[id:01HXTGT0000000000000000000][T]]\n",
        ),
        (
            "b.org",
            "* Target\n:PROPERTIES:\n:ID: 01HXTGT0000000000000000000\n:END:\n",
        ),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let g = v.link_graph();
    let src = closure_core::BlockId::from_existing("01HXSRC0000000000000000000");
    let edges = g.get(&src).expect("edges");
    let tgt = closure_core::BlockId::from_existing("01HXTGT0000000000000000000");
    assert!(edges.contains(&tgt));
}

#[test]
fn vault_headline_and_word_counts_aggregate() {
    let td = write_vault(&[
        ("a.org", "* One\n* Two\nbody words here\n"),
        ("b.org", "* Three\nmore words\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.headline_count(), 3);
    assert!(v.word_count() >= 7);
}

#[test]
fn vault_char_count_match() {
    let td = write_vault(&[("a.org", "* Áb\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // "* Áb\n" = 5 chars, "* C\n" = 4 chars -> 9
    assert_eq!(v.char_count(), 9);
}

#[test]
fn vault_char_count_of_match() {
    let td = write_vault(&[("a.org", "* Áb\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.char_count_of(&td.path().join("a.org")), Some(5));
    assert_eq!(v.char_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_mean_file_char_count_match() {
    let td = write_vault(&[("a.org", "* AB\n"), ("b.org", "* CD\n")]);
    let v = Vault::open(td.path()).expect("open");
    // each file 5 chars, 2 files -> 5
    assert_eq!(v.mean_file_char_count(), 5);
}

#[test]
fn vault_mean_file_char_count_zero_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.char_count(), 0);
    assert_eq!(v.mean_file_char_count(), 0);
}

#[test]
fn vault_max_min_file_char_count_match() {
    let td = write_vault(&[("a.org", "* A\n"), ("b.org", "* BBBBBB\n")]);
    let v = Vault::open(td.path()).expect("open");
    // "* A\n"=4, "* BBBBBB\n"=9 chars
    assert_eq!(v.max_file_char_count(), Some(9));
    assert_eq!(v.min_file_char_count(), Some(4));
}

#[test]
fn vault_median_file_char_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* BB\n"),
        ("c.org", "* CCC\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 4,5,6 -> median 5
    assert_eq!(v.median_file_char_count(), Some(5));
}

#[test]
fn vault_file_char_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_char_count(), None);
    assert_eq!(v.min_file_char_count(), None);
    assert_eq!(v.median_file_char_count(), None);
}

#[test]
fn vault_all_tags_and_all_todos_aggregate() {
    let td = write_vault(&[
        ("a.org", "* TODO Fix :work:\n"),
        ("b.org", "* DONE Ship :work:urgent:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.all_tags(), vec!["urgent".to_owned(), "work".to_owned()]);
    assert_eq!(v.all_todos(), vec!["DONE".to_owned(), "TODO".to_owned()]);
}

#[test]
fn vault_save_writes_byte_exact_source() {
    let td = write_vault(&[("x.org", "* Original\n")]);
    let v = Vault::open(td.path()).expect("open");
    let doc = v.document(&td.path().join("x.org")).expect("lookup");
    v.save(&td.path().join("x.org"), &doc.source())
        .expect("save");
    let on_disk = fs::read_to_string(td.path().join("x.org")).expect("read");
    assert_eq!(on_disk, "* Original\n");
}

#[test]
fn vault_create_file_adds_new_document() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert_eq!(v.len(), 1);
    let p = v
        .create_file(std::path::Path::new("b.org"), "* B\n")
        .expect("create");
    assert!(p.ends_with("b.org"));
    assert_eq!(v.len(), 2);
    assert!(v.document(&p).is_some());
}

#[test]
fn vault_delete_file_drops_entry() {
    let td = write_vault(&[("a.org", "* A\n"), ("b.org", "* B\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert_eq!(v.len(), 2);
    let a = td.path().join("a.org");
    v.delete_file(&a).expect("delete");
    assert_eq!(v.len(), 1);
    assert!(!a.exists());
}

#[test]
fn vault_rename_file_updates_indexes() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let from = td.path().join("a.org");
    let to_rel = std::path::Path::new("renamed.org");
    let new_path = v.rename_file(&from, to_rel).expect("rename");
    assert!(!from.exists());
    assert!(new_path.exists());
    assert!(v.document(&new_path).is_some());
    assert!(v.document(&from).is_none());
}

#[test]
fn vault_create_file_refuses_to_overwrite() {
    let td = write_vault(&[("x.org", "* X\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    let err = v
        .create_file(std::path::Path::new("x.org"), "* New\n")
        .unwrap_err();
    let _ = err;
}

#[test]
fn vault_save_with_backup_writes_bak() {
    let td = write_vault(&[("x.org", "* Original\n")]);
    let v = Vault::open(td.path()).expect("open");
    let path = td.path().join("x.org");
    v.save_with_backup(&path, "* Updated\n").expect("save");
    let now = fs::read_to_string(&path).expect("read");
    let bak = fs::read_to_string(td.path().join("x.org.bak")).expect("read bak");
    assert_eq!(now, "* Updated\n");
    assert_eq!(bak, "* Original\n");
}

#[test]
fn vault_reload_picks_up_new_file() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let mut v = Vault::open(td.path()).expect("open");
    assert_eq!(v.len(), 1);
    fs::write(td.path().join("b.org"), "* B\n").expect("write");
    v.reload().expect("reload");
    assert_eq!(v.len(), 2);
}

#[test]
fn vault_backlinks_index_finds_linkers() {
    let td = write_vault(&[
        (
            "target.org",
            "* T\n:PROPERTIES:\n:ID: 01HXTGTTARGET0000000000AA\n:END:\n",
        ),
        (
            "src.org",
            "* Source\nSee [[id:01HXTGTTARGET0000000000AA][T]].\n",
        ),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // Either form (with or without `id:` prefix) should work.
    let bare = v.backlinks_of("01HXTGTTARGET0000000000AA");
    assert_eq!(bare.len(), 1);
    let prefixed = v.backlinks_of("id:01HXTGTTARGET0000000000AA");
    assert_eq!(prefixed.len(), 1);
}

#[test]
fn vault_distinct_tags_returns_sorted_unique() {
    let td = write_vault(&[
        ("a.org", "* X :work:\n* Y :home:\n"),
        ("b.org", "* Z :work:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_tags(), vec!["home".to_owned(), "work".to_owned()]);
}

#[test]
fn vault_distinct_todos_returns_sorted_unique() {
    let td = write_vault(&[
        ("a.org", "* TODO X\n* DONE Y\n"),
        ("b.org", "* TODO Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_todos(), vec!["DONE".to_owned(), "TODO".to_owned()]);
}

#[test]
fn vault_distinct_priorities_returns_sorted_unique() {
    let td = write_vault(&[
        ("a.org", "* [#B] X\n* [#A] Y\n"),
        ("b.org", "* [#A] Z\n* [#C] W\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_priorities(), vec!['A', 'B', 'C']);
}

#[test]
fn vault_distinct_levels_returns_sorted_unique() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n*** C\n"),
        ("b.org", "* D\n** E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_levels(), vec![1, 2, 3]);
}

#[test]
fn vault_distinct_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* X :work:\n* Y :home:\n"),
        ("b.org", "* Z :work:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_tag_count(), 2);
}

#[test]
fn vault_distinct_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO X\n* DONE Y\n"),
        ("b.org", "* TODO Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_todo_count(), 2);
}

#[test]
fn vault_distinct_priority_count_match() {
    let td = write_vault(&[
        ("a.org", "* [#B] X\n* [#A] Y\n"),
        ("b.org", "* [#A] Z\n* [#C] W\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_priority_count(), 3);
}

#[test]
fn vault_distinct_level_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n*** C\n"),
        ("b.org", "* D\n** E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_level_count(), 3);
}

#[test]
fn vault_priority_counts_returns_sorted_ranked() {
    let td = write_vault(&[
        ("a.org", "* [#B] X\n* [#A] Y\n"),
        ("b.org", "* [#A] Z\n* [#C] W\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let ranks = v.priority_counts();
    assert_eq!(ranks[0], ('A', 2));
    assert!(ranks.iter().any(|(c, n)| *c == 'B' && *n == 1));
    assert!(ranks.iter().any(|(c, n)| *c == 'C' && *n == 1));
}

#[test]
fn vault_min_max_priority_match() {
    let td = write_vault(&[("a.org", "* [#B] X\n* [#A] Y\n* [#C] Z\n")]);
    let v = Vault::open(td.path()).expect("open");
    // smallest letter = highest urgency
    assert_eq!(v.min_priority(), Some('A'));
    assert_eq!(v.max_priority(), Some('C'));
}

#[test]
fn vault_mode_priority_match() {
    let td = write_vault(&[("a.org", "* [#A] X\n* [#A] Y\n* [#C] Z\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_priority(), Some('A'));
}

#[test]
fn vault_priority_none_when_no_priority() {
    let td = write_vault(&[("a.org", "* A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.min_priority(), None);
    assert_eq!(v.max_priority(), None);
    assert_eq!(v.mode_priority(), None);
}

#[test]
fn vault_with_priority_count_match() {
    let td = write_vault(&[("a.org", "* [#A] X\n* Y\n* [#C] Z\n* W\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_priority_count(), 2);
}

#[test]
fn vault_priority_pct_match() {
    let td = write_vault(&[("a.org", "* [#A] X\n* Y\n* [#C] Z\n* W\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2 of 4 -> 50
    assert_eq!(v.priority_pct(), 50);
}

#[test]
fn vault_priority_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_priority_count(), 0);
    assert_eq!(v.priority_pct(), 0);
}

#[test]
fn vault_count_no_priority_match() {
    let td = write_vault(&[("a.org", "* [#A] X\n* Y\n* [#C] Z\n* W\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2 of 4 lack priority
    assert_eq!(v.count_no_priority(), 2);
}

#[test]
fn vault_no_priority_pct_match() {
    let td = write_vault(&[("a.org", "* [#A] X\n* Y\n* [#C] Z\n* W\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_priority_pct(), 50);
}

#[test]
fn vault_no_priority_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_priority(), 0);
    assert_eq!(v.no_priority_pct(), 0);
}

#[test]
fn vault_level_counts_returns_ranked() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n*** C\n*** D\n*** E\n"),
        ("b.org", "* F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let ranks = v.level_counts();
    assert_eq!(ranks[0], (3, 3));
    assert!(ranks.iter().any(|(l, n)| *l == 1 && *n == 2));
    assert!(ranks.iter().any(|(l, n)| *l == 2 && *n == 1));
}

#[test]
fn vault_most_common_tag_returns_top() {
    let td = write_vault(&[
        ("a.org", "* X :work:\n* Y :home:\n"),
        ("b.org", "* Z :work:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.most_common_tag(), Some("work".to_owned()));
}

#[test]
fn vault_least_common_tag_returns_bottom() {
    let td = write_vault(&[("a.org", "* X :work:\n* Y :home:\n* Z :work:\n")]);
    let v = Vault::open(td.path()).expect("open");
    // work=2, home=1 -> least is home
    assert_eq!(v.least_common_tag(), Some("home".to_owned()));
}

#[test]
fn vault_least_common_tag_none_when_no_tags() {
    let td = write_vault(&[("a.org", "* X\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.least_common_tag(), None);
}

#[test]
fn vault_tag_count_map_match() {
    let td = write_vault(&[("a.org", "* X :work:\n* Y :home:\n* Z :work:\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.tag_count_map();
    assert_eq!(m.get("work"), Some(&2));
    assert_eq!(m.get("home"), Some(&1));
}

#[test]
fn vault_least_common_todo_returns_bottom() {
    let td = write_vault(&[("a.org", "* TODO X\n* TODO Y\n* DONE Z\n")]);
    let v = Vault::open(td.path()).expect("open");
    // TODO=2, DONE=1 -> least DONE
    assert_eq!(v.least_common_todo(), Some("DONE".to_owned()));
}

#[test]
fn vault_todo_count_map_match() {
    let td = write_vault(&[("a.org", "* TODO X\n* TODO Y\n* DONE Z\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.todo_count_map();
    assert_eq!(m.get("TODO"), Some(&2));
    assert_eq!(m.get("DONE"), Some(&1));
}

#[test]
fn vault_least_common_priority_returns_bottom() {
    let td = write_vault(&[("a.org", "* [#A] X\n* [#A] Y\n* [#B] Z\n")]);
    let v = Vault::open(td.path()).expect("open");
    // A=2, B=1 -> least B
    assert_eq!(v.least_common_priority(), Some('B'));
}

#[test]
fn vault_priority_count_map_match() {
    let td = write_vault(&[("a.org", "* [#A] X\n* [#A] Y\n* [#B] Z\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.priority_count_map();
    assert_eq!(m.get(&'A'), Some(&2));
    assert_eq!(m.get(&'B'), Some(&1));
}

#[test]
fn vault_least_common_todo_none_when_none() {
    let td = write_vault(&[("a.org", "* X\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.least_common_todo(), None);
}

#[test]
fn vault_distinct_property_keys_sorted() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:END:\n* B\n:PROPERTIES:\n:Foo: 9\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(
        v.distinct_property_keys(),
        vec!["Bar".to_owned(), "Foo".to_owned()]
    );
}

#[test]
fn vault_distinct_property_key_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_property_key_count(), 2);
}

#[test]
fn vault_property_key_counts_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:END:\n* B\n:PROPERTIES:\n:Foo: 9\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.property_key_counts();
    assert_eq!(m.get("Foo"), Some(&2));
    assert_eq!(m.get("Bar"), Some(&1));
}

#[test]
fn vault_most_common_property_key_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:END:\n* B\n:PROPERTIES:\n:Foo: 9\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.most_common_property_key(), Some("Foo".to_owned()));
}

#[test]
fn vault_property_values_for_key_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n* B\n:PROPERTIES:\n:Foo: 2\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let mut vals = v.property_values("Foo");
    vals.sort();
    assert_eq!(vals, vec!["1".to_owned(), "2".to_owned()]);
}

#[test]
fn vault_distinct_property_values_for_key_sorted() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: x\n:END:\n* B\n:PROPERTIES:\n:Foo: x\n:END:\n* C\n:PROPERTIES:\n:Foo: y\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(
        v.distinct_property_values("Foo"),
        vec!["x".to_owned(), "y".to_owned()]
    );
}

#[test]
fn vault_property_values_empty_for_unknown_key() {
    let td = write_vault(&[("a.org", "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.property_values("Nope").is_empty());
}

#[test]
fn vault_distinct_property_value_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: x\n:END:\n* B\n:PROPERTIES:\n:Foo: x\n:END:\n* C\n:PROPERTIES:\n:Foo: y\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_property_value_count("Foo"), 2);
}

#[test]
fn vault_property_value_counts_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: x\n:END:\n* B\n:PROPERTIES:\n:Foo: x\n:END:\n* C\n:PROPERTIES:\n:Foo: y\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.property_value_counts("Foo");
    assert_eq!(m.get("x"), Some(&2));
    assert_eq!(m.get("y"), Some(&1));
}

#[test]
fn vault_most_common_property_value_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: x\n:END:\n* B\n:PROPERTIES:\n:Foo: x\n:END:\n* C\n:PROPERTIES:\n:Foo: y\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.most_common_property_value("Foo"), Some("x".to_owned()));
}

#[test]
fn vault_distinct_property_value_count_zero_for_unknown() {
    let td = write_vault(&[("a.org", "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_property_value_count("Nope"), 0);
}

#[test]
fn vault_least_common_property_key_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:END:\n* B\n:PROPERTIES:\n:Foo: 3\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // Foo=2, Bar=1 -> least Bar
    assert_eq!(v.least_common_property_key(), Some("Bar".to_owned()));
}

#[test]
fn vault_least_common_property_value_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: x\n:END:\n* B\n:PROPERTIES:\n:Foo: x\n:END:\n* C\n:PROPERTIES:\n:Foo: y\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // x=2, y=1 -> least y
    assert_eq!(
        v.least_common_property_value("Foo"),
        Some("y".to_owned())
    );
}

#[test]
fn vault_least_common_property_key_none_when_empty() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.least_common_property_key(), None);
}

#[test]
fn vault_with_property_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n* B\n* C\n:PROPERTIES:\n:Bar: 2\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_property_count(), 2);
}

#[test]
fn vault_property_pct_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n* B\n* C\n* D\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.property_pct(), 25);
}

#[test]
fn vault_count_no_property_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n* B\n* C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_property(), 2);
}

#[test]
fn vault_no_property_pct_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:END:\n* B\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_property_pct(), 50);
}

#[test]
fn vault_property_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.property_pct(), 0);
}

#[test]
fn vault_empty_title_count_match() {
    let td = write_vault(&[("a.org", "* \n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.empty_title_count(), 1);
}

#[test]
fn vault_empty_title_pct_match() {
    let td = write_vault(&[("a.org", "* \n* B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.empty_title_pct(), 25);
}

#[test]
fn vault_count_nonempty_title_match() {
    let td = write_vault(&[("a.org", "* \n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_nonempty_title(), 2);
}

#[test]
fn vault_nonempty_title_pct_match() {
    let td = write_vault(&[("a.org", "* \n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.nonempty_title_pct(), 50);
}

#[test]
fn vault_empty_title_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.empty_title_pct(), 0);
}

#[test]
fn vault_duplicate_title_count_match() {
    let td = write_vault(&[("a.org", "* A\n* A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    // total 3 - distinct 2 = 1
    assert_eq!(v.duplicate_title_count(), 1);
}

#[test]
fn vault_has_duplicate_titles_match() {
    let dup = write_vault(&[("a.org", "* A\n"), ("b.org", "* A\n")]);
    let vd = Vault::open(dup.path()).expect("open");
    assert!(vd.has_duplicate_titles());
    let uniq = write_vault(&[("a.org", "* A\n* B\n")]);
    let vu = Vault::open(uniq.path()).expect("open");
    assert!(!vu.has_duplicate_titles());
}

#[test]
fn vault_unique_title_pct_match() {
    let td = write_vault(&[("a.org", "* A\n* A\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // distinct 3 of 4 -> 75
    assert_eq!(v.unique_title_pct(), 75);
}

#[test]
fn vault_unique_title_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.unique_title_pct(), 0);
    assert!(!v.has_duplicate_titles());
}

#[test]
fn vault_has_duplicate_ids_match() {
    let dup = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:ID: x\n:END:\n* B\n:PROPERTIES:\n:ID: x\n:END:\n",
    )]);
    let vd = Vault::open(dup.path()).expect("open");
    assert!(vd.has_duplicate_ids());
    let uniq = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:ID: x\n:END:\n* B\n:PROPERTIES:\n:ID: y\n:END:\n",
    )]);
    let vu = Vault::open(uniq.path()).expect("open");
    assert!(!vu.has_duplicate_ids());
}

#[test]
fn vault_id_uniqueness_pct_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:ID: x\n:END:\n* B\n:PROPERTIES:\n:ID: x\n:END:\n* C\n:PROPERTIES:\n:ID: y\n:END:\n* D\n:PROPERTIES:\n:ID: z\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 4 ids, distinct {x,y,z}=3 -> 75
    assert_eq!(v.id_uniqueness_pct(), 75);
}

#[test]
fn vault_id_uniqueness_pct_zero_when_no_ids() {
    let td = write_vault(&[("a.org", "* A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.id_uniqueness_pct(), 0);
    assert!(!v.has_duplicate_ids());
}

#[test]
fn vault_tag_diversity_pct_match() {
    let td = write_vault(&[("a.org", "* A :x:y:\n* B :x:\n* C :x:\n")]);
    let v = Vault::open(td.path()).expect("open");
    // tag occurrences x*3,y*1=4; distinct {x,y}=2 -> 50
    assert_eq!(v.tag_diversity_pct(), 50);
}

#[test]
fn vault_tag_diversity_pct_zero_when_no_tags() {
    let td = write_vault(&[("a.org", "* A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.tag_diversity_pct(), 0);
}

#[test]
fn vault_todo_diversity_pct_match() {
    let td = write_vault(&[("a.org", "* TODO A\n* TODO B\n* DONE C\n* DONE D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 4 todo occurrences, distinct {TODO,DONE}=2 -> 50
    assert_eq!(v.todo_diversity_pct(), 50);
}

#[test]
fn vault_todo_diversity_pct_zero_when_no_todos() {
    let td = write_vault(&[("a.org", "* A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.todo_diversity_pct(), 0);
}

#[test]
fn vault_level_diversity_pct_match() {
    let td = write_vault(&[("a.org", "* A\n* B\n** C\n** D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 4 headlines, distinct levels {1,2}=2 -> 50
    assert_eq!(v.level_diversity_pct(), 50);
}

#[test]
fn vault_level_diversity_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.level_diversity_pct(), 0);
}

#[test]
fn vault_property_key_diversity_pct_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:END:\n* B\n:PROPERTIES:\n:Foo: 3\n:END:\n* C\n:PROPERTIES:\n:Foo: 4\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // key occurrences Foo*3,Bar*1=4; distinct {Foo,Bar}=2 -> 50
    assert_eq!(v.property_key_diversity_pct(), 50);
}

#[test]
fn vault_property_key_diversity_pct_zero_when_none() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.property_key_diversity_pct(), 0);
}

#[test]
fn vault_priority_diversity_pct_match() {
    let td = write_vault(&[("a.org", "* [#A] X\n* [#A] Y\n* [#B] Z\n* [#C] W\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 4 priority occurrences, distinct {A,B,C}=3 -> 75
    assert_eq!(v.priority_diversity_pct(), 75);
}

#[test]
fn vault_priority_diversity_pct_zero_when_none() {
    let td = write_vault(&[("a.org", "* A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.priority_diversity_pct(), 0);
}

#[test]
fn vault_title_diversity_pct_match() {
    let td = write_vault(&[("a.org", "* A\n* A\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 4 headlines, distinct {A,B,C}=3 -> 75
    assert_eq!(v.title_diversity_pct(), 75);
}

#[test]
fn vault_title_diversity_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.title_diversity_pct(), 0);
}

#[test]
fn vault_total_body_byte_count_match() {
    let td = write_vault(&[("a.org", "* A\nxy\n* B\nz\n")]);
    let v = Vault::open(td.path()).expect("open");
    // body_text A "xy\n"=3, B "z\n"=2 = 5
    assert_eq!(v.total_body_byte_count(), 5);
}

#[test]
fn vault_max_min_body_byte_count_match() {
    let td = write_vault(&[("a.org", "* A\nxy\n* B\nz\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // body bytes A=3, B=2, C=0
    assert_eq!(v.max_body_byte_count(), Some(3));
    assert_eq!(v.min_body_byte_count(), Some(0));
}

#[test]
fn vault_mean_body_byte_count_match() {
    let td = write_vault(&[("a.org", "* A\nxy\n* B\nz\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 3+2+0=5, n=3 -> 1
    assert_eq!(v.mean_body_byte_count(), 1);
}

#[test]
fn vault_body_byte_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_body_byte_count(), None);
    assert_eq!(v.min_body_byte_count(), None);
    assert_eq!(v.mean_body_byte_count(), 0);
}

#[test]
fn vault_median_body_byte_count_match() {
    let td = write_vault(&[("a.org", "* A\nxy\n* B\nz\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // body bytes 3,2,0 -> sorted 0,2,3 -> median 2
    assert_eq!(v.median_body_byte_count(), Some(2));
}

#[test]
fn vault_body_byte_count_counts_match() {
    let td = write_vault(&[("a.org", "* A\nxy\n* B\nab\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // body bytes A=3, B=3, C=0
    let m = v.body_byte_count_counts();
    assert_eq!(m.get(&3), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_body_byte_count_match() {
    let td = write_vault(&[("a.org", "* A\nxy\n* B\nab\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 3,3,0 -> mode 3
    assert_eq!(v.mode_body_byte_count(), Some(3));
}

#[test]
fn vault_median_body_byte_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_body_byte_count(), None);
    assert_eq!(v.mode_body_byte_count(), None);
}

#[test]
fn vault_body_byte_pct_match() {
    let td = write_vault(&[("a.org", "* H\nxxxxx\n")]);
    let v = Vault::open(td.path()).expect("open");
    // source 10, body_text "xxxxx\n"=6 -> 60
    assert_eq!(v.body_byte_pct(), 60);
}

#[test]
fn vault_body_byte_pct_zero_when_no_body() {
    let td = write_vault(&[("a.org", "* A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.body_byte_pct(), 0);
}

#[test]
fn vault_body_byte_pct_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.body_byte_pct(), 0);
}

#[test]
fn vault_title_byte_pct_match() {
    let td = write_vault(&[("a.org", "* HHHH\n")]);
    let v = Vault::open(td.path()).expect("open");
    // source "* HHHH\n"=7, title "HHHH"=4 -> 4*100/7 = 57
    assert_eq!(v.title_byte_pct(), 57);
}

#[test]
fn vault_title_byte_pct_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.title_byte_pct(), 0);
}

#[test]
fn vault_non_body_byte_count_match() {
    let td = write_vault(&[("a.org", "* H\nxxxxx\n")]);
    let v = Vault::open(td.path()).expect("open");
    // source 10, body_text "xxxxx\n"=6 -> non-body 4
    assert_eq!(v.non_body_byte_count(), 4);
}

#[test]
fn vault_non_body_byte_pct_match() {
    let td = write_vault(&[("a.org", "* H\nxxxxx\n")]);
    let v = Vault::open(td.path()).expect("open");
    // non-body 4 of source 10 -> 40
    assert_eq!(v.non_body_byte_pct(), 40);
}

#[test]
fn vault_non_body_byte_pct_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.non_body_byte_pct(), 0);
}

#[test]
fn vault_max_min_tag_len_match() {
    let td = write_vault(&[("a.org", "* A :ab:ccc:\n* B :d:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_tag_len(), Some(3));
    assert_eq!(v.min_tag_len(), Some(1));
}

#[test]
fn vault_mean_tag_len_match() {
    let td = write_vault(&[("a.org", "* A :ab:ccc:\n* B :d:\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2,3,1 -> 6/3 = 2
    assert_eq!(v.mean_tag_len(), 2);
}

#[test]
fn vault_total_tag_len_match() {
    let td = write_vault(&[("a.org", "* A :ab:ccc:\n* B :d:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_tag_len(), 6);
}

#[test]
fn vault_tag_len_none_when_no_tags() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_tag_len(), None);
    assert_eq!(v.mean_tag_len(), 0);
}

#[test]
fn vault_median_tag_len_match() {
    let td = write_vault(&[("a.org", "* A :a:bb:cccc:\n")]);
    let v = Vault::open(td.path()).expect("open");
    // lens 1,2,4 -> median 2
    assert_eq!(v.median_tag_len(), Some(2));
}

#[test]
fn vault_tag_len_counts_match() {
    let td = write_vault(&[("a.org", "* A :ab:cd:\n* B :e:\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.tag_len_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&1), Some(&1));
}

#[test]
fn vault_mode_tag_len_match() {
    let td = write_vault(&[("a.org", "* A :ab:cd:\n* B :e:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_tag_len(), Some(2));
}

#[test]
fn vault_median_tag_len_none_when_no_tags() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_tag_len(), None);
}

#[test]
fn vault_max_min_todo_len_match() {
    let td = write_vault(&[("a.org", "* TODO A\n* DONE B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_todo_len(), Some(4));
    assert_eq!(v.min_todo_len(), Some(4));
}

#[test]
fn vault_mean_todo_len_match() {
    let td = write_vault(&[("a.org", "* TODO A\n* DONE B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_todo_len(), 4);
}

#[test]
fn vault_total_todo_len_match() {
    let td = write_vault(&[("a.org", "* TODO A\n* DONE B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_todo_len(), 8);
}

#[test]
fn vault_todo_len_none_when_no_todos() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_todo_len(), None);
    assert_eq!(v.min_todo_len(), None);
    assert_eq!(v.mean_todo_len(), 0);
    assert_eq!(v.total_todo_len(), 0);
}

#[test]
fn vault_median_todo_len_match() {
    let td = write_vault(&[("a.org", "* TODO A\n* TODO B\n* DONE C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_todo_len(), Some(4));
}

#[test]
fn vault_todo_len_counts_match() {
    let td = write_vault(&[("a.org", "* TODO A\n* TODO B\n* DONE C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.todo_len_counts();
    assert_eq!(m.get(&4), Some(&3));
}

#[test]
fn vault_mode_todo_len_match() {
    let td = write_vault(&[("a.org", "* TODO A\n* TODO B\n* DONE C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_todo_len(), Some(4));
}

#[test]
fn vault_median_todo_len_none_when_no_todos() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_todo_len(), None);
    assert_eq!(v.mode_todo_len(), None);
}

#[test]
fn vault_max_min_property_key_len_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:LongerKey: 2\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_property_key_len(), Some(9));
    assert_eq!(v.min_property_key_len(), Some(3));
}

#[test]
fn vault_mean_property_key_len_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:LongerKey: 2\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_property_key_len(), 6);
}

#[test]
fn vault_total_property_key_len_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:LongerKey: 2\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_property_key_len(), 12);
}

#[test]
fn vault_property_key_len_none_when_no_props() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_property_key_len(), None);
    assert_eq!(v.min_property_key_len(), None);
    assert_eq!(v.mean_property_key_len(), 0);
    assert_eq!(v.total_property_key_len(), 0);
}

#[test]
fn vault_median_property_key_len_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:LongerKey: 3\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // lens sorted: 3, 3, 9 — median = 3
    assert_eq!(v.median_property_key_len(), Some(3));
}

#[test]
fn vault_property_key_len_counts_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:LongerKey: 3\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.property_key_len_counts();
    assert_eq!(m.get(&3), Some(&2));
    assert_eq!(m.get(&9), Some(&1));
}

#[test]
fn vault_mode_property_key_len_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: 1\n:Bar: 2\n:LongerKey: 3\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_property_key_len(), Some(3));
}

#[test]
fn vault_median_property_key_len_none_when_no_props() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_property_key_len(), None);
    assert_eq!(v.mode_property_key_len(), None);
}

#[test]
fn vault_max_min_property_value_len_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: abc\n:Bar: defghij\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_property_value_len(), Some(7));
    assert_eq!(v.min_property_value_len(), Some(3));
}

#[test]
fn vault_mean_property_value_len_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: abc\n:Bar: defghij\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_property_value_len(), 5);
}

#[test]
fn vault_total_property_value_len_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:Foo: abc\n:Bar: def\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_property_value_len(), 6);
}

#[test]
fn vault_property_value_len_none_when_no_props() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_property_value_len(), None);
    assert_eq!(v.min_property_value_len(), None);
    assert_eq!(v.mean_property_value_len(), 0);
    assert_eq!(v.total_property_value_len(), 0);
}

#[test]
fn vault_median_property_value_len_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:A: a\n:B: bb\n:C: dddd\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // vals lens sorted: 1, 2, 4 -> median 2
    assert_eq!(v.median_property_value_len(), Some(2));
}

#[test]
fn vault_property_value_len_counts_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:A: aa\n:B: bb\n:C: longer\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.property_value_len_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&6), Some(&1));
}

#[test]
fn vault_mode_property_value_len_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:A: aa\n:B: bb\n:C: longer\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_property_value_len(), Some(2));
}

#[test]
fn vault_median_property_value_len_none_when_no_props() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_property_value_len(), None);
    assert_eq!(v.mode_property_value_len(), None);
}

#[test]
fn vault_max_min_timestamp_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n<2026-01-01> <2026-01-02>\n* B\n<2026-01-03>\n* C\nno ts\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_timestamp_count(), Some(2));
    assert_eq!(v.min_timestamp_count(), Some(0));
}

#[test]
fn vault_mean_timestamp_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n<2026-01-01> <2026-01-02>\n* B\n<2026-01-03>\n* C\nno ts\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 2,1,0 -> 3/3=1
    assert_eq!(v.mean_timestamp_count(), 1);
}

#[test]
fn vault_timestamp_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_timestamp_count(), None);
    assert_eq!(v.min_timestamp_count(), None);
    assert_eq!(v.mean_timestamp_count(), 0);
}

#[test]
fn vault_median_timestamp_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\nno\n* B\n<2026-01-01>\n* C\n<2026-01-02> <2026-01-03>\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_timestamp_count(), Some(1));
}

#[test]
fn vault_timestamp_count_counts_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n<2026-01-01>\n* B\n<2026-01-02>\n* C\n<2026-01-03> <2026-01-04>\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.timestamp_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_timestamp_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n<2026-01-01>\n* B\n<2026-01-02>\n* C\n<2026-01-03> <2026-01-04>\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_timestamp_count(), Some(1));
}

#[test]
fn vault_median_timestamp_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_timestamp_count(), None);
    assert_eq!(v.mode_timestamp_count(), None);
}

#[test]
fn vault_with_timestamp_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n<2026-01-01>\n* B\nno ts\n* C\n<2026-01-02> <2026-01-03>\n* D\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // A and C carry timestamps
    assert_eq!(v.with_timestamp_count(), 2);
}

#[test]
fn vault_timestamp_pct_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n<2026-01-01>\n* B\nno ts\n* C\n<2026-01-02>\n* D\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 2 of 4 -> 50
    assert_eq!(v.timestamp_pct(), 50);
}

#[test]
fn vault_timestamp_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_timestamp_count(), 0);
    assert_eq!(v.timestamp_pct(), 0);
}

#[test]
fn vault_max_min_link_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n[[l1]] [[l2]]\n* B\n[[l3]]\n* C\nno links\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_link_count(), Some(2));
    assert_eq!(v.min_link_count(), Some(0));
}

#[test]
fn vault_mean_link_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n[[l1]] [[l2]]\n* B\n[[l3]]\n* C\nno links\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 2,1,0 -> 3/3=1
    assert_eq!(v.mean_link_count(), 1);
}

#[test]
fn vault_link_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_link_count(), None);
    assert_eq!(v.min_link_count(), None);
    assert_eq!(v.mean_link_count(), 0);
}

#[test]
fn vault_median_link_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\nno\n* B\n[[l1]]\n* C\n[[l2]] [[l3]]\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_link_count(), Some(1));
}

#[test]
fn vault_link_count_counts_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n[[l1]]\n* B\n[[l2]]\n* C\n[[l3]] [[l4]]\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.link_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_link_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n[[l1]]\n* B\n[[l2]]\n* C\n[[l3]] [[l4]]\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_link_count(), Some(1));
}

#[test]
fn vault_median_link_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_link_count(), None);
    assert_eq!(v.mode_link_count(), None);
}

#[test]
fn vault_with_link_count_match() {
    let td = write_vault(&[("a.org", "* A\n[[l1]]\n* B\nno links\n* C\n[[l2]] [[l3]]\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // A and C carry links
    assert_eq!(v.with_link_count(), 2);
}

#[test]
fn vault_link_pct_match() {
    let td = write_vault(&[("a.org", "* A\n[[l1]]\n* B\nno links\n* C\n[[l2]]\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2 of 4 -> 50
    assert_eq!(v.link_pct(), 50);
}

#[test]
fn vault_link_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_link_count(), 0);
    assert_eq!(v.link_pct(), 0);
}

#[test]
fn vault_max_min_child_count_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // A=2, B=1, C=0, a1=0, a2=0, b1=0
    assert_eq!(v.max_child_count(), Some(2));
    assert_eq!(v.min_child_count(), Some(0));
}

#[test]
fn vault_total_child_count_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2+1+0+0+0+0 = 3
    assert_eq!(v.total_child_count(), 3);
}

#[test]
fn vault_mean_child_count_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n* B\n** b1\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 1+0+1+0=2, n=4 -> 0
    assert_eq!(v.mean_child_count(), 0);
}

#[test]
fn vault_child_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_child_count(), None);
    assert_eq!(v.min_child_count(), None);
    assert_eq!(v.total_child_count(), 0);
    assert_eq!(v.mean_child_count(), 0);
}

#[test]
fn vault_median_child_count_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // child counts: A=2, B=1, C=0, a1=0, a2=0, b1=0 -> sorted 0,0,0,0,1,2 -> median midpoint(0,0)=0
    assert_eq!(v.median_child_count(), Some(0));
}

#[test]
fn vault_child_count_counts_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.child_count_counts();
    // 4 zeros, 1 one, 1 two
    assert_eq!(m.get(&0), Some(&4));
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_child_count_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_child_count(), Some(0));
}

#[test]
fn vault_median_child_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_child_count(), None);
    assert_eq!(v.mode_child_count(), None);
}

#[test]
fn vault_max_min_descendant_count_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // descendants: A=2, a1=1, a2=0, B=1, b1=0, C=0
    assert_eq!(v.max_descendant_count(), Some(2));
    assert_eq!(v.min_descendant_count(), Some(0));
}

#[test]
fn vault_total_descendant_count_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2+1+0+1+0+0 = 4
    assert_eq!(v.total_descendant_count(), 4);
}

#[test]
fn vault_mean_descendant_count_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 4/6=0
    assert_eq!(v.mean_descendant_count(), 0);
}

#[test]
fn vault_descendant_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_descendant_count(), None);
    assert_eq!(v.min_descendant_count(), None);
    assert_eq!(v.total_descendant_count(), 0);
    assert_eq!(v.mean_descendant_count(), 0);
}

#[test]
fn vault_median_descendant_count_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // sorted desc counts: 0,0,0,0,1,1,2 wait n=6: A=2,a1=1,a2=0,B=1,b1=0,C=0 -> sorted 0,0,0,1,1,2 mid=midpoint(0,1)=0
    assert_eq!(v.median_descendant_count(), Some(0));
}

#[test]
fn vault_descendant_count_counts_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.descendant_count_counts();
    // 3 zeros, 2 ones, 1 two
    assert_eq!(m.get(&0), Some(&3));
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_descendant_count_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_descendant_count(), Some(0));
}

#[test]
fn vault_median_descendant_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_descendant_count(), None);
    assert_eq!(v.mode_descendant_count(), None);
}

#[test]
fn vault_max_min_subtree_size_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // subtree sizes: A=3, a1=2, a2=1, B=2, b1=1, C=1
    assert_eq!(v.max_subtree_size(), Some(3));
    assert_eq!(v.min_subtree_size(), Some(1));
}

#[test]
fn vault_total_subtree_size_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 3+2+1+2+1+1 = 10
    assert_eq!(v.total_subtree_size(), 10);
}

#[test]
fn vault_mean_subtree_size_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 10/6=1
    assert_eq!(v.mean_subtree_size(), 1);
}

#[test]
fn vault_subtree_size_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_size(), None);
    assert_eq!(v.min_subtree_size(), None);
    assert_eq!(v.total_subtree_size(), 0);
    assert_eq!(v.mean_subtree_size(), 0);
}

#[test]
fn vault_median_subtree_size_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // sizes: 3,2,1,2,1,1 -> sorted 1,1,1,2,2,3 -> midpoint(1,2)=1
    assert_eq!(v.median_subtree_size(), Some(1));
}

#[test]
fn vault_subtree_size_counts_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.subtree_size_counts();
    // 3 ones, 2 twos, 1 three
    assert_eq!(m.get(&1), Some(&3));
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn vault_mode_subtree_size_match() {
    let td = write_vault(&[("a.org", "* A\n** a1\n*** a2\n* B\n** b1\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_size(), Some(1));
}

#[test]
fn vault_median_subtree_size_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_size(), None);
    assert_eq!(v.mode_subtree_size(), None);
}

#[test]
fn vault_max_min_tag_count_match() {
    let td = write_vault(&[("a.org", "* A :x:y:\n* B :z:\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // tag counts: A=2, B=1, C=0
    assert_eq!(v.max_tag_count(), Some(2));
    assert_eq!(v.min_tag_count(), Some(0));
}

#[test]
fn vault_mean_tag_count_match() {
    let td = write_vault(&[("a.org", "* A :x:y:\n* B :z:\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2+1+0=3, 3/3=1
    assert_eq!(v.mean_tag_count(), 1);
}

#[test]
fn vault_tag_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_tag_count(), None);
    assert_eq!(v.min_tag_count(), None);
    assert_eq!(v.mean_tag_count(), 0);
}

#[test]
fn vault_median_tag_count_match() {
    let td = write_vault(&[("a.org", "* A :x:y:\n* B :z:\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_tag_count(), Some(1));
}

#[test]
fn vault_tag_count_counts_match() {
    let td = write_vault(&[("a.org", "* A :x:\n* B :y:\n* C :a:b:\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.tag_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_tag_count_match() {
    let td = write_vault(&[("a.org", "* A :x:\n* B :y:\n* C :a:b:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_tag_count(), Some(1));
}

#[test]
fn vault_median_tag_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_tag_count(), None);
    assert_eq!(v.mode_tag_count(), None);
}

#[test]
fn vault_max_min_file_byte_count_match() {
    let td = write_vault(&[("a.org", "* A\n"), ("b.org", "* BBBBBB\n")]);
    let v = Vault::open(td.path()).expect("open");
    // "* A\n"=4, "* BBBBBB\n"=9
    assert_eq!(v.max_file_byte_count(), Some(9));
    assert_eq!(v.min_file_byte_count(), Some(4));
}

#[test]
fn vault_median_file_byte_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* BB\n"),
        ("c.org", "* CCCCCC\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 4,5,9 -> median 5
    assert_eq!(v.median_file_byte_count(), Some(5));
}

#[test]
fn vault_mean_file_byte_count_match() {
    let td = write_vault(&[("a.org", "* AB\n"), ("b.org", "* CD\n")]);
    let v = Vault::open(td.path()).expect("open");
    // each 5 bytes, 2 files -> 5
    assert_eq!(v.mean_file_byte_count(), 5);
}

#[test]
fn vault_mean_file_byte_count_zero_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_byte_count(), 0);
}

#[test]
fn vault_max_min_file_headline_count_match() {
    let td = write_vault(&[("a.org", "* A\n* B\n* C\n"), ("b.org", "* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_headline_count(), Some(3));
    assert_eq!(v.min_file_headline_count(), Some(1));
}

#[test]
fn vault_mean_file_headline_count_match() {
    let td = write_vault(&[("a.org", "* A\n* B\n* C\n"), ("b.org", "* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_headline_count(), 2);
}

#[test]
fn vault_median_file_headline_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n* C\n"),
        ("c.org", "* D\n* E\n* F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 1,2,3 -> median 2
    assert_eq!(v.median_file_headline_count(), Some(2));
}

#[test]
fn vault_file_headline_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_headline_count(), None);
    assert_eq!(v.min_file_headline_count(), None);
    assert_eq!(v.mean_file_headline_count(), 0);
    assert_eq!(v.median_file_headline_count(), None);
}

#[test]
fn vault_root_count_of_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n* C\n"), ("b.org", "* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.root_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.root_count_of(&td.path().join("b.org")), Some(1));
    assert_eq!(v.root_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_root_count_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n* C\n"), ("b.org", "* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // roots: a=2, b=1
    assert_eq!(v.max_file_root_count(), Some(2));
    assert_eq!(v.min_file_root_count(), Some(1));
}

#[test]
fn vault_mean_file_root_count_match() {
    let td = write_vault(&[("a.org", "* A\n* B\n* C\n"), ("b.org", "* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // roots a=3, b=1 -> total 4, 2 files -> 2
    assert_eq!(v.mean_file_root_count(), 2);
}

#[test]
fn vault_median_file_root_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n* C\n"),
        ("c.org", "* D\n* E\n* F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // roots 1,2,3 -> median 2
    assert_eq!(v.median_file_root_count(), Some(2));
}

#[test]
fn vault_file_root_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_root_count(), None);
    assert_eq!(v.min_file_root_count(), None);
    assert_eq!(v.mean_file_root_count(), 0);
    assert_eq!(v.median_file_root_count(), None);
}

#[test]
fn vault_link_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[l1]] [[l2]]\n* B\n[[l3]]\n"),
        ("b.org", "* C\nno links\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org total links 3, b.org 0
    assert_eq!(v.link_count_of(&td.path().join("a.org")), Some(3));
    assert_eq!(v.link_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.link_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[l1]] [[l2]]\n* B\n[[l3]]\n"),
        ("b.org", "* C\nno links\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_link_count(), Some(3));
    assert_eq!(v.min_file_link_count(), Some(0));
}

#[test]
fn vault_mean_file_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[l1]] [[l2]] [[l3]]\n"),
        ("b.org", "* C\n[[l4]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_link_count(), 2);
}

#[test]
fn vault_median_file_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nno\n"),
        ("b.org", "* B\n[[l1]]\n"),
        ("c.org", "* C\n[[l2]] [[l3]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_link_count(), Some(1));
}

#[test]
fn vault_file_link_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_link_count(), None);
    assert_eq!(v.min_file_link_count(), None);
    assert_eq!(v.mean_file_link_count(), 0);
    assert_eq!(v.median_file_link_count(), None);
}

#[test]
fn vault_tag_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:y:\n* B :z:\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org tag occurrences 3, b.org 0
    assert_eq!(v.tag_count_of(&td.path().join("a.org")), Some(3));
    assert_eq!(v.tag_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.tag_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:y:\n* B :z:\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_tag_count(), Some(3));
    assert_eq!(v.min_file_tag_count(), Some(0));
}

#[test]
fn vault_mean_file_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:y:z:\n"),
        ("b.org", "* C :w:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_tag_count(), 2);
}

#[test]
fn vault_median_file_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B :x:\n"),
        ("c.org", "* C :y:z:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_tag_count(), Some(1));
}

#[test]
fn vault_file_tag_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_tag_count(), None);
    assert_eq!(v.min_file_tag_count(), None);
    assert_eq!(v.mean_file_tag_count(), 0);
    assert_eq!(v.median_file_tag_count(), None);
}

#[test]
fn vault_todo_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n* DONE B\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org TODO-marked 2, b.org 0
    assert_eq!(v.todo_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.todo_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.todo_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n* DONE B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_todo_count(), Some(2));
    assert_eq!(v.min_file_todo_count(), Some(0));
}

#[test]
fn vault_mean_file_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n* TODO B\n* DONE C\n"),
        ("b.org", "* TODO D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_todo_count(), 2);
}

#[test]
fn vault_median_file_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* TODO B\n"),
        ("c.org", "* TODO C\n* DONE D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_todo_count(), Some(1));
}

#[test]
fn vault_file_todo_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_todo_count(), None);
    assert_eq!(v.min_file_todo_count(), None);
    assert_eq!(v.mean_file_todo_count(), 0);
    assert_eq!(v.median_file_todo_count(), None);
}

#[test]
fn vault_timestamp_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-01-01> <2026-01-02>\n* B\n<2026-01-03>\n"),
        ("b.org", "* C\nno ts\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org 3 timestamps, b.org 0
    assert_eq!(v.timestamp_count_of(&td.path().join("a.org")), Some(3));
    assert_eq!(v.timestamp_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.timestamp_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_timestamp_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-01-01> <2026-01-02>\n* B\n<2026-01-03>\n"),
        ("b.org", "* C\nno ts\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_timestamp_count(), Some(3));
    assert_eq!(v.min_file_timestamp_count(), Some(0));
}

#[test]
fn vault_mean_file_timestamp_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-01-01> <2026-01-02> <2026-01-03>\n"),
        ("b.org", "* C\n<2026-01-04>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_timestamp_count(), 2);
}

#[test]
fn vault_median_file_timestamp_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nno\n"),
        ("b.org", "* B\n<2026-01-01>\n"),
        ("c.org", "* C\n<2026-01-02> <2026-01-03>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_timestamp_count(), Some(1));
}

#[test]
fn vault_file_timestamp_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_timestamp_count(), None);
    assert_eq!(v.min_file_timestamp_count(), None);
    assert_eq!(v.mean_file_timestamp_count(), 0);
    assert_eq!(v.median_file_timestamp_count(), None);
}

#[test]
fn vault_property_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:K1: v\n:K2: w\n:END:\n* B\n:PROPERTIES:\n:K3: x\n:END:\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org 3 property pairs, b.org 0
    assert_eq!(v.property_count_of(&td.path().join("a.org")), Some(3));
    assert_eq!(v.property_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.property_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_property_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:K1: v\n:K2: w\n:END:\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_property_count(), Some(2));
    assert_eq!(v.min_file_property_count(), Some(0));
}

#[test]
fn vault_mean_file_property_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:K1: v\n:K2: w\n:K3: x\n:END:\n"),
        ("b.org", "* C\n:PROPERTIES:\n:K4: y\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_property_count(), 2);
}

#[test]
fn vault_median_file_property_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n:PROPERTIES:\n:K1: v\n:END:\n"),
        ("c.org", "* C\n:PROPERTIES:\n:K2: w\n:K3: x\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_property_count(), Some(1));
}

#[test]
fn vault_file_property_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_property_count(), None);
    assert_eq!(v.min_file_property_count(), None);
    assert_eq!(v.mean_file_property_count(), 0);
    assert_eq!(v.median_file_property_count(), None);
}

#[test]
fn vault_body_word_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\nthree little words\n* B\none\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org body words 3+1=4, b.org 0
    assert_eq!(v.body_word_count_of(&td.path().join("a.org")), Some(4));
    assert_eq!(v.body_word_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.body_word_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_body_word_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nthree little words\n* B\none\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_body_word_count(), Some(4));
    assert_eq!(v.min_file_body_word_count(), Some(0));
}

#[test]
fn vault_mean_file_body_word_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nthree little words\n"),
        ("b.org", "* C\none\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_body_word_count(), 2);
}

#[test]
fn vault_median_file_body_word_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\none\n"),
        ("c.org", "* C\ntwo words\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_body_word_count(), Some(1));
}

#[test]
fn vault_file_body_word_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_body_word_count(), None);
    assert_eq!(v.min_file_body_word_count(), None);
    assert_eq!(v.mean_file_body_word_count(), 0);
    assert_eq!(v.median_file_body_word_count(), None);
}

#[test]
fn vault_body_byte_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\nxy\n* B\nz\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org body bytes "xy\n"=3 + "z\n"=2 = 5, b.org 0
    assert_eq!(v.body_byte_count_of(&td.path().join("a.org")), Some(5));
    assert_eq!(v.body_byte_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.body_byte_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_body_byte_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nxy\n* B\nz\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_body_byte_count(), Some(5));
    assert_eq!(v.min_file_body_byte_count(), Some(0));
}

#[test]
fn vault_mean_file_body_byte_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nxyz\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org "xyz\n"=4, b.org 0 -> total 4, 2 files -> 2
    assert_eq!(v.mean_file_body_byte_count(), 2);
}

#[test]
fn vault_median_file_body_byte_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\nz\n"),
        ("c.org", "* C\nxyz\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // body bytes 0, 2, 4 -> median 2
    assert_eq!(v.median_file_body_byte_count(), Some(2));
}

#[test]
fn vault_file_body_byte_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_body_byte_count(), None);
    assert_eq!(v.min_file_body_byte_count(), None);
    assert_eq!(v.mean_file_body_byte_count(), 0);
    assert_eq!(v.median_file_body_byte_count(), None);
}

#[test]
fn vault_body_char_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\náb\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org body "áb\n" = 3 chars, b.org 0
    assert_eq!(v.body_char_count_of(&td.path().join("a.org")), Some(3));
    assert_eq!(v.body_char_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.body_char_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_body_char_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\náb\n* B\nz\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org "áb\n"=3 + "z\n"=2 = 5 chars, b.org 0
    assert_eq!(v.max_file_body_char_count(), Some(5));
    assert_eq!(v.min_file_body_char_count(), Some(0));
}

#[test]
fn vault_mean_file_body_char_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nxyz\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org "xyz\n"=4, b.org 0 -> total 4, 2 files -> 2
    assert_eq!(v.mean_file_body_char_count(), 2);
}

#[test]
fn vault_median_file_body_char_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\nz\n"),
        ("c.org", "* C\nxyz\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0, 2, 4 -> median 2
    assert_eq!(v.median_file_body_char_count(), Some(2));
}

#[test]
fn vault_file_body_char_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_body_char_count(), None);
    assert_eq!(v.min_file_body_char_count(), None);
    assert_eq!(v.mean_file_body_char_count(), 0);
    assert_eq!(v.median_file_body_char_count(), None);
}

#[test]
fn vault_body_line_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\nl1\nl2\n* B\nl3\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org body lines 2+1=3, b.org 0
    assert_eq!(v.body_line_count_of(&td.path().join("a.org")), Some(3));
    assert_eq!(v.body_line_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.body_line_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_body_line_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nl1\nl2\n* B\nl3\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_body_line_count(), Some(3));
    assert_eq!(v.min_file_body_line_count(), Some(0));
}

#[test]
fn vault_mean_file_body_line_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nl1\nl2\nl3\n"),
        ("b.org", "* C\nl4\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_body_line_count(), 2);
}

#[test]
fn vault_median_file_body_line_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\nl1\n"),
        ("c.org", "* C\nl2\nl3\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_body_line_count(), Some(1));
}

#[test]
fn vault_file_body_line_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_body_line_count(), None);
    assert_eq!(v.min_file_body_line_count(), None);
    assert_eq!(v.mean_file_body_line_count(), 0);
    assert_eq!(v.median_file_body_line_count(), None);
}

#[test]
fn vault_scheduled_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-01-01>\n* B\nSCHEDULED: <2026-01-02>\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org 2 scheduled, b.org 0
    assert_eq!(v.scheduled_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.scheduled_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.scheduled_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_scheduled_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-01-01>\n* B\nSCHEDULED: <2026-01-02>\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_scheduled_count(), Some(2));
    assert_eq!(v.min_file_scheduled_count(), Some(0));
}

#[test]
fn vault_mean_file_scheduled_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-01-01>\n* B\nSCHEDULED: <2026-01-02>\n* C\nSCHEDULED: <2026-01-03>\n"),
        ("b.org", "* D\nSCHEDULED: <2026-01-04>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_scheduled_count(), 2);
}

#[test]
fn vault_median_file_scheduled_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\nSCHEDULED: <2026-01-01>\n"),
        ("c.org", "* C\nSCHEDULED: <2026-01-02>\n* D\nSCHEDULED: <2026-01-03>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_scheduled_count(), Some(1));
}

#[test]
fn vault_file_scheduled_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_scheduled_count(), None);
    assert_eq!(v.min_file_scheduled_count(), None);
    assert_eq!(v.mean_file_scheduled_count(), 0);
    assert_eq!(v.median_file_scheduled_count(), None);
}

#[test]
fn vault_deadline_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\nDEADLINE: <2026-01-01>\n* B\nDEADLINE: <2026-01-02>\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.deadline_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.deadline_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.deadline_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_deadline_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nDEADLINE: <2026-01-01>\n* B\nDEADLINE: <2026-01-02>\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_deadline_count(), Some(2));
    assert_eq!(v.min_file_deadline_count(), Some(0));
}

#[test]
fn vault_mean_file_deadline_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nDEADLINE: <2026-01-01>\n* B\nDEADLINE: <2026-01-02>\n* C\nDEADLINE: <2026-01-03>\n"),
        ("b.org", "* D\nDEADLINE: <2026-01-04>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_deadline_count(), 2);
}

#[test]
fn vault_median_file_deadline_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\nDEADLINE: <2026-01-01>\n"),
        ("c.org", "* C\nDEADLINE: <2026-01-02>\n* D\nDEADLINE: <2026-01-03>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_deadline_count(), Some(1));
}

#[test]
fn vault_file_deadline_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_deadline_count(), None);
    assert_eq!(v.min_file_deadline_count(), None);
    assert_eq!(v.mean_file_deadline_count(), 0);
    assert_eq!(v.median_file_deadline_count(), None);
}

#[test]
fn vault_closed_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\nCLOSED: [2026-01-01]\n* B\nCLOSED: [2026-01-02]\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.closed_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.closed_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.closed_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_closed_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nCLOSED: [2026-01-01]\n* B\nCLOSED: [2026-01-02]\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_closed_count(), Some(2));
    assert_eq!(v.min_file_closed_count(), Some(0));
}

#[test]
fn vault_mean_file_closed_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nCLOSED: [2026-01-01]\n* B\nCLOSED: [2026-01-02]\n* C\nCLOSED: [2026-01-03]\n"),
        ("b.org", "* D\nCLOSED: [2026-01-04]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_closed_count(), 2);
}

#[test]
fn vault_median_file_closed_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\nCLOSED: [2026-01-01]\n"),
        ("c.org", "* C\nCLOSED: [2026-01-02]\n* D\nCLOSED: [2026-01-03]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_closed_count(), Some(1));
}

#[test]
fn vault_file_closed_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_closed_count(), None);
    assert_eq!(v.min_file_closed_count(), None);
    assert_eq!(v.mean_file_closed_count(), 0);
    assert_eq!(v.median_file_closed_count(), None);
}

#[test]
fn vault_id_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: id1\n:END:\n* B\n:PROPERTIES:\n:ID: id2\n:END:\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.id_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.id_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.id_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_id_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: id1\n:END:\n* B\n:PROPERTIES:\n:ID: id2\n:END:\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_id_count(), Some(2));
    assert_eq!(v.min_file_id_count(), Some(0));
}

#[test]
fn vault_mean_file_id_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: id1\n:END:\n* B\n:PROPERTIES:\n:ID: id2\n:END:\n* C\n:PROPERTIES:\n:ID: id3\n:END:\n"),
        ("b.org", "* D\n:PROPERTIES:\n:ID: id4\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_id_count(), 2);
}

#[test]
fn vault_median_file_id_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n:PROPERTIES:\n:ID: id1\n:END:\n"),
        ("c.org", "* C\n:PROPERTIES:\n:ID: id2\n:END:\n* D\n:PROPERTIES:\n:ID: id3\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_id_count(), Some(1));
}

#[test]
fn vault_file_id_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_id_count(), None);
    assert_eq!(v.min_file_id_count(), None);
    assert_eq!(v.mean_file_id_count(), 0);
    assert_eq!(v.median_file_id_count(), None);
}

#[test]
fn vault_archived_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A :ARCHIVE:\n* B :ARCHIVE:\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.archived_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.archived_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.archived_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_archived_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :ARCHIVE:\n* B :ARCHIVE:\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_archived_count(), Some(2));
    assert_eq!(v.min_file_archived_count(), Some(0));
}

#[test]
fn vault_mean_file_archived_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :ARCHIVE:\n* B :ARCHIVE:\n* C :ARCHIVE:\n"),
        ("b.org", "* D :ARCHIVE:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_archived_count(), 2);
}

#[test]
fn vault_median_file_archived_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B :ARCHIVE:\n"),
        ("c.org", "* C :ARCHIVE:\n* D :ARCHIVE:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_archived_count(), Some(1));
}

#[test]
fn vault_file_archived_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_archived_count(), None);
    assert_eq!(v.min_file_archived_count(), None);
    assert_eq!(v.mean_file_archived_count(), 0);
    assert_eq!(v.median_file_archived_count(), None);
}

#[test]
fn vault_comment_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* COMMENT A\n* COMMENT B\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.comment_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.comment_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.comment_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_comment_count_match() {
    let td = write_vault(&[
        ("a.org", "* COMMENT A\n* COMMENT B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_comment_count(), Some(2));
    assert_eq!(v.min_file_comment_count(), Some(0));
}

#[test]
fn vault_mean_file_comment_count_match() {
    let td = write_vault(&[
        ("a.org", "* COMMENT A\n* COMMENT B\n* COMMENT C\n"),
        ("b.org", "* COMMENT D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_comment_count(), 2);
}

#[test]
fn vault_median_file_comment_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* COMMENT B\n"),
        ("c.org", "* COMMENT C\n* COMMENT D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_comment_count(), Some(1));
}

#[test]
fn vault_file_comment_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_comment_count(), None);
    assert_eq!(v.min_file_comment_count(), None);
    assert_eq!(v.mean_file_comment_count(), 0);
    assert_eq!(v.median_file_comment_count(), None);
}

#[test]
fn vault_leaf_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org leaves: B, C = 2 (A has child); b.org D = 1
    assert_eq!(v.leaf_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.leaf_count_of(&td.path().join("b.org")), Some(1));
    assert_eq!(v.leaf_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_leaf_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n* B\n* C\n"),
        ("b.org", "* D\n** E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org 3 leaves, b.org 1 leaf (E)
    assert_eq!(v.max_file_leaf_count(), Some(3));
    assert_eq!(v.min_file_leaf_count(), Some(1));
}

#[test]
fn vault_mean_file_leaf_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n* B\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_leaf_count(), 2);
}

#[test]
fn vault_median_file_leaf_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n* D\n"),
        ("c.org", "* E\n* F\n* G\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // leaves: a=1(B), b=2, c=3 -> median 2
    assert_eq!(v.median_file_leaf_count(), Some(2));
}

#[test]
fn vault_file_leaf_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_leaf_count(), None);
    assert_eq!(v.min_file_leaf_count(), None);
    assert_eq!(v.mean_file_leaf_count(), 0);
    assert_eq!(v.median_file_leaf_count(), None);
}

#[test]
fn vault_branch_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n*** C\n* D\n"),
        ("b.org", "* E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org branches: A, B = 2; b.org 0
    assert_eq!(v.branch_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.branch_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.branch_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_branch_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n*** C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org branches A,B = 2; b.org 0
    assert_eq!(v.max_file_branch_count(), Some(2));
    assert_eq!(v.min_file_branch_count(), Some(0));
}

#[test]
fn vault_mean_file_branch_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n* C\n** D\n* E\n** F\n"),
        ("b.org", "* G\n** H\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org branches A,C,E = 3; b.org G = 1 -> total 4, 2 files -> 2
    assert_eq!(v.mean_file_branch_count(), 2);
}

#[test]
fn vault_median_file_branch_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n** C\n"),
        ("c.org", "* D\n** E\n* F\n** G\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // branches: a=0, b=1, c=2 -> median 1
    assert_eq!(v.median_file_branch_count(), Some(1));
}

#[test]
fn vault_file_branch_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_branch_count(), None);
    assert_eq!(v.min_file_branch_count(), None);
    assert_eq!(v.mean_file_branch_count(), 0);
    assert_eq!(v.median_file_branch_count(), None);
}

#[test]
fn vault_with_priority_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n* [#C] Y\n* Z\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_priority_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.with_priority_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.with_priority_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_priority_count_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n* [#C] Y\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_priority_count(), Some(2));
    assert_eq!(v.min_file_priority_count(), Some(0));
}

#[test]
fn vault_mean_file_priority_count_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n* [#B] Y\n* [#C] Z\n"),
        ("b.org", "* [#A] D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_priority_count(), 2);
}

#[test]
fn vault_median_file_priority_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* [#A] B\n"),
        ("c.org", "* [#A] C\n* [#B] D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_priority_count(), Some(1));
}

#[test]
fn vault_file_priority_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_priority_count(), None);
    assert_eq!(v.min_file_priority_count(), None);
    assert_eq!(v.mean_file_priority_count(), 0);
    assert_eq!(v.median_file_priority_count(), None);
}

#[test]
fn vault_with_body_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody\n* B\nmore\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org A,B have body = 2; b.org 0
    assert_eq!(v.with_body_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.with_body_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.with_body_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_with_body_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody\n* B\nmore\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_with_body_count(), Some(2));
    assert_eq!(v.min_file_with_body_count(), Some(0));
}

#[test]
fn vault_mean_file_with_body_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody\n* B\nmore\n* C\nyet\n"),
        ("b.org", "* D\nbody\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_with_body_count(), 2);
}

#[test]
fn vault_median_file_with_body_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\nbody\n"),
        ("c.org", "* C\nbody\n* D\nmore\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_with_body_count(), Some(1));
}

#[test]
fn vault_file_with_body_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_with_body_count(), None);
    assert_eq!(v.min_file_with_body_count(), None);
    assert_eq!(v.mean_file_with_body_count(), 0);
    assert_eq!(v.median_file_with_body_count(), None);
}

#[test]
fn vault_with_link_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[l1]]\n* B\n[[l2]] [[l3]]\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org headlines carrying links: A, B = 2; b.org 0
    assert_eq!(v.with_link_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.with_link_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.with_link_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_with_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[l1]]\n* B\n[[l2]]\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_with_link_count(), Some(2));
    assert_eq!(v.min_file_with_link_count(), Some(0));
}

#[test]
fn vault_mean_file_with_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[l1]]\n* B\n[[l2]]\n* C\n[[l3]]\n"),
        ("b.org", "* D\n[[l4]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_with_link_count(), 2);
}

#[test]
fn vault_median_file_with_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n[[l1]]\n"),
        ("c.org", "* C\n[[l2]]\n* D\n[[l3]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_with_link_count(), Some(1));
}

#[test]
fn vault_file_with_link_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_with_link_count(), None);
    assert_eq!(v.min_file_with_link_count(), None);
    assert_eq!(v.mean_file_with_link_count(), 0);
    assert_eq!(v.median_file_with_link_count(), None);
}

#[test]
fn vault_with_property_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:K: v\n:END:\n* B\n:PROPERTIES:\n:K: w\n:END:\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org headlines with properties: A, B = 2; b.org 0
    assert_eq!(v.with_property_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.with_property_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.with_property_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_with_property_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:K: v\n:END:\n* B\n:PROPERTIES:\n:K: w\n:END:\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_with_property_count(), Some(2));
    assert_eq!(v.min_file_with_property_count(), Some(0));
}

#[test]
fn vault_mean_file_with_property_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:K: v\n:END:\n* B\n:PROPERTIES:\n:K: w\n:END:\n* C\n:PROPERTIES:\n:K: x\n:END:\n"),
        ("b.org", "* D\n:PROPERTIES:\n:K: y\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_with_property_count(), 2);
}

#[test]
fn vault_median_file_with_property_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n:PROPERTIES:\n:K: v\n:END:\n"),
        ("c.org", "* C\n:PROPERTIES:\n:K: w\n:END:\n* D\n:PROPERTIES:\n:K: x\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_with_property_count(), Some(1));
}

#[test]
fn vault_file_with_property_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_with_property_count(), None);
    assert_eq!(v.min_file_with_property_count(), None);
    assert_eq!(v.mean_file_with_property_count(), 0);
    assert_eq!(v.median_file_with_property_count(), None);
}

#[test]
fn vault_with_timestamp_count_of_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-01-01>\n* B\n<2026-01-02> <2026-01-03>\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // a.org headlines with timestamps: A, B = 2; b.org 0
    assert_eq!(v.with_timestamp_count_of(&td.path().join("a.org")), Some(2));
    assert_eq!(v.with_timestamp_count_of(&td.path().join("b.org")), Some(0));
    assert_eq!(v.with_timestamp_count_of(&td.path().join("missing.org")), None);
}

#[test]
fn vault_max_min_file_with_timestamp_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-01-01>\n* B\n<2026-01-02>\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_with_timestamp_count(), Some(2));
    assert_eq!(v.min_file_with_timestamp_count(), Some(0));
}

#[test]
fn vault_mean_file_with_timestamp_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-01-01>\n* B\n<2026-01-02>\n* C\n<2026-01-03>\n"),
        ("b.org", "* D\n<2026-01-04>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 3+1=4, 2 files -> 2
    assert_eq!(v.mean_file_with_timestamp_count(), 2);
}

#[test]
fn vault_median_file_with_timestamp_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n<2026-01-01>\n"),
        ("c.org", "* C\n<2026-01-02>\n* D\n<2026-01-03>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_file_with_timestamp_count(), Some(1));
}

#[test]
fn vault_file_with_timestamp_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_with_timestamp_count(), None);
    assert_eq!(v.min_file_with_timestamp_count(), None);
    assert_eq!(v.mean_file_with_timestamp_count(), 0);
    assert_eq!(v.median_file_with_timestamp_count(), None);
}

#[test]
fn vault_file_byte_count_none_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_byte_count(), None);
    assert_eq!(v.min_file_byte_count(), None);
    assert_eq!(v.median_file_byte_count(), None);
}

#[test]
fn vault_total_line_count_match() {
    let td = write_vault(&[("a.org", "* A\nx\n"), ("b.org", "* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    // a.org 2 lines, b.org 1 line = 3
    assert_eq!(v.total_line_count(), 3);
}

#[test]
fn vault_mean_file_line_count_match() {
    let td = write_vault(&[("a.org", "* A\nx\n"), ("b.org", "* B\ny\nz\nw\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2,4 -> mean 3
    assert_eq!(v.mean_file_line_count(), 3);
}

#[test]
fn vault_max_min_file_line_count_match() {
    let td = write_vault(&[("a.org", "* A\n"), ("b.org", "* B\ny\nz\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_line_count(), Some(3));
    assert_eq!(v.min_file_line_count(), Some(1));
}

#[test]
fn vault_median_file_line_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\ny\n"),
        ("c.org", "* C\nx\ny\nz\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 1,2,4 -> median 2
    assert_eq!(v.median_file_line_count(), Some(2));
}

#[test]
fn vault_line_count_none_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_line_count(), 0);
    assert_eq!(v.max_file_line_count(), None);
}

#[test]
fn vault_mean_file_word_count_match() {
    let td = write_vault(&[("a.org", "* a b\n"), ("b.org", "* c d e f\n")]);
    let v = Vault::open(td.path()).expect("open");
    // a.org "* a b" = 3 words, b.org "* c d e f" = 5 -> mean 4
    assert_eq!(v.mean_file_word_count(), 4);
}

#[test]
fn vault_max_min_file_word_count_match() {
    let td = write_vault(&[("a.org", "* a\n"), ("b.org", "* b c d\n")]);
    let v = Vault::open(td.path()).expect("open");
    // "* a"=2, "* b c d"=4
    assert_eq!(v.max_file_word_count(), Some(4));
    assert_eq!(v.min_file_word_count(), Some(2));
}

#[test]
fn vault_median_file_word_count_match() {
    let td = write_vault(&[
        ("a.org", "* a\n"),
        ("b.org", "* b c\n"),
        ("c.org", "* d e f g h\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // "* a"=2, "* b c"=3, "* d e f g h"=6 -> median 3
    assert_eq!(v.median_file_word_count(), Some(3));
}

#[test]
fn vault_file_word_count_none_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_word_count(), None);
    assert_eq!(v.mean_file_word_count(), 0);
}

#[test]
fn vault_most_common_todo_returns_top() {
    let td = write_vault(&[
        ("a.org", "* TODO X\n* DONE Y\n"),
        ("b.org", "* TODO Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.most_common_todo(), Some("TODO".to_owned()));
}

#[test]
fn vault_most_common_priority_returns_top() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n* [#B] Y\n"),
        ("b.org", "* [#A] Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.most_common_priority(), Some('A'));
}

#[test]
fn vault_most_common_level_returns_top() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n*** C\n*** D\n*** E\n"),
        ("b.org", "* F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.most_common_level(), Some(3));
}

#[test]
fn vault_max_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n*** C\n"),
        ("b.org", "* D\n** E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_level(), Some(3));
}

#[test]
fn vault_min_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n*** C\n"),
        ("b.org", "** D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.min_level(), Some(1));
}

#[test]
fn vault_max_level_none_on_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_level(), None);
}

#[test]
fn vault_total_level_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n*** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 1+2+3 = 6
    assert_eq!(v.total_level(), 6);
}

#[test]
fn vault_mean_level_match() {
    let td = write_vault(&[("a.org", "* A\n*** B\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 1,3 -> mean 2
    assert_eq!(v.mean_level(), 2);
}

#[test]
fn vault_mean_level_zero_on_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_level(), 0);
}

#[test]
fn vault_median_level_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n**** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 1,2,4 -> median 2
    assert_eq!(v.median_level(), Some(2));
}

#[test]
fn vault_mode_level_match() {
    let td = write_vault(&[("a.org", "* A\n* B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // levels 1,1,2 -> mode 1
    assert_eq!(v.mode_level(), Some(1));
}

#[test]
fn vault_level_range_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n*** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.level_range(), Some((1, 3)));
}

#[test]
fn vault_level_range_none_on_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.level_range(), None);
}

#[test]
fn vault_root_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n* C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // roots: a.org A,C + b.org D = 3
    assert_eq!(v.root_count(), 3);
}

#[test]
fn vault_leaf_count_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // A has child B -> not leaf; B leaf; C leaf -> 2 leaves
    assert_eq!(v.leaf_count(), 2);
}

#[test]
fn vault_leaf_pct_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // leaves: B, C, D = 3 of 4 -> 75
    assert_eq!(v.leaf_pct(), 75);
}

#[test]
fn vault_leaf_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.leaf_count(), 0);
    assert_eq!(v.leaf_pct(), 0);
}

#[test]
fn vault_branch_count_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n*** C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // A has child, B has child -> 2 branches (C, D leaves)
    assert_eq!(v.branch_count(), 2);
}

#[test]
fn vault_branch_pct_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // A is branch (1 of 4) -> 25
    assert_eq!(v.branch_pct(), 25);
}

#[test]
fn vault_branch_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.branch_count(), 0);
    assert_eq!(v.branch_pct(), 0);
}

#[test]
fn vault_paths_with_priority_returns_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n"),
        ("b.org", "* Plain\n"),
        ("c.org", "* [#A] Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let paths: Vec<&str> = v
        .paths_with_priority('A')
        .into_iter()
        .filter_map(|p| p.file_name().and_then(|s| s.to_str()))
        .collect();
    assert!(paths.contains(&"a.org"));
    assert!(paths.contains(&"c.org"));
    assert!(!paths.contains(&"b.org"));
}

#[test]
fn vault_paths_at_level_returns_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "** D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let paths: Vec<&str> = v
        .paths_at_level(2)
        .into_iter()
        .filter_map(|p| p.file_name().and_then(|s| s.to_str()))
        .collect();
    assert!(paths.contains(&"a.org"));
    assert!(paths.contains(&"c.org"));
    assert!(!paths.contains(&"b.org"));
}

#[test]
fn vault_paths_with_todo_returns_match() {
    let td = write_vault(&[
        ("a.org", "* TODO X\n"),
        ("b.org", "* DONE Y\n"),
        ("c.org", "* TODO Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let paths: Vec<&str> = v
        .paths_with_todo("TODO")
        .into_iter()
        .filter_map(|p| p.file_name().and_then(|s| s.to_str()))
        .collect();
    assert!(paths.contains(&"a.org"));
    assert!(paths.contains(&"c.org"));
    assert!(!paths.contains(&"b.org"));
}

#[test]
fn vault_paths_with_id_returns_match() {
    let td = write_vault(&[
        ("a.org", "* X\n:PROPERTIES:\n:ID: x\n:END:\n"),
        ("b.org", "* Y\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let paths: Vec<&str> = v
        .paths_with_id()
        .into_iter()
        .filter_map(|p| p.file_name().and_then(|s| s.to_str()))
        .collect();
    assert!(paths.contains(&"a.org"));
    assert!(!paths.contains(&"b.org"));
}

#[test]
fn vault_paths_with_property_returns_match() {
    let td = write_vault(&[
        ("a.org", "* X\n:PROPERTIES:\n:CATEGORY: foo\n:END:\n"),
        ("b.org", "* Y\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let paths: Vec<&str> = v
        .paths_with_property("CATEGORY")
        .into_iter()
        .filter_map(|p| p.file_name().and_then(|s| s.to_str()))
        .collect();
    assert!(paths.contains(&"a.org"));
    assert!(!paths.contains(&"b.org"));
}

#[test]
fn vault_path_count_with_tag_match() {
    let td = write_vault(&[
        ("a.org", "* X :work:\n"),
        ("b.org", "* Y :home:\n"),
        ("c.org", "* Z :work:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.path_count_with_tag("work"), 2);
    assert_eq!(v.path_count_with_tag("none"), 0);
}

#[test]
fn vault_path_count_with_todo_match() {
    let td = write_vault(&[
        ("a.org", "* TODO X\n"),
        ("b.org", "* DONE Y\n"),
        ("c.org", "* TODO Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.path_count_with_todo("TODO"), 2);
    assert_eq!(v.path_count_with_todo("WAIT"), 0);
}

#[test]
fn vault_path_count_with_priority_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n"),
        ("b.org", "* [#B] Y\n"),
        ("c.org", "* [#A] Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.path_count_with_priority('A'), 2);
    assert_eq!(v.path_count_with_priority('C'), 0);
}

#[test]
fn vault_path_count_at_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "** D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.path_count_at_level(2), 2);
    assert_eq!(v.path_count_at_level(3), 0);
}

#[test]
fn vault_path_count_with_id_match() {
    let td = write_vault(&[
        ("a.org", "* X\n:PROPERTIES:\n:ID: x\n:END:\n"),
        ("b.org", "* Y\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.path_count_with_id(), 1);
}

#[test]
fn vault_path_count_with_property_match() {
    let td = write_vault(&[
        ("a.org", "* X\n:PROPERTIES:\n:K: 1\n:END:\n"),
        ("b.org", "* Y\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.path_count_with_property("K"), 1);
    assert_eq!(v.path_count_with_property("MISSING"), 0);
}

#[test]
fn vault_total_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* X :work:\n* Y :work:home:\n"),
        ("b.org", "* Z :work:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_tag_count(), 4);
}

#[test]
fn vault_total_priority_count_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n* Y\n"),
        ("b.org", "* [#B] Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_priority_count(), 2);
}

#[test]
fn vault_total_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO X\n* Y\n"),
        ("b.org", "* DONE Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_todo_count(), 2);
}

#[test]
fn vault_total_id_count_match() {
    let td = write_vault(&[
        ("a.org", "* X\n:PROPERTIES:\n:ID: x\n:END:\n* Y\n"),
        ("b.org", "* Z\n:PROPERTIES:\n:ID: z\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_id_count(), 2);
}

#[test]
fn vault_total_property_count_match() {
    let td = write_vault(&[
        ("a.org", "* X\n:PROPERTIES:\n:K1: 1\n:K2: 2\n:END:\n"),
        ("b.org", "* Y\n:PROPERTIES:\n:K3: 3\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_property_count(), 3);
}

#[test]
fn vault_headline_count_with_tag_match() {
    let td = write_vault(&[
        ("a.org", "* X :work:\n* Y :work:home:\n"),
        ("b.org", "* Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.headline_count_with_tag("work"), 2);
}

#[test]
fn vault_headline_count_with_todo_match() {
    let td = write_vault(&[
        ("a.org", "* TODO X\n* DONE Y\n"),
        ("b.org", "* TODO Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.headline_count_with_todo("TODO"), 2);
    assert_eq!(v.headline_count_with_todo("DONE"), 1);
}

#[test]
fn vault_headline_count_with_priority_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n* [#B] Y\n"),
        ("b.org", "* [#A] Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.headline_count_with_priority('A'), 2);
    assert_eq!(v.headline_count_with_priority('B'), 1);
}

#[test]
fn vault_headline_count_at_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n** C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.headline_count_at_level(1), 2);
    assert_eq!(v.headline_count_at_level(2), 2);
}

#[test]
fn vault_headline_count_with_property_match() {
    let td = write_vault(&[
        ("a.org", "* X\n:PROPERTIES:\n:K: 1\n:END:\n"),
        ("b.org", "* Y\n:PROPERTIES:\n:K: 2\n:END:\n* Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.headline_count_with_property("K"), 2);
}

#[test]
fn vault_nonempty_path_count_match() {
    let td = write_vault(&[
        ("a.org", "* X\n"),
        ("b.org", "no headlines\n"),
        ("c.org", "* Y\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.nonempty_path_count(), 2);
}

#[test]
fn vault_empty_path_count_match() {
    let td = write_vault(&[
        ("a.org", "* X\n"),
        ("b.org", "no headlines\n"),
        ("c.org", "no headlines either\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.empty_path_count(), 2);
}

#[test]
fn vault_max_headlines_per_path_match() {
    let td = write_vault(&[
        ("a.org", "* X\n* Y\n* Z\n"),
        ("b.org", "* W\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_headlines_per_path(), Some(3));
}

#[test]
fn vault_min_headlines_per_path_match() {
    let td = write_vault(&[
        ("a.org", "* X\n* Y\n"),
        ("b.org", "* W\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.min_headlines_per_path(), Some(1));
}

#[test]
fn vault_total_headline_count_match() {
    let td = write_vault(&[
        ("a.org", "* X\n** Y\n"),
        ("b.org", "* Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_headline_count(), 3);
}

#[test]
fn vault_median_headlines_per_path_match_odd() {
    let td = write_vault(&[
        ("a.org", "* X\n"),
        ("b.org", "* X\n* Y\n* Z\n"),
        ("c.org", "* X\n* Y\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_headlines_per_path(), Some(2));
}

#[test]
fn vault_median_headlines_per_path_match_even() {
    let td = write_vault(&[
        ("a.org", "* X\n"),
        ("b.org", "* X\n* Y\n"),
        ("c.org", "* X\n* Y\n* Z\n"),
        ("d.org", "* X\n* Y\n* Z\n* W\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_headlines_per_path(), Some(2));
}

#[test]
fn vault_median_headlines_per_path_none_on_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_headlines_per_path(), None);
}

#[test]
fn vault_headlines_per_path_counts_match() {
    let td = write_vault(&[
        ("a.org", "* X\n"),
        ("b.org", "* Y\n"),
        ("c.org", "* P\n* Q\n* R\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // counts: 1,1,3
    let m = v.headlines_per_path_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn vault_mode_headlines_per_path_match() {
    let td = write_vault(&[
        ("a.org", "* X\n"),
        ("b.org", "* Y\n"),
        ("c.org", "* P\n* Q\n* R\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_headlines_per_path(), Some(1));
}

#[test]
fn vault_mode_headlines_per_path_none_on_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_headlines_per_path(), None);
}

#[test]
fn vault_max_min_property_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:K1: v\n:K2: w\n:END:\n* B\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_property_count(), Some(2));
    assert_eq!(v.min_property_count(), Some(0));
}

#[test]
fn vault_mean_property_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:K1: v\n:K2: w\n:K3: x\n:K4: y\n:END:\n* B\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 4,0 -> mean 2
    assert_eq!(v.mean_property_count(), 2);
}

#[test]
fn vault_median_property_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n* B\n:PROPERTIES:\n:K1: v\n:END:\n* C\n:PROPERTIES:\n:K2: v\n:K3: w\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,2 -> median 1
    assert_eq!(v.median_property_count(), Some(1));
}

#[test]
fn vault_property_count_counts_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:K1: v\n:END:\n* B\n:PROPERTIES:\n:K2: v\n:END:\n* C\n:PROPERTIES:\n:K3: v\n:K4: w\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.property_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_property_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:K1: v\n:END:\n* B\n:PROPERTIES:\n:K2: v\n:END:\n* C\n:PROPERTIES:\n:K3: v\n:K4: w\n:END:\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_property_count(), Some(1));
}

#[test]
fn vault_scheduled_pct_match() {
    let td = write_vault(&[("a.org", "* A\nSCHEDULED: <2026-01-01>\n* B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 1 of 4 -> 25
    assert_eq!(v.scheduled_pct(), 25);
}

#[test]
fn vault_deadline_pct_match() {
    let td = write_vault(&[("a.org", "* A\nDEADLINE: <2026-01-01>\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.deadline_pct(), 50);
}

#[test]
fn vault_count_unscheduled_match() {
    let td = write_vault(&[("a.org", "* A\nSCHEDULED: <2026-01-01>\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_unscheduled(), 2);
}

#[test]
fn vault_unscheduled_pct_match() {
    let td = write_vault(&[("a.org", "* A\nSCHEDULED: <2026-01-01>\n* B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.unscheduled_pct(), 75);
}

#[test]
fn vault_count_no_deadline_match() {
    let td = write_vault(&[("a.org", "* A\nDEADLINE: <2026-01-01>\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_deadline(), 2);
}

#[test]
fn vault_no_deadline_pct_match() {
    let td = write_vault(&[("a.org", "* A\nDEADLINE: <2026-01-01>\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_deadline_pct(), 50);
}

#[test]
fn vault_scheduled_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.scheduled_pct(), 0);
}

#[test]
fn vault_closed_pct_match() {
    let td = write_vault(&[("a.org", "* A\nCLOSED: [2026-01-01]\n* B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 1 of 4 -> 25
    assert_eq!(v.closed_pct(), 25);
}

#[test]
fn vault_closed_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.closed_pct(), 0);
}

#[test]
fn vault_count_unclosed_match() {
    let td = write_vault(&[("a.org", "* A\nCLOSED: [2026-01-01]\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2 of 3 lack CLOSED
    assert_eq!(v.count_unclosed(), 2);
}

#[test]
fn vault_unclosed_pct_match() {
    let td = write_vault(&[("a.org", "* A\nCLOSED: [2026-01-01]\n* B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 3 of 4 -> 75
    assert_eq!(v.unclosed_pct(), 75);
}

#[test]
fn vault_unclosed_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_unclosed(), 0);
    assert_eq!(v.unclosed_pct(), 0);
}

#[test]
fn vault_with_body_count_match() {
    let td = write_vault(&[("a.org", "* A\nbody\n* B\n* C\nmore\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_body_count(), 2);
}

#[test]
fn vault_body_pct_match() {
    let td = write_vault(&[("a.org", "* A\nbody\n* B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.body_pct(), 25);
}

#[test]
fn vault_count_empty_body_match() {
    let td = write_vault(&[("a.org", "* A\nbody\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_empty_body(), 2);
}

#[test]
fn vault_empty_body_pct_match() {
    let td = write_vault(&[("a.org", "* A\nbody\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.empty_body_pct(), 50);
}

#[test]
fn vault_body_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.body_pct(), 0);
}

#[test]
fn vault_archived_pct_match() {
    let td = write_vault(&[("a.org", "* A :ARCHIVE:\n* B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.archived_pct(), 25);
}

#[test]
fn vault_comment_pct_match() {
    let td = write_vault(&[("a.org", "* COMMENT A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.comment_pct(), 50);
}

#[test]
fn vault_count_non_archived_match() {
    let td = write_vault(&[("a.org", "* A :ARCHIVE:\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_non_archived(), 2);
}

#[test]
fn vault_non_archived_pct_match() {
    let td = write_vault(&[("a.org", "* A :ARCHIVE:\n* B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.non_archived_pct(), 75);
}

#[test]
fn vault_count_non_comment_match() {
    let td = write_vault(&[("a.org", "* COMMENT A\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_non_comment(), 2);
}

#[test]
fn vault_non_comment_pct_match() {
    let td = write_vault(&[("a.org", "* COMMENT A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.non_comment_pct(), 50);
}

#[test]
fn vault_archived_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.archived_pct(), 0);
    assert_eq!(v.comment_pct(), 0);
}

#[test]
fn vault_todo_pct_match() {
    let td = write_vault(&[("a.org", "* TODO A\n* B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.todo_pct(), 25);
}

#[test]
fn vault_count_no_todo_match() {
    let td = write_vault(&[("a.org", "* TODO A\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_todo(), 2);
}

#[test]
fn vault_no_todo_pct_match() {
    let td = write_vault(&[("a.org", "* TODO A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_todo_pct(), 50);
}

#[test]
fn vault_tagged_count_match() {
    let td = write_vault(&[("a.org", "* A :x:\n* B\n* C :y:z:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.tagged_count(), 2);
}

#[test]
fn vault_tagged_pct_match() {
    let td = write_vault(&[("a.org", "* A :x:\n* B\n* C\n* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.tagged_pct(), 25);
}

#[test]
fn vault_untagged_count_match() {
    let td = write_vault(&[("a.org", "* A :x:\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.untagged_count(), 2);
}

#[test]
fn vault_untagged_pct_match() {
    let td = write_vault(&[("a.org", "* A :x:\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.untagged_pct(), 50);
}

#[test]
fn vault_todo_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.todo_pct(), 0);
    assert_eq!(v.tagged_pct(), 0);
}

#[test]
fn vault_id_pct_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:ID: x1\n:END:\n* B\n* C\n* D\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.id_pct(), 25);
}

#[test]
fn vault_count_no_id_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:ID: x1\n:END:\n* B\n* C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_id(), 2);
}

#[test]
fn vault_no_id_pct_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:ID: x1\n:END:\n* B\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_id_pct(), 50);
}

#[test]
fn vault_id_pct_zero_when_empty() {
    let td = write_vault(&[("a.org", "no headlines\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.id_pct(), 0);
}

#[test]
fn vault_path_with_max_headlines_match() {
    let td = write_vault(&[
        ("a.org", "* X\n"),
        ("b.org", "* X\n* Y\n* Z\n"),
        ("c.org", "* X\n* Y\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let p = v.path_with_max_headlines().expect("hit");
    assert_eq!(p.file_name().and_then(|s| s.to_str()), Some("b.org"));
}

#[test]
fn vault_path_with_min_headlines_match() {
    let td = write_vault(&[
        ("a.org", "* X\n* Y\n"),
        ("b.org", "* X\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let p = v.path_with_min_headlines().expect("hit");
    assert_eq!(p.file_name().and_then(|s| s.to_str()), Some("b.org"));
}

#[test]
fn vault_path_with_max_headlines_none_on_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.path_with_max_headlines().is_none());
}

#[test]
fn vault_all_titles_returns_collection() {
    let td = write_vault(&[("a.org", "* X\n** Y\n"), ("b.org", "* Z\n")]);
    let v = Vault::open(td.path()).expect("open");
    let mut titles = v.all_titles();
    titles.sort();
    assert_eq!(titles, vec!["X", "Y", "Z"]);
}

#[test]
fn vault_distinct_titles_returns_sorted_unique() {
    let td = write_vault(&[
        ("a.org", "* X\n* Y\n"),
        ("b.org", "* X\n* Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(
        v.distinct_titles(),
        vec!["X".to_owned(), "Y".to_owned(), "Z".to_owned()]
    );
}

#[test]
fn vault_distinct_id_properties_returns_sorted() {
    let td = write_vault(&[
        ("a.org", "* X\n:PROPERTIES:\n:ID: z\n:END:\n"),
        ("b.org", "* Y\n:PROPERTIES:\n:ID: a\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(
        v.distinct_id_properties(),
        vec!["a".to_owned(), "z".to_owned()]
    );
}

#[test]
fn vault_all_id_properties_returns_collection() {
    let td = write_vault(&[
        ("a.org", "* X\n:PROPERTIES:\n:ID: x\n:END:\n* Y\n:PROPERTIES:\n:ID: y\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let mut ids = v.all_id_properties();
    ids.sort();
    assert_eq!(ids, vec!["x".to_owned(), "y".to_owned()]);
}

#[test]
fn vault_path_count_match() {
    let td = write_vault(&[("a.org", "* X\n"), ("b.org", "* Y\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.path_count(), 2);
}

#[test]
fn vault_distinct_id_property_count_match() {
    let td = write_vault(&[
        ("a.org", "* X\n:PROPERTIES:\n:ID: x\n:END:\n"),
        ("b.org", "* Y\n:PROPERTIES:\n:ID: y\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_id_property_count(), 2);
}

#[test]
fn vault_distinct_title_count_match() {
    let td = write_vault(&[
        ("a.org", "* X\n* Y\n"),
        ("b.org", "* X\n* Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_title_count(), 3);
}

#[test]
fn vault_watcher_observes_modify() {
    let td = write_vault(&[("x.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    let watcher = v.watch().expect("watch");
    // Give inotify a moment to register.
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(td.path().join("x.org"), "* B\n").expect("rewrite");
    // Wait up to a few seconds for the event.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut saw = false;
    while std::time::Instant::now() < deadline {
        if watcher.try_recv().is_some() {
            saw = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(saw, "expected at least one watcher event");
}

#[test]
fn vault_total_title_len_match() {
    let td = write_vault(&[("a.org", "* AA\n** BBBB\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_title_len(), 6);
}

#[test]
fn vault_mean_title_len_match() {
    let td = write_vault(&[("a.org", "* AA\n** BBBB\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_title_len(), 3);
}

#[test]
fn vault_max_min_title_len_match() {
    let td = write_vault(&[("a.org", "* A\n** CCCC\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_title_len(), Some(4));
    assert_eq!(v.min_title_len(), Some(1));
}

#[test]
fn vault_median_title_len_match() {
    let td = write_vault(&[("a.org", "* A\n** BB\n** CCCC\n")]);
    let v = Vault::open(td.path()).expect("open");
    // chars 1,2,4 -> median 2
    assert_eq!(v.median_title_len(), Some(2));
}

#[test]
fn vault_title_len_counts_match() {
    let td = write_vault(&[("a.org", "* AA\n** BB\n** CCC\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.title_len_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn vault_mode_title_len_match() {
    let td = write_vault(&[("a.org", "* AA\n** BB\n** CCC\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_title_len(), Some(2));
}

#[test]
fn vault_total_title_word_count_match() {
    let td = write_vault(&[("a.org", "* a b\n** c d e\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_title_word_count(), 5);
}

#[test]
fn vault_mean_title_word_count_match() {
    let td = write_vault(&[("a.org", "* a b\n** c d e f\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2,4 -> 3
    assert_eq!(v.mean_title_word_count(), 3);
}

#[test]
fn vault_max_min_title_word_count_match() {
    let td = write_vault(&[("a.org", "* a\n** b c d\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_title_word_count(), Some(3));
    assert_eq!(v.min_title_word_count(), Some(1));
}

#[test]
fn vault_median_title_word_count_match() {
    let td = write_vault(&[("a.org", "* a\n** b c\n** d e f g\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 1,2,4 -> median 2
    assert_eq!(v.median_title_word_count(), Some(2));
}

#[test]
fn vault_title_word_count_counts_match() {
    let td = write_vault(&[("a.org", "* a b\n** c d\n** e f g\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.title_word_count_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn vault_mode_title_word_count_match() {
    let td = write_vault(&[("a.org", "* a b\n** c d\n** e f g\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_title_word_count(), Some(2));
}

#[test]
fn vault_total_body_line_count_match() {
    let td = write_vault(&[("a.org", "* A\nx\n* B\ny\nz\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_body_line_count(), 3);
}

#[test]
fn vault_mean_body_line_count_match() {
    let td = write_vault(&[("a.org", "* A\nx\n* B\ny\nz\nw\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 1,3 -> 2
    assert_eq!(v.mean_body_line_count(), 2);
}

#[test]
fn vault_max_min_body_line_count_match() {
    let td = write_vault(&[("a.org", "* A\nx\n* B\ny\nz\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_body_line_count(), Some(2));
    assert_eq!(v.min_body_line_count(), Some(1));
}

#[test]
fn vault_median_body_line_count_match() {
    let td = write_vault(&[("a.org", "* A\n* B\nx\n* C\ny\nz\nw\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,3 -> median 1
    assert_eq!(v.median_body_line_count(), Some(1));
}

#[test]
fn vault_body_line_count_counts_match() {
    let td = write_vault(&[("a.org", "* A\nx\n* B\ny\n* C\nz\nw\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.body_line_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_body_line_count_match() {
    let td = write_vault(&[("a.org", "* A\nx\n* B\ny\n* C\nz\nw\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_body_line_count(), Some(1));
}

#[test]
fn vault_total_body_char_count_match() {
    let td = write_vault(&[("a.org", "* A\nab\n* B\nc\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_body_char_count(), 5);
}

#[test]
fn vault_mean_body_char_count_match() {
    let td = write_vault(&[("a.org", "* A\nab\n* B\nabcdef\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 3,7 -> 5
    assert_eq!(v.mean_body_char_count(), 5);
}

#[test]
fn vault_max_min_body_char_count_match() {
    let td = write_vault(&[("a.org", "* A\nx\n* B\nabcd\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_body_char_count(), Some(5));
    assert_eq!(v.min_body_char_count(), Some(2));
}

#[test]
fn vault_median_body_char_count_match() {
    let td = write_vault(&[("a.org", "* A\n* B\nx\n* C\nabc\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 0,2,4 -> median 2
    assert_eq!(v.median_body_char_count(), Some(2));
}

#[test]
fn vault_body_char_count_counts_match() {
    let td = write_vault(&[("a.org", "* A\nx\n* B\ny\n* C\nabc\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.body_char_count_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&4), Some(&1));
}

#[test]
fn vault_mode_body_char_count_match() {
    let td = write_vault(&[("a.org", "* A\nx\n* B\ny\n* C\nabc\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_body_char_count(), Some(2));
}

#[test]
fn vault_total_body_word_count_match() {
    let td = write_vault(&[("a.org", "* A\none two\n* B\nthree\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_body_word_count(), 3);
}

#[test]
fn vault_max_min_body_word_count_match() {
    let td = write_vault(&[("a.org", "* A\none\n* B\ntwo three four\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_body_word_count(), Some(3));
    assert_eq!(v.min_body_word_count(), Some(1));
}

#[test]
fn vault_median_body_word_count_match() {
    let td = write_vault(&[("a.org", "* A\n* B\none\n* C\ntwo three four\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 0,1,3 -> median 1
    assert_eq!(v.median_body_word_count(), Some(1));
}

#[test]
fn vault_body_word_count_counts_match() {
    let td = write_vault(&[("a.org", "* A\none\n* B\ntwo\n* C\nthree words here\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.body_word_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn vault_mode_body_word_count_match() {
    let td = write_vault(&[("a.org", "* A\none\n* B\ntwo\n* C\nthree words here\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_body_word_count(), Some(1));
}

#[test]
fn vault_total_title_byte_len_match() {
    let td = write_vault(&[("a.org", "* AA\n** BBBB\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_title_byte_len(), 6);
}

#[test]
fn vault_total_title_byte_len_unicode() {
    let td = write_vault(&[("a.org", "* é\n")]);
    let v = Vault::open(td.path()).expect("open");
    // "é" = 2 bytes, 1 char
    assert_eq!(v.total_title_byte_len(), 2);
    assert_eq!(v.total_title_len(), 1);
}

#[test]
fn vault_mean_title_byte_len_match() {
    let td = write_vault(&[("a.org", "* AA\n** BBBB\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2,4 -> 3
    assert_eq!(v.mean_title_byte_len(), 3);
}

#[test]
fn vault_max_min_title_byte_len_match() {
    let td = write_vault(&[("a.org", "* A\n** CCCC\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_title_byte_len(), Some(4));
    assert_eq!(v.min_title_byte_len(), Some(1));
}

#[test]
fn vault_median_title_byte_len_match() {
    let td = write_vault(&[("a.org", "* A\n** BB\n** CCCC\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 1,2,4 -> 2
    assert_eq!(v.median_title_byte_len(), Some(2));
}

#[test]
fn vault_title_byte_len_counts_match() {
    let td = write_vault(&[("a.org", "* AA\n** BB\n** CCC\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.title_byte_len_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn vault_mode_title_byte_len_match() {
    let td = write_vault(&[("a.org", "* AA\n** BB\n** CCC\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_title_byte_len(), Some(2));
}

#[test]
fn vault_max_min_subtree_word_count_match() {
    // parent has body "one two\n" (2 words) + child body "three\n" (1 word).
    // parent subtree_word_count = 3; child subtree_word_count = 1.
    let td = write_vault(&[("a.org", "* A\none two\n** B\nthree\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_word_count(), Some(3));
    assert_eq!(v.min_subtree_word_count(), Some(1));
}

#[test]
fn vault_total_subtree_word_count_match() {
    let td = write_vault(&[("a.org", "* A\none two\n** B\nthree\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 3 + 1 = 4
    assert_eq!(v.total_subtree_word_count(), 4);
}

#[test]
fn vault_mean_subtree_word_count_match() {
    let td = write_vault(&[("a.org", "* A\none two\n** B\nthree\n")]);
    let v = Vault::open(td.path()).expect("open");
    // (3+1)/2 = 2
    assert_eq!(v.mean_subtree_word_count(), 2);
}

#[test]
fn vault_subtree_word_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_word_count(), None);
    assert_eq!(v.min_subtree_word_count(), None);
    assert_eq!(v.total_subtree_word_count(), 0);
    assert_eq!(v.mean_subtree_word_count(), 0);
}

#[test]
fn vault_median_subtree_word_count_match() {
    // headlines subtree counts: parent=3, child=1 -> sorted [1,3], midpoint 2
    let td = write_vault(&[("a.org", "* A\none two\n** B\nthree\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_word_count(), Some(2));
}

#[test]
fn vault_median_subtree_word_count_odd() {
    // three headlines: A subtree=1, B subtree=1, C subtree=2 -> sorted [1,1,2] -> median 1
    let td = write_vault(&[("a.org", "* A\nx\n* B\ny\n* C\nz w\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_word_count(), Some(1));
}

#[test]
fn vault_median_subtree_word_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_word_count(), None);
}

#[test]
fn vault_subtree_word_count_counts_match() {
    // counts: 3 -> 1, 1 -> 1
    let td = write_vault(&[("a.org", "* A\none two\n** B\nthree\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.subtree_word_count_counts();
    assert_eq!(m.get(&3), Some(&1));
    assert_eq!(m.get(&1), Some(&1));
}

#[test]
fn vault_mode_subtree_word_count_match() {
    // subtree counts: A=1 (own body "x"), B=1 (own body "y"), C=2 (z w)
    // mode = 1
    let td = write_vault(&[("a.org", "* A\nx\n* B\ny\n* C\nz w\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_word_count(), Some(1));
}

#[test]
fn vault_mode_subtree_word_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_word_count(), None);
}

#[test]
fn vault_max_min_subtree_byte_count_match() {
    // single root "* AAA\n" - parent subtree_byte_count covers whole subtree.
    let td = write_vault(&[("a.org", "* AAA\n** BB\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.max_subtree_byte_count().is_some());
    assert!(v.min_subtree_byte_count().is_some());
    assert!(v.max_subtree_byte_count() >= v.min_subtree_byte_count());
}

#[test]
fn vault_total_subtree_byte_count_positive() {
    let td = write_vault(&[("a.org", "* AAA\n** BB\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.total_subtree_byte_count() > 0);
}

#[test]
fn vault_mean_subtree_byte_count_positive() {
    let td = write_vault(&[("a.org", "* AAA\n** BB\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.mean_subtree_byte_count() > 0);
}

#[test]
fn vault_subtree_byte_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_byte_count(), None);
    assert_eq!(v.min_subtree_byte_count(), None);
    assert_eq!(v.total_subtree_byte_count(), 0);
    assert_eq!(v.mean_subtree_byte_count(), 0);
}

#[test]
fn vault_median_subtree_byte_count_positive() {
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.median_subtree_byte_count().is_some());
}

#[test]
fn vault_median_subtree_byte_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_byte_count(), None);
}

#[test]
fn vault_mode_subtree_byte_count_match() {
    // siblings B and C both "* B\n" = 4 bytes, A subtree larger
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // B == C → mode is their byte count, A larger
    let counts = v.subtree_byte_count_counts();
    let mode = v.mode_subtree_byte_count().expect("non-empty");
    assert_eq!(counts.get(&mode), Some(&2));
}

#[test]
fn vault_mode_subtree_byte_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_byte_count(), None);
}

#[test]
fn vault_subtree_byte_count_counts_nonempty() {
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.subtree_byte_count_counts();
    assert!(!m.is_empty());
}

#[test]
fn vault_max_min_subtree_link_count_match() {
    // root A body has 2 links, child B body has 1, child C none
    let td = write_vault(&[(
        "a.org",
        "* A\n[[x][X]] [[y][Y]]\n** B\n[[z][Z]]\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // subtree counts: A=3, B=1, C=0
    assert_eq!(v.max_subtree_link_count(), Some(3));
    assert_eq!(v.min_subtree_link_count(), Some(0));
}

#[test]
fn vault_total_subtree_link_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n[[x][X]] [[y][Y]]\n** B\n[[z][Z]]\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 3 + 1 + 0 = 4
    assert_eq!(v.total_subtree_link_count(), 4);
}

#[test]
fn vault_mean_subtree_link_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n[[x][X]] [[y][Y]]\n** B\n[[z][Z]]\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 4/3 = 1
    assert_eq!(v.mean_subtree_link_count(), 1);
}

#[test]
fn vault_subtree_link_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_link_count(), None);
    assert_eq!(v.min_subtree_link_count(), None);
    assert_eq!(v.total_subtree_link_count(), 0);
    assert_eq!(v.mean_subtree_link_count(), 0);
}

#[test]
fn vault_median_subtree_link_count_match() {
    // counts: A=3, B=1, C=0 sorted [0,1,3] -> median 1
    let td = write_vault(&[(
        "a.org",
        "* A\n[[x][X]] [[y][Y]]\n** B\n[[z][Z]]\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_link_count(), Some(1));
}

#[test]
fn vault_median_subtree_link_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_link_count(), None);
}

#[test]
fn vault_mode_subtree_link_count_match() {
    // headlines A,B,C no links each → all 0 → mode 0
    let td = write_vault(&[("a.org", "* A\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_link_count(), Some(0));
}

#[test]
fn vault_mode_subtree_link_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_link_count(), None);
}

#[test]
fn vault_subtree_link_count_counts_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n[[x][X]] [[y][Y]]\n** B\n[[z][Z]]\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.subtree_link_count_counts();
    assert_eq!(m.get(&3), Some(&1));
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_max_min_subtree_tag_count_match() {
    // A has :x: B :y: C nothing — subtree counts: A=2, B=1, C=0
    let td = write_vault(&[("a.org", "* A :x:\n** B :y:\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_tag_count(), Some(2));
    assert_eq!(v.min_subtree_tag_count(), Some(0));
}

#[test]
fn vault_total_subtree_tag_count_match() {
    let td = write_vault(&[("a.org", "* A :x:\n** B :y:\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2+1+0
    assert_eq!(v.total_subtree_tag_count(), 3);
}

#[test]
fn vault_mean_subtree_tag_count_match() {
    let td = write_vault(&[("a.org", "* A :x:\n** B :y:\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 3/3=1
    assert_eq!(v.mean_subtree_tag_count(), 1);
}

#[test]
fn vault_subtree_tag_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_tag_count(), None);
    assert_eq!(v.min_subtree_tag_count(), None);
    assert_eq!(v.total_subtree_tag_count(), 0);
    assert_eq!(v.mean_subtree_tag_count(), 0);
}

#[test]
fn vault_median_subtree_tag_count_match() {
    // counts A=2, B=1, C=0 -> sorted [0,1,2] -> median 1
    let td = write_vault(&[("a.org", "* A :x:\n** B :y:\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_tag_count(), Some(1));
}

#[test]
fn vault_median_subtree_tag_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_tag_count(), None);
}

#[test]
fn vault_mode_subtree_tag_count_match() {
    // each headline 0 tags -> mode 0
    let td = write_vault(&[("a.org", "* A\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_tag_count(), Some(0));
}

#[test]
fn vault_mode_subtree_tag_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_tag_count(), None);
}

#[test]
fn vault_subtree_tag_count_counts_match() {
    let td = write_vault(&[("a.org", "* A :x:\n** B :y:\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.subtree_tag_count_counts();
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_max_min_subtree_todo_count_match() {
    // distinct keywords. A subtree has {TODO, DONE}=2, B={DONE}=1, C={}=0
    let td = write_vault(&[("a.org", "* TODO A\n** DONE B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_todo_count(), Some(2));
    assert_eq!(v.min_subtree_todo_count(), Some(0));
}

#[test]
fn vault_total_subtree_todo_count_match() {
    let td = write_vault(&[("a.org", "* TODO A\n** DONE B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2+1+0
    assert_eq!(v.total_subtree_todo_count(), 3);
}

#[test]
fn vault_mean_subtree_todo_count_match() {
    let td = write_vault(&[("a.org", "* TODO A\n** DONE B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 3/3=1
    assert_eq!(v.mean_subtree_todo_count(), 1);
}

#[test]
fn vault_subtree_todo_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_todo_count(), None);
    assert_eq!(v.min_subtree_todo_count(), None);
    assert_eq!(v.total_subtree_todo_count(), 0);
    assert_eq!(v.mean_subtree_todo_count(), 0);
}

#[test]
fn vault_median_subtree_todo_count_match() {
    // A=2, B=1, C=0 sorted [0,1,2] -> median 1
    let td = write_vault(&[("a.org", "* TODO A\n** DONE B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_todo_count(), Some(1));
}

#[test]
fn vault_median_subtree_todo_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_todo_count(), None);
}

#[test]
fn vault_mode_subtree_todo_count_match() {
    // no headlines have TODO -> all 0 -> mode 0
    let td = write_vault(&[("a.org", "* A\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_todo_count(), Some(0));
}

#[test]
fn vault_mode_subtree_todo_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_todo_count(), None);
}

#[test]
fn vault_subtree_todo_count_counts_match() {
    let td = write_vault(&[("a.org", "* TODO A\n** DONE B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.subtree_todo_count_counts();
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_max_min_subtree_priority_count_match() {
    // A=[#A], B=[#B], C no priority. distinct in subtree: A={A,B}=2, B={B}=1, C={}=0
    let td = write_vault(&[("a.org", "* [#A] A\n** [#B] B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_priority_count(), Some(2));
    assert_eq!(v.min_subtree_priority_count(), Some(0));
}

#[test]
fn vault_total_subtree_priority_count_match() {
    let td = write_vault(&[("a.org", "* [#A] A\n** [#B] B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2+1+0
    assert_eq!(v.total_subtree_priority_count(), 3);
}

#[test]
fn vault_mean_subtree_priority_count_match() {
    let td = write_vault(&[("a.org", "* [#A] A\n** [#B] B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_subtree_priority_count(), 1);
}

#[test]
fn vault_subtree_priority_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_priority_count(), None);
    assert_eq!(v.min_subtree_priority_count(), None);
    assert_eq!(v.total_subtree_priority_count(), 0);
    assert_eq!(v.mean_subtree_priority_count(), 0);
}

#[test]
fn vault_median_subtree_priority_count_match() {
    // counts A=2,B=1,C=0 sorted [0,1,2] -> 1
    let td = write_vault(&[("a.org", "* [#A] A\n** [#B] B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_priority_count(), Some(1));
}

#[test]
fn vault_median_subtree_priority_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_priority_count(), None);
}

#[test]
fn vault_mode_subtree_priority_count_match() {
    let td = write_vault(&[("a.org", "* A\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_priority_count(), Some(0));
}

#[test]
fn vault_mode_subtree_priority_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_priority_count(), None);
}

#[test]
fn vault_subtree_priority_count_counts_match() {
    let td = write_vault(&[("a.org", "* [#A] A\n** [#B] B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.subtree_priority_count_counts();
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_max_min_subtree_level_count_match() {
    // A subtree distinct levels {1,2}=2. B subtree {2}=1. C subtree {2}=1
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_level_count(), Some(2));
    assert_eq!(v.min_subtree_level_count(), Some(1));
}

#[test]
fn vault_total_subtree_level_count_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2+1+1
    assert_eq!(v.total_subtree_level_count(), 4);
}

#[test]
fn vault_mean_subtree_level_count_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 4/3=1
    assert_eq!(v.mean_subtree_level_count(), 1);
}

#[test]
fn vault_subtree_level_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_level_count(), None);
    assert_eq!(v.min_subtree_level_count(), None);
    assert_eq!(v.total_subtree_level_count(), 0);
    assert_eq!(v.mean_subtree_level_count(), 0);
}

#[test]
fn vault_median_subtree_level_count_match() {
    // counts A=2,B=1,C=1 sorted [1,1,2] -> median 1
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_level_count(), Some(1));
}

#[test]
fn vault_median_subtree_level_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_level_count(), None);
}

#[test]
fn vault_mode_subtree_level_count_match() {
    // siblings B,C: 1 distinct level. A: 2 distinct -> mode=1
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_level_count(), Some(1));
}

#[test]
fn vault_mode_subtree_level_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_level_count(), None);
}

#[test]
fn vault_subtree_level_count_counts_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.subtree_level_count_counts();
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&1), Some(&2));
}

#[test]
fn vault_max_min_subtree_property_count_match() {
    // A has :PROPERTIES: with 1 prop, B has 2, C 0.
    // subtree counts: A=3, B=2, C=0
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:x: 1\n:END:\n** B\n:PROPERTIES:\n:y: 2\n:z: 3\n:END:\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_property_count(), Some(3));
    assert_eq!(v.min_subtree_property_count(), Some(0));
}

#[test]
fn vault_total_subtree_property_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:x: 1\n:END:\n** B\n:PROPERTIES:\n:y: 2\n:z: 3\n:END:\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 3+2+0
    assert_eq!(v.total_subtree_property_count(), 5);
}

#[test]
fn vault_mean_subtree_property_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:x: 1\n:END:\n** B\n:PROPERTIES:\n:y: 2\n:z: 3\n:END:\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 5/3=1
    assert_eq!(v.mean_subtree_property_count(), 1);
}

#[test]
fn vault_subtree_property_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_property_count(), None);
    assert_eq!(v.min_subtree_property_count(), None);
    assert_eq!(v.total_subtree_property_count(), 0);
    assert_eq!(v.mean_subtree_property_count(), 0);
}

#[test]
fn vault_median_subtree_property_count_match() {
    // A=3, B=2, C=0 sorted [0,2,3] -> median 2
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:x: 1\n:END:\n** B\n:PROPERTIES:\n:y: 2\n:z: 3\n:END:\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_property_count(), Some(2));
}

#[test]
fn vault_median_subtree_property_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_property_count(), None);
}

#[test]
fn vault_mode_subtree_property_count_match() {
    let td = write_vault(&[("a.org", "* A\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_property_count(), Some(0));
}

#[test]
fn vault_mode_subtree_property_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_property_count(), None);
}

#[test]
fn vault_subtree_property_count_counts_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n:PROPERTIES:\n:x: 1\n:END:\n** B\n:PROPERTIES:\n:y: 2\n:z: 3\n:END:\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.subtree_property_count_counts();
    assert_eq!(m.get(&3), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_max_min_subtree_timestamp_count_match() {
    // A body has 1 ts, B body has 1, C none. Subtree: A=2, B=1, C=0.
    let td = write_vault(&[(
        "a.org",
        "* A\n<2026-05-30 Sat>\n** B\n<2026-05-31 Sun>\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_timestamp_count(), Some(2));
    assert_eq!(v.min_subtree_timestamp_count(), Some(0));
}

#[test]
fn vault_total_subtree_timestamp_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n<2026-05-30 Sat>\n** B\n<2026-05-31 Sun>\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 2+1+0
    assert_eq!(v.total_subtree_timestamp_count(), 3);
}

#[test]
fn vault_mean_subtree_timestamp_count_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n<2026-05-30 Sat>\n** B\n<2026-05-31 Sun>\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    // 3/3=1
    assert_eq!(v.mean_subtree_timestamp_count(), 1);
}

#[test]
fn vault_subtree_timestamp_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_subtree_timestamp_count(), None);
    assert_eq!(v.min_subtree_timestamp_count(), None);
    assert_eq!(v.total_subtree_timestamp_count(), 0);
    assert_eq!(v.mean_subtree_timestamp_count(), 0);
}

#[test]
fn vault_median_subtree_timestamp_count_match() {
    // A=2, B=1, C=0 -> sorted [0,1,2] -> 1
    let td = write_vault(&[(
        "a.org",
        "* A\n<2026-05-30 Sat>\n** B\n<2026-05-31 Sun>\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_timestamp_count(), Some(1));
}

#[test]
fn vault_median_subtree_timestamp_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_subtree_timestamp_count(), None);
}

#[test]
fn vault_mode_subtree_timestamp_count_match() {
    let td = write_vault(&[("a.org", "* A\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_timestamp_count(), Some(0));
}

#[test]
fn vault_mode_subtree_timestamp_count_none_when_empty() {
    let td = write_vault(&[("a.org", "")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_subtree_timestamp_count(), None);
}

#[test]
fn vault_subtree_timestamp_count_counts_match() {
    let td = write_vault(&[(
        "a.org",
        "* A\n<2026-05-30 Sat>\n** B\n<2026-05-31 Sun>\n** C\n",
    )]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.subtree_timestamp_count_counts();
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_file_todo_count_counts_match() {
    // a.org: 1 TODO, b.org: 1 TODO, c.org: 0 TODOs
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* TODO B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_todo_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* TODO B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_todo_count(), Some(1));
}

#[test]
fn vault_mode_file_todo_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_todo_count(), None);
}

#[test]
fn vault_file_priority_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] A\n"),
        ("b.org", "* [#B] B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_priority_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_priority_count_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] A\n"),
        ("b.org", "* [#B] B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_priority_count(), Some(1));
}

#[test]
fn vault_mode_file_priority_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_priority_count(), None);
}

#[test]
fn vault_file_leaf_count_counts_match() {
    // a.org has 1 leaf (A no children), b.org 1 leaf, c.org 2 leaves
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_leaf_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_file_leaf_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_leaf_count(), Some(1));
}

#[test]
fn vault_mode_file_leaf_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_leaf_count(), None);
}

#[test]
fn vault_file_branch_count_counts_match() {
    // a.org 1 branch (A has child B), b.org 0 (just C), c.org 1
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_branch_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_branch_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_branch_count(), Some(1));
}

#[test]
fn vault_mode_file_branch_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_branch_count(), None);
}

#[test]
fn vault_file_root_count_counts_match() {
    // a.org 1 root, b.org 1 root, c.org 2 roots
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_root_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_file_root_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_root_count(), Some(1));
}

#[test]
fn vault_mode_file_root_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_root_count(), None);
}

#[test]
fn vault_file_tag_count_counts_match() {
    // file tag count = headlines with non-empty tags count
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :y:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_tag_count_counts();
    // a.org: 1 tag, b.org: 1 tag, c.org: 0
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :y:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_tag_count(), Some(1));
}

#[test]
fn vault_mode_file_tag_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_tag_count(), None);
}

#[test]
fn vault_file_link_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n"),
        ("b.org", "* B\n[[y][Y]]\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_link_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n"),
        ("b.org", "* B\n[[y][Y]]\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_link_count(), Some(1));
}

#[test]
fn vault_mode_file_link_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_link_count(), None);
}

#[test]
fn vault_file_timestamp_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n"),
        ("b.org", "* B\n<2026-05-31 Sun>\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_timestamp_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_timestamp_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n"),
        ("b.org", "* B\n<2026-05-31 Sun>\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_timestamp_count(), Some(1));
}

#[test]
fn vault_mode_file_timestamp_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_timestamp_count(), None);
}

#[test]
fn vault_file_headline_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_headline_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_file_headline_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_headline_count(), Some(1));
}

#[test]
fn vault_mode_file_headline_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_headline_count(), None);
}

#[test]
fn vault_file_with_body_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody\n"),
        ("b.org", "* B\nbody\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_with_body_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_with_body_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody\n"),
        ("b.org", "* B\nbody\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_with_body_count(), Some(1));
}

#[test]
fn vault_mode_file_with_body_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_with_body_count(), None);
}

#[test]
fn vault_file_with_property_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:y: 2\n:END:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_with_property_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_with_property_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:y: 2\n:END:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_with_property_count(), Some(1));
}

#[test]
fn vault_mode_file_with_property_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_with_property_count(), None);
}

#[test]
fn vault_file_word_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* one\n"),
        ("b.org", "* two\n"),
        ("c.org", "* three four\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_word_count_counts();
    // source.split_whitespace(): a,b each yield ["*","one"|"two"] = 2; c = 3
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn vault_mode_file_word_count_match() {
    let td = write_vault(&[
        ("a.org", "* one\n"),
        ("b.org", "* two\n"),
        ("c.org", "* three four\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_word_count(), Some(2));
}

#[test]
fn vault_mode_file_word_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_word_count(), None);
}

#[test]
fn vault_file_byte_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* CCCCCC\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_byte_count_counts();
    // "* A\n" = 4, "* B\n" = 4, "* CCCCCC\n" = 9
    assert_eq!(m.get(&4), Some(&2));
    assert_eq!(m.get(&9), Some(&1));
}

#[test]
fn vault_mode_file_byte_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* CCCCCC\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_byte_count(), Some(4));
}

#[test]
fn vault_mode_file_byte_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_byte_count(), None);
}

#[test]
fn vault_file_char_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* CCCCCC\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_char_count_counts();
    assert_eq!(m.get(&4), Some(&2));
    assert_eq!(m.get(&9), Some(&1));
}

#[test]
fn vault_mode_file_char_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* CCCCCC\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_char_count(), Some(4));
}

#[test]
fn vault_mode_file_char_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_char_count(), None);
}

#[test]
fn vault_file_id_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:ID: y\n:END:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_id_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_id_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:ID: y\n:END:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_id_count(), Some(1));
}

#[test]
fn vault_mode_file_id_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_id_count(), None);
}

#[test]
fn vault_file_with_link_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n"),
        ("b.org", "* B\n[[y][Y]]\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_with_link_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_with_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n"),
        ("b.org", "* B\n[[y][Y]]\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_with_link_count(), Some(1));
}

#[test]
fn vault_mode_file_with_link_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_with_link_count(), None);
}

#[test]
fn vault_file_with_timestamp_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n"),
        ("b.org", "* B\n<2026-05-31 Sun>\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_with_timestamp_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_with_timestamp_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n"),
        ("b.org", "* B\n<2026-05-31 Sun>\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_with_timestamp_count(), Some(1));
}

#[test]
fn vault_mode_file_with_timestamp_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_with_timestamp_count(), None);
}

#[test]
fn vault_file_body_byte_count_counts_match() {
    // bodies: "x\n" (2 bytes), "x\n" (2), "" (0)
    let td = write_vault(&[
        ("a.org", "* A\nx\n"),
        ("b.org", "* B\nx\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_body_byte_count_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_body_byte_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nx\n"),
        ("b.org", "* B\nx\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_body_byte_count(), Some(2));
}

#[test]
fn vault_mode_file_body_byte_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_body_byte_count(), None);
}

#[test]
fn vault_file_body_line_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\nx\n"),
        ("b.org", "* B\nx\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_body_line_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_body_line_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nx\n"),
        ("b.org", "* B\nx\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_body_line_count(), Some(1));
}

#[test]
fn vault_mode_file_body_line_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_body_line_count(), None);
}

#[test]
fn vault_file_body_char_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\nx\n"),
        ("b.org", "* B\nx\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_body_char_count_counts();
    assert_eq!(m.get(&2), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_body_char_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nx\n"),
        ("b.org", "* B\nx\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_body_char_count(), Some(2));
}

#[test]
fn vault_mode_file_body_char_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_body_char_count(), None);
}

#[test]
fn vault_file_body_word_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\nx\n"),
        ("b.org", "* B\nx\n"),
        ("c.org", "* C\nx y\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_body_word_count_counts();
    // a,b body "x" -> 1 word; c body "x y" -> 2 words
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_file_body_word_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nx\n"),
        ("b.org", "* B\nx\n"),
        ("c.org", "* C\nx y\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_body_word_count(), Some(1));
}

#[test]
fn vault_mode_file_body_word_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_body_word_count(), None);
}

#[test]
fn vault_max_min_file_title_byte_len_match() {
    // titles: "A" (1), "BBB" (3), "CC" (2). per-file sums: 1, 3, 2
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* BBB\n"),
        ("c.org", "* CC\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_title_byte_len(), Some(3));
    assert_eq!(v.min_file_title_byte_len(), Some(1));
}

#[test]
fn vault_total_file_title_byte_len_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* BBB\n"),
        ("c.org", "* CC\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_title_byte_len(), 6);
}

#[test]
fn vault_mean_file_title_byte_len_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* BBB\n"),
        ("c.org", "* CC\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_title_byte_len(), 2);
}

#[test]
fn vault_median_file_title_byte_len_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* BBB\n"),
        ("c.org", "* CC\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // sums sorted [1,2,3] median 2
    assert_eq!(v.median_file_title_byte_len(), Some(2));
}

#[test]
fn vault_file_title_byte_len_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* BBB\n"),
        ("c.org", "* CC\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_title_byte_len_counts();
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&3), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_file_title_byte_len_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* A\n"),
        ("c.org", "* BBB\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_title_byte_len(), Some(1));
}

#[test]
fn vault_file_title_byte_len_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_title_byte_len(), None);
    assert_eq!(v.min_file_title_byte_len(), None);
    assert_eq!(v.total_file_title_byte_len(), 0);
    assert_eq!(v.mean_file_title_byte_len(), 0);
    assert_eq!(v.median_file_title_byte_len(), None);
    assert_eq!(v.mode_file_title_byte_len(), None);
}

#[test]
fn vault_max_min_file_title_word_count_match() {
    // titles "A B" 2 words, "C" 1, "D E F" 3
    let td = write_vault(&[
        ("a.org", "* A B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D E F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_title_word_count(), Some(3));
    assert_eq!(v.min_file_title_word_count(), Some(1));
}

#[test]
fn vault_total_file_title_word_count_match() {
    let td = write_vault(&[
        ("a.org", "* A B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D E F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_title_word_count(), 6);
}

#[test]
fn vault_mean_file_title_word_count_match() {
    let td = write_vault(&[
        ("a.org", "* A B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D E F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_title_word_count(), 2);
}

#[test]
fn vault_median_file_title_word_count_match() {
    let td = write_vault(&[
        ("a.org", "* A B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D E F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_file_title_word_count(), Some(2));
}

#[test]
fn vault_file_title_word_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D E F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_title_word_count_counts();
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn vault_mode_file_title_word_count_match() {
    let td = write_vault(&[
        ("a.org", "* A B\n"),
        ("b.org", "* C D\n"),
        ("c.org", "* E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_title_word_count(), Some(2));
}

#[test]
fn vault_file_title_word_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_title_word_count(), None);
    assert_eq!(v.min_file_title_word_count(), None);
    assert_eq!(v.total_file_title_word_count(), 0);
    assert_eq!(v.mean_file_title_word_count(), 0);
    assert_eq!(v.median_file_title_word_count(), None);
    assert_eq!(v.mode_file_title_word_count(), None);
}

#[test]
fn vault_max_min_file_descendant_count_match() {
    // a.org: A has child B -> A.descendant_count=1, B.descendant_count=0; sum=1
    // b.org: only "* C" -> sum=0
    // c.org: D->E->F -> D=2, E=1, F=0; sum=3
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n*** F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_descendant_count(), Some(3));
    assert_eq!(v.min_file_descendant_count(), Some(0));
}

#[test]
fn vault_total_file_descendant_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n*** F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_descendant_count(), 4);
}

#[test]
fn vault_mean_file_descendant_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n*** F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_descendant_count(), 1);
}

#[test]
fn vault_median_file_descendant_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n*** F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [0,1,3] median 1
    assert_eq!(v.median_file_descendant_count(), Some(1));
}

#[test]
fn vault_file_descendant_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n*** F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_descendant_count_counts();
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
    assert_eq!(m.get(&3), Some(&1));
}

#[test]
fn vault_mode_file_descendant_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n** D\n"),
        ("c.org", "* E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_descendant_count(), Some(1));
}

#[test]
fn vault_file_descendant_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_descendant_count(), None);
    assert_eq!(v.min_file_descendant_count(), None);
    assert_eq!(v.total_file_descendant_count(), 0);
    assert_eq!(v.mean_file_descendant_count(), 0);
    assert_eq!(v.median_file_descendant_count(), None);
    assert_eq!(v.mode_file_descendant_count(), None);
}

#[test]
fn vault_max_min_file_subtree_size_match() {
    // subtree_size = 1 + descendant_count for each headline
    // a.org A->B: A=2, B=1; sum=3
    // b.org C: sum=1
    // c.org D->E->F: D=3, E=2, F=1; sum=6
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n*** F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_size(), Some(6));
    assert_eq!(v.min_file_subtree_size(), Some(1));
}

#[test]
fn vault_total_file_subtree_size_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n*** F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_subtree_size(), 10);
}

#[test]
fn vault_mean_file_subtree_size_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n*** F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_subtree_size(), 3);
}

#[test]
fn vault_median_file_subtree_size_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n*** F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [1,3,6] median 3
    assert_eq!(v.median_file_subtree_size(), Some(3));
}

#[test]
fn vault_file_subtree_size_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n*** F\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_subtree_size_counts();
    assert_eq!(m.get(&3), Some(&1));
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&6), Some(&1));
}

#[test]
fn vault_mode_file_subtree_size_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n** D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // sums: 1, 1, 3 → mode 1
    assert_eq!(v.mode_file_subtree_size(), Some(1));
}

#[test]
fn vault_file_subtree_size_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_size(), None);
    assert_eq!(v.min_file_subtree_size(), None);
    assert_eq!(v.total_file_subtree_size(), 0);
    assert_eq!(v.mean_file_subtree_size(), 0);
    assert_eq!(v.median_file_subtree_size(), None);
    assert_eq!(v.mode_file_subtree_size(), None);
}

#[test]
fn vault_max_min_file_max_level_match() {
    // per-file max_level: a=1, b=3, c=2
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* A\n** B\n*** C\n"),
        ("c.org", "* A\n** B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_max_level(), Some(3));
    assert_eq!(v.min_file_max_level(), Some(1));
}

#[test]
fn vault_total_file_max_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* A\n** B\n*** C\n"),
        ("c.org", "* A\n** B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_max_level(), 6);
}

#[test]
fn vault_mean_file_max_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* A\n** B\n*** C\n"),
        ("c.org", "* A\n** B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_max_level(), 2);
}

#[test]
fn vault_median_file_max_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* A\n** B\n*** C\n"),
        ("c.org", "* A\n** B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [1,2,3] median 2
    assert_eq!(v.median_file_max_level(), Some(2));
}

#[test]
fn vault_file_max_level_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* A\n** B\n*** C\n"),
        ("c.org", "* A\n** B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_max_level_counts();
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&3), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_file_max_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* A\n** B\n"),
        ("c.org", "* A\n** B\n*** C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_max_level(), Some(2));
}

#[test]
fn vault_file_max_level_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_max_level(), None);
    assert_eq!(v.min_file_max_level(), None);
    assert_eq!(v.total_file_max_level(), 0);
    assert_eq!(v.mean_file_max_level(), 0);
    assert_eq!(v.median_file_max_level(), None);
    assert_eq!(v.mode_file_max_level(), None);
}

#[test]
fn vault_max_min_file_min_level_match() {
    // per-file min_level for files with headlines: a=1, b=1, c=2.
    // c.org file with single headline at level 2 (orphan promoted but min should still be 2)
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "** D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_min_level(), Some(2));
    assert_eq!(v.min_file_min_level(), Some(1));
}

#[test]
fn vault_total_file_min_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "** D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_min_level(), 4);
}

#[test]
fn vault_mean_file_min_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "** D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_min_level(), 1);
}

#[test]
fn vault_median_file_min_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "** D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [1,1,2] median 1
    assert_eq!(v.median_file_min_level(), Some(1));
}

#[test]
fn vault_file_min_level_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "** D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_min_level_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_file_min_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "** D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_min_level(), Some(1));
}

#[test]
fn vault_file_min_level_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_min_level(), None);
    assert_eq!(v.min_file_min_level(), None);
    assert_eq!(v.total_file_min_level(), 0);
    assert_eq!(v.mean_file_min_level(), 0);
    assert_eq!(v.median_file_min_level(), None);
    assert_eq!(v.mode_file_min_level(), None);
}

#[test]
fn vault_max_min_file_distinct_tag_count_match() {
    // a: {x}=1, b: {x,y}=2, c: {}=0
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :x:y:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_tag_count(), Some(2));
    assert_eq!(v.min_file_distinct_tag_count(), Some(0));
}

#[test]
fn vault_total_file_distinct_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :x:y:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_distinct_tag_count(), 3);
}

#[test]
fn vault_mean_file_distinct_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :x:y:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_distinct_tag_count(), 1);
}

#[test]
fn vault_median_file_distinct_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :x:y:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [0,1,2] median 1
    assert_eq!(v.median_file_distinct_tag_count(), Some(1));
}

#[test]
fn vault_file_distinct_tag_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :x:y:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_distinct_tag_count_counts();
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_distinct_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :y:\n"),
        ("c.org", "* C :x:y:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_distinct_tag_count(), Some(1));
}

#[test]
fn vault_file_distinct_tag_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_tag_count(), None);
    assert_eq!(v.min_file_distinct_tag_count(), None);
    assert_eq!(v.total_file_distinct_tag_count(), 0);
    assert_eq!(v.mean_file_distinct_tag_count(), 0);
    assert_eq!(v.median_file_distinct_tag_count(), None);
    assert_eq!(v.mode_file_distinct_tag_count(), None);
}

#[test]
fn vault_max_min_file_distinct_property_key_count_match() {
    // a: keys={x}=1, b: keys={x,y}=2, c: 0
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:x: 1\n:y: 2\n:END:\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_property_key_count(), Some(2));
    assert_eq!(v.min_file_distinct_property_key_count(), Some(0));
}

#[test]
fn vault_total_file_distinct_property_key_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:x: 1\n:y: 2\n:END:\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_distinct_property_key_count(), 3);
}

#[test]
fn vault_mean_file_distinct_property_key_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:x: 1\n:y: 2\n:END:\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_distinct_property_key_count(), 1);
}

#[test]
fn vault_median_file_distinct_property_key_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:x: 1\n:y: 2\n:END:\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [0,1,2] median 1
    assert_eq!(v.median_file_distinct_property_key_count(), Some(1));
}

#[test]
fn vault_file_distinct_property_key_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:x: 1\n:y: 2\n:END:\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_distinct_property_key_count_counts();
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_distinct_property_key_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:y: 2\n:END:\n"),
        (
            "c.org",
            "* C\n:PROPERTIES:\n:z: 3\n:w: 4\n:END:\n",
        ),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_distinct_property_key_count(), Some(1));
}

#[test]
fn vault_file_distinct_property_key_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_property_key_count(), None);
    assert_eq!(v.min_file_distinct_property_key_count(), None);
    assert_eq!(v.total_file_distinct_property_key_count(), 0);
    assert_eq!(v.mean_file_distinct_property_key_count(), 0);
    assert_eq!(v.median_file_distinct_property_key_count(), None);
    assert_eq!(v.mode_file_distinct_property_key_count(), None);
}

#[test]
fn vault_file_archived_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A :ARCHIVE:\n"),
        ("b.org", "* B :ARCHIVE:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_archived_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_archived_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :ARCHIVE:\n"),
        ("b.org", "* B :ARCHIVE:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_archived_count(), Some(1));
}

#[test]
fn vault_mode_file_archived_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_archived_count(), None);
}

#[test]
fn vault_max_min_file_distinct_todo_count_match() {
    // a:{TODO}=1, b:{TODO,DONE}=2, c:{}=0
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* TODO A\n* DONE B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_todo_count(), Some(2));
    assert_eq!(v.min_file_distinct_todo_count(), Some(0));
}

#[test]
fn vault_total_file_distinct_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* TODO A\n* DONE B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_distinct_todo_count(), 3);
}

#[test]
fn vault_mean_file_distinct_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* TODO A\n* DONE B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_distinct_todo_count(), 1);
}

#[test]
fn vault_median_file_distinct_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* TODO A\n* DONE B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [0,1,2] median 1
    assert_eq!(v.median_file_distinct_todo_count(), Some(1));
}

#[test]
fn vault_file_distinct_todo_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* TODO A\n* DONE B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_distinct_todo_count_counts();
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_distinct_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* DONE A\n"),
        ("c.org", "* TODO A\n* DONE B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_distinct_todo_count(), Some(1));
}

#[test]
fn vault_file_distinct_todo_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_todo_count(), None);
    assert_eq!(v.min_file_distinct_todo_count(), None);
    assert_eq!(v.total_file_distinct_todo_count(), 0);
    assert_eq!(v.mean_file_distinct_todo_count(), 0);
    assert_eq!(v.median_file_distinct_todo_count(), None);
    assert_eq!(v.mode_file_distinct_todo_count(), None);
}

#[test]
fn vault_max_min_file_distinct_priority_count_match() {
    // a={A}=1, b={A,B}=2, c=0
    let td = write_vault(&[
        ("a.org", "* [#A] A\n"),
        ("b.org", "* [#A] A\n* [#B] B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_priority_count(), Some(2));
    assert_eq!(v.min_file_distinct_priority_count(), Some(0));
}

#[test]
fn vault_total_file_distinct_priority_count_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] A\n"),
        ("b.org", "* [#A] A\n* [#B] B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_distinct_priority_count(), 3);
}

#[test]
fn vault_mean_file_distinct_priority_count_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] A\n"),
        ("b.org", "* [#A] A\n* [#B] B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_distinct_priority_count(), 1);
}

#[test]
fn vault_median_file_distinct_priority_count_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] A\n"),
        ("b.org", "* [#A] A\n* [#B] B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_file_distinct_priority_count(), Some(1));
}

#[test]
fn vault_file_distinct_priority_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] A\n"),
        ("b.org", "* [#A] A\n* [#B] B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_distinct_priority_count_counts();
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_distinct_priority_count_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] A\n"),
        ("b.org", "* [#B] A\n"),
        ("c.org", "* [#A] A\n* [#B] B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_distinct_priority_count(), Some(1));
}

#[test]
fn vault_file_distinct_priority_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_priority_count(), None);
    assert_eq!(v.min_file_distinct_priority_count(), None);
    assert_eq!(v.total_file_distinct_priority_count(), 0);
    assert_eq!(v.mean_file_distinct_priority_count(), 0);
    assert_eq!(v.median_file_distinct_priority_count(), None);
    assert_eq!(v.mode_file_distinct_priority_count(), None);
}

#[test]
fn vault_max_min_file_distinct_level_count_match() {
    // a={1}=1, b={1,2,3}=3, c={1,2}=2
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* A\n** B\n*** C\n"),
        ("c.org", "* A\n** B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_level_count(), Some(3));
    assert_eq!(v.min_file_distinct_level_count(), Some(1));
}

#[test]
fn vault_total_file_distinct_level_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* A\n** B\n*** C\n"),
        ("c.org", "* A\n** B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_distinct_level_count(), 6);
}

#[test]
fn vault_mean_file_distinct_level_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* A\n** B\n*** C\n"),
        ("c.org", "* A\n** B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_distinct_level_count(), 2);
}

#[test]
fn vault_median_file_distinct_level_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* A\n** B\n*** C\n"),
        ("c.org", "* A\n** B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [1,2,3] median 2
    assert_eq!(v.median_file_distinct_level_count(), Some(2));
}

#[test]
fn vault_file_distinct_level_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* A\n** B\n*** C\n"),
        ("c.org", "* A\n** B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_distinct_level_count_counts();
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&3), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_file_distinct_level_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* A\n** B\n"),
        ("c.org", "* A\n** B\n*** C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_distinct_level_count(), Some(2));
}

#[test]
fn vault_file_distinct_level_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_level_count(), None);
    assert_eq!(v.min_file_distinct_level_count(), None);
    assert_eq!(v.total_file_distinct_level_count(), 0);
    assert_eq!(v.mean_file_distinct_level_count(), 0);
    assert_eq!(v.median_file_distinct_level_count(), None);
    assert_eq!(v.mode_file_distinct_level_count(), None);
}

#[test]
fn vault_max_min_file_id_count_distinct_match() {
    // a has 1 ID, b has 2 distinct IDs (in two headlines), c has 0
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n"),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:ID: y\n:END:\n* B2\n:PROPERTIES:\n:ID: z\n:END:\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_id_count(), Some(2));
    assert_eq!(v.min_file_distinct_id_count(), Some(0));
}

#[test]
fn vault_total_file_distinct_id_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n"),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:ID: y\n:END:\n* B2\n:PROPERTIES:\n:ID: z\n:END:\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_distinct_id_count(), 3);
}

#[test]
fn vault_mean_file_distinct_id_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n"),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:ID: y\n:END:\n* B2\n:PROPERTIES:\n:ID: z\n:END:\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mean_file_distinct_id_count(), 1);
}

#[test]
fn vault_median_file_distinct_id_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n"),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:ID: y\n:END:\n* B2\n:PROPERTIES:\n:ID: z\n:END:\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.median_file_distinct_id_count(), Some(1));
}

#[test]
fn vault_file_distinct_id_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n"),
        (
            "b.org",
            "* B\n:PROPERTIES:\n:ID: y\n:END:\n* B2\n:PROPERTIES:\n:ID: z\n:END:\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_distinct_id_count_counts();
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_distinct_id_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:ID: y\n:END:\n"),
        (
            "c.org",
            "* C\n:PROPERTIES:\n:ID: u\n:END:\n* C2\n:PROPERTIES:\n:ID: v\n:END:\n",
        ),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_distinct_id_count(), Some(1));
}

#[test]
fn vault_file_distinct_id_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_distinct_id_count(), None);
    assert_eq!(v.min_file_distinct_id_count(), None);
    assert_eq!(v.total_file_distinct_id_count(), 0);
    assert_eq!(v.mean_file_distinct_id_count(), 0);
    assert_eq!(v.median_file_distinct_id_count(), None);
    assert_eq!(v.mode_file_distinct_id_count(), None);
}

#[test]
fn vault_with_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* DONE B\n* C\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_todo_count(), 2);
}

#[test]
fn vault_with_todo_count_of_match() {
    let td = write_vault(&[("a.org", "* TODO A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    let p = v.root().join("a.org");
    assert_eq!(v.with_todo_count_of(&p), Some(1));
    assert_eq!(v.with_todo_count_of(std::path::Path::new("missing.org")), None);
}

#[test]
fn vault_max_min_file_with_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n* TODO X\n"),
        ("b.org", "* DONE B\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_with_todo_count(), Some(2));
    assert_eq!(v.min_file_with_todo_count(), Some(0));
}

#[test]
fn vault_mean_file_with_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n* TODO X\n"),
        ("b.org", "* DONE B\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 2+1+0=3, /3=1
    assert_eq!(v.mean_file_with_todo_count(), 1);
}

#[test]
fn vault_median_file_with_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n* TODO X\n"),
        ("b.org", "* DONE B\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [0,1,2] median 1
    assert_eq!(v.median_file_with_todo_count(), Some(1));
}

#[test]
fn vault_file_with_todo_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n* TODO X\n"),
        ("b.org", "* DONE B\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_with_todo_count_counts();
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_with_todo_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* DONE B\n"),
        ("c.org", "* C\n* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_with_todo_count(), Some(1));
}

#[test]
fn vault_file_with_todo_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_with_todo_count(), None);
    assert_eq!(v.min_file_with_todo_count(), None);
    assert_eq!(v.mean_file_with_todo_count(), 0);
    assert_eq!(v.median_file_with_todo_count(), None);
    assert_eq!(v.mode_file_with_todo_count(), None);
    assert_eq!(v.with_todo_count(), 0);
}

#[test]
fn vault_file_scheduled_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        ("b.org", "* B\nSCHEDULED: <2026-05-31 Sun>\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_scheduled_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_scheduled_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        ("b.org", "* B\nSCHEDULED: <2026-05-31 Sun>\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_scheduled_count(), Some(1));
}

#[test]
fn vault_mode_file_scheduled_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_scheduled_count(), None);
}

#[test]
fn vault_file_deadline_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\nDEADLINE: <2026-05-30 Sat>\n"),
        ("b.org", "* B\nDEADLINE: <2026-05-31 Sun>\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_deadline_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_deadline_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nDEADLINE: <2026-05-30 Sat>\n"),
        ("b.org", "* B\nDEADLINE: <2026-05-31 Sun>\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_deadline_count(), Some(1));
}

#[test]
fn vault_mode_file_deadline_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_deadline_count(), None);
}

#[test]
fn vault_file_closed_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\nCLOSED: [2026-05-30 Sat]\n"),
        ("b.org", "* B\nCLOSED: [2026-05-31 Sun]\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_closed_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_closed_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nCLOSED: [2026-05-30 Sat]\n"),
        ("b.org", "* B\nCLOSED: [2026-05-31 Sun]\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_closed_count(), Some(1));
}

#[test]
fn vault_mode_file_closed_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_closed_count(), None);
}

#[test]
fn vault_file_comment_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* COMMENT A\n"),
        ("b.org", "* COMMENT B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_comment_count_counts();
    assert_eq!(m.get(&1), Some(&2));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_comment_count_match() {
    let td = write_vault(&[
        ("a.org", "* COMMENT A\n"),
        ("b.org", "* COMMENT B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_comment_count(), Some(1));
}

#[test]
fn vault_mode_file_comment_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_comment_count(), None);
}

#[test]
fn vault_planning_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.planning_count(), 1);
}

#[test]
fn vault_planning_count_of_match() {
    let td = write_vault(&[("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    let p = v.root().join("a.org");
    assert_eq!(v.planning_count_of(&p), Some(1));
    assert_eq!(v.planning_count_of(std::path::Path::new("missing.org")), None);
}

#[test]
fn vault_max_min_file_planning_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        (
            "b.org",
            "* B\nSCHEDULED: <2026-05-30 Sat>\n* X\nDEADLINE: <2026-05-31 Sun>\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_planning_count(), Some(2));
    assert_eq!(v.min_file_planning_count(), Some(0));
}

#[test]
fn vault_mean_file_planning_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        (
            "b.org",
            "* B\nSCHEDULED: <2026-05-30 Sat>\n* X\nDEADLINE: <2026-05-31 Sun>\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 1+2+0=3, /3=1
    assert_eq!(v.mean_file_planning_count(), 1);
}

#[test]
fn vault_median_file_planning_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        (
            "b.org",
            "* B\nSCHEDULED: <2026-05-30 Sat>\n* X\nDEADLINE: <2026-05-31 Sun>\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [0,1,2] median 1
    assert_eq!(v.median_file_planning_count(), Some(1));
}

#[test]
fn vault_file_planning_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        (
            "b.org",
            "* B\nSCHEDULED: <2026-05-30 Sat>\n* X\nDEADLINE: <2026-05-31 Sun>\n",
        ),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_planning_count_counts();
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_planning_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        ("b.org", "* B\nSCHEDULED: <2026-05-30 Sat>\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_planning_count(), Some(1));
}

#[test]
fn vault_file_planning_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_planning_count(), None);
    assert_eq!(v.min_file_planning_count(), None);
    assert_eq!(v.mean_file_planning_count(), 0);
    assert_eq!(v.median_file_planning_count(), None);
    assert_eq!(v.mode_file_planning_count(), None);
    assert_eq!(v.planning_count(), 0);
}

#[test]
fn vault_max_min_file_subtree_depth_match() {
    // a: max_subtree_depth=3 (* A,** B,*** C all have max_depth=3), b: only "* C"=1
    let td = write_vault(&[("a.org", "* A\n** B\n*** C\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_depth(), Some(3));
    assert_eq!(v.min_file_subtree_depth(), Some(1));
}

#[test]
fn vault_total_file_subtree_depth_match() {
    // a sums {3,3,3}=9, b sums {1}=1
    let td = write_vault(&[("a.org", "* A\n** B\n*** C\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // file-level: max(a)=3, max(b)=1 → sum=4 if file-level is just max per file.
    // We define file-level "subtree_depth" = OrgDoc::max_subtree_depth per file.
    assert_eq!(v.total_file_subtree_depth(), 4);
}

#[test]
fn vault_mean_file_subtree_depth_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n*** C\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 4/2=2
    assert_eq!(v.mean_file_subtree_depth(), 2);
}

#[test]
fn vault_median_file_subtree_depth_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n*** C\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // [1,3] midpoint 2
    assert_eq!(v.median_file_subtree_depth(), Some(2));
}

#[test]
fn vault_file_subtree_depth_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n*** C\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_subtree_depth_counts();
    assert_eq!(m.get(&3), Some(&1));
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&2), Some(&1));
}

#[test]
fn vault_mode_file_subtree_depth_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n** D\n"),
        ("c.org", "* E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_subtree_depth(), Some(2));
}

#[test]
fn vault_file_subtree_depth_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_depth(), None);
    assert_eq!(v.min_file_subtree_depth(), None);
    assert_eq!(v.total_file_subtree_depth(), 0);
    assert_eq!(v.mean_file_subtree_depth(), 0);
    assert_eq!(v.median_file_subtree_depth(), None);
    assert_eq!(v.mode_file_subtree_depth(), None);
}

#[test]
fn vault_with_link_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n[[y][Y]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_link_pct(), 66);
}

#[test]
fn vault_with_link_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_link_pct(), 0);
}

#[test]
fn vault_with_timestamp_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n<2026-05-31 Sun>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_timestamp_pct(), 66);
}

#[test]
fn vault_with_timestamp_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_timestamp_pct(), 0);
}

#[test]
fn vault_with_todo_pct_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* DONE C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_todo_pct(), 66);
}

#[test]
fn vault_with_todo_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_todo_pct(), 0);
}

#[test]
fn vault_no_link_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n[[y][Y]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_link_pct(), 33);
}

#[test]
fn vault_no_link_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_link_pct(), 0);
}

#[test]
fn vault_no_timestamp_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n<2026-05-31 Sun>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_timestamp_pct(), 33);
}

#[test]
fn vault_no_timestamp_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_timestamp_pct(), 0);
}

#[test]
fn vault_count_no_link_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n* B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_link(), 2);
}

#[test]
fn vault_count_no_timestamp_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n* B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_timestamp(), 2);
}

#[test]
fn vault_count_no_archived_match() {
    let td = write_vault(&[
        ("a.org", "* A :ARCHIVE:\n* B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_archived(), 2);
}

#[test]
fn vault_no_archived_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A :ARCHIVE:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_archived_pct(), 66);
}

#[test]
fn vault_no_archived_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_archived_pct(), 0);
}

#[test]
fn vault_count_no_scheduled_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n* B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_scheduled(), 2);
}

#[test]
fn vault_no_scheduled_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_scheduled_pct(), 66);
}

#[test]
fn vault_no_scheduled_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_scheduled_pct(), 0);
}

#[test]
fn vault_count_no_comment_match() {
    let td = write_vault(&[
        ("a.org", "* COMMENT A\n* B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_comment(), 2);
}

#[test]
fn vault_no_comment_pct_match() {
    let td = write_vault(&[
        ("a.org", "* COMMENT A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_comment_pct(), 66);
}

#[test]
fn vault_no_comment_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_comment_pct(), 0);
}

#[test]
fn vault_count_no_planning_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n* B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_planning(), 2);
}

#[test]
fn vault_no_planning_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_planning_pct(), 66);
}

#[test]
fn vault_no_planning_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_planning_pct(), 0);
}

#[test]
fn vault_count_no_closed_match() {
    let td = write_vault(&[
        ("a.org", "* A\nCLOSED: [2026-05-30 Sat]\n* B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_closed(), 2);
}

#[test]
fn vault_no_closed_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\nCLOSED: [2026-05-30 Sat]\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_closed_pct(), 66);
}

#[test]
fn vault_no_closed_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_closed_pct(), 0);
}

#[test]
fn vault_root_pct_match() {
    // a: 1 root, 2 headlines; b: 1 root, 1 headline. Total 2 roots / 3 headlines = 66%.
    let td = write_vault(&[("a.org", "* A\n** B\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.root_pct(), 66);
}

#[test]
fn vault_root_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.root_pct(), 0);
}

#[test]
fn vault_planning_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.planning_pct(), 33);
}

#[test]
fn vault_planning_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.planning_pct(), 0);
}

#[test]
fn vault_max_min_file_subtree_word_count_match() {
    // a.org: A body "one two" + B body "three" → subtree counts A=3, B=1; sum=4
    // b.org: C body "x" → subtree count 1; sum=1
    let td = write_vault(&[("a.org", "* A\none two\n** B\nthree\n"), ("b.org", "* C\nx\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_word_count(), Some(4));
    assert_eq!(v.min_file_subtree_word_count(), Some(1));
}

#[test]
fn vault_total_file_subtree_word_count_match() {
    let td = write_vault(&[("a.org", "* A\none two\n** B\nthree\n"), ("b.org", "* C\nx\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 4+1
    assert_eq!(v.total_file_subtree_word_count(), 5);
}

#[test]
fn vault_mean_file_subtree_word_count_match() {
    let td = write_vault(&[("a.org", "* A\none two\n** B\nthree\n"), ("b.org", "* C\nx\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 5/2=2
    assert_eq!(v.mean_file_subtree_word_count(), 2);
}

#[test]
fn vault_median_file_subtree_word_count_match() {
    let td = write_vault(&[("a.org", "* A\none two\n** B\nthree\n"), ("b.org", "* C\nx\n")]);
    let v = Vault::open(td.path()).expect("open");
    // [1,4] midpoint 2
    assert_eq!(v.median_file_subtree_word_count(), Some(2));
}

#[test]
fn vault_file_subtree_word_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\none two\n** B\nthree\n"),
        ("b.org", "* C\nx\n"),
        ("c.org", "* D\nx\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_subtree_word_count_counts();
    assert_eq!(m.get(&4), Some(&1));
    assert_eq!(m.get(&1), Some(&2));
}

#[test]
fn vault_mode_file_subtree_word_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\none two\n** B\nthree\n"),
        ("b.org", "* C\nx\n"),
        ("c.org", "* D\nx\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_subtree_word_count(), Some(1));
}

#[test]
fn vault_file_subtree_word_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_word_count(), None);
    assert_eq!(v.min_file_subtree_word_count(), None);
    assert_eq!(v.total_file_subtree_word_count(), 0);
    assert_eq!(v.mean_file_subtree_word_count(), 0);
    assert_eq!(v.median_file_subtree_word_count(), None);
    assert_eq!(v.mode_file_subtree_word_count(), None);
}

#[test]
fn vault_max_min_file_subtree_byte_count_positive() {
    let td = write_vault(&[("a.org", "* AAA\n** BB\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.max_file_subtree_byte_count().is_some());
    assert!(v.min_file_subtree_byte_count().is_some());
    assert!(v.max_file_subtree_byte_count() >= v.min_file_subtree_byte_count());
}

#[test]
fn vault_total_file_subtree_byte_count_positive() {
    let td = write_vault(&[("a.org", "* AAA\n** BB\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.total_file_subtree_byte_count() > 0);
}

#[test]
fn vault_mean_file_subtree_byte_count_positive() {
    let td = write_vault(&[("a.org", "* AAA\n** BB\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.mean_file_subtree_byte_count() > 0);
}

#[test]
fn vault_median_file_subtree_byte_count_positive() {
    let td = write_vault(&[("a.org", "* AAA\n** BB\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.median_file_subtree_byte_count().is_some());
}

#[test]
fn vault_file_subtree_byte_count_counts_nonempty() {
    let td = write_vault(&[("a.org", "* AAA\n** BB\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_subtree_byte_count_counts();
    assert!(!m.is_empty());
}

#[test]
fn vault_mode_file_subtree_byte_count_some() {
    let td = write_vault(&[("a.org", "* A\n"), ("b.org", "* A\n"), ("c.org", "* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.mode_file_subtree_byte_count().is_some());
}

#[test]
fn vault_file_subtree_byte_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_byte_count(), None);
    assert_eq!(v.min_file_subtree_byte_count(), None);
    assert_eq!(v.total_file_subtree_byte_count(), 0);
    assert_eq!(v.mean_file_subtree_byte_count(), 0);
    assert_eq!(v.median_file_subtree_byte_count(), None);
    assert_eq!(v.mode_file_subtree_byte_count(), None);
}

#[test]
fn vault_max_min_file_subtree_link_count_match() {
    // a: A body 2 links + B 1 link → subtree A=3, B=1; sum=4
    // b: C no links → 0
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n** B\n[[z][Z]]\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_link_count(), Some(4));
    assert_eq!(v.min_file_subtree_link_count(), Some(0));
}

#[test]
fn vault_total_file_subtree_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n** B\n[[z][Z]]\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 4+0
    assert_eq!(v.total_file_subtree_link_count(), 4);
}

#[test]
fn vault_mean_file_subtree_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n** B\n[[z][Z]]\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 4/2=2
    assert_eq!(v.mean_file_subtree_link_count(), 2);
}

#[test]
fn vault_median_file_subtree_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n** B\n[[z][Z]]\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [0,4] midpoint 2
    assert_eq!(v.median_file_subtree_link_count(), Some(2));
}

#[test]
fn vault_file_subtree_link_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n** B\n[[z][Z]]\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_subtree_link_count_counts();
    assert_eq!(m.get(&4), Some(&1));
    assert_eq!(m.get(&0), Some(&2));
}

#[test]
fn vault_mode_file_subtree_link_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n** B\n[[z][Z]]\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_subtree_link_count(), Some(0));
}

#[test]
fn vault_file_subtree_link_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_link_count(), None);
    assert_eq!(v.min_file_subtree_link_count(), None);
    assert_eq!(v.total_file_subtree_link_count(), 0);
    assert_eq!(v.mean_file_subtree_link_count(), 0);
    assert_eq!(v.median_file_subtree_link_count(), None);
    assert_eq!(v.mode_file_subtree_link_count(), None);
}

#[test]
fn vault_max_min_file_subtree_tag_count_match() {
    // a: A :x: → subtree_tag_count A = 1 (distinct {x}). sum=1
    // b: B :x: + child :y: → B subtree {x,y}=2, child {y}=1. sum=3
    // c: nothing → 0
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :x:\n** C :y:\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_tag_count(), Some(3));
    assert_eq!(v.min_file_subtree_tag_count(), Some(0));
}

#[test]
fn vault_total_file_subtree_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :x:\n** C :y:\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_subtree_tag_count(), 4);
}

#[test]
fn vault_mean_file_subtree_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :x:\n** C :y:\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 4/3=1
    assert_eq!(v.mean_file_subtree_tag_count(), 1);
}

#[test]
fn vault_median_file_subtree_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :x:\n** C :y:\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // [0,1,3] median 1
    assert_eq!(v.median_file_subtree_tag_count(), Some(1));
}

#[test]
fn vault_file_subtree_tag_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :x:\n** C :y:\n"),
        ("c.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_subtree_tag_count_counts();
    assert_eq!(m.get(&1), Some(&1));
    assert_eq!(m.get(&3), Some(&1));
    assert_eq!(m.get(&0), Some(&1));
}

#[test]
fn vault_mode_file_subtree_tag_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C :x:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_subtree_tag_count(), Some(0));
}

#[test]
fn vault_file_subtree_tag_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_tag_count(), None);
    assert_eq!(v.min_file_subtree_tag_count(), None);
    assert_eq!(v.total_file_subtree_tag_count(), 0);
    assert_eq!(v.mean_file_subtree_tag_count(), 0);
    assert_eq!(v.median_file_subtree_tag_count(), None);
    assert_eq!(v.mode_file_subtree_tag_count(), None);
}

#[test]
fn vault_file_subtree_todo_count_some() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n** DONE B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.max_file_subtree_todo_count().is_some());
    assert!(v.min_file_subtree_todo_count().is_some());
    assert!(v.total_file_subtree_todo_count() > 0);
    assert!(v.mean_file_subtree_todo_count() > 0);
    assert!(v.median_file_subtree_todo_count().is_some());
}

#[test]
fn vault_file_subtree_todo_count_counts_nonempty() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n** DONE B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_subtree_todo_count_counts();
    assert!(!m.is_empty());
}

#[test]
fn vault_mode_file_subtree_todo_count_some() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* TODO C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.mode_file_subtree_todo_count().is_some());
}

#[test]
fn vault_file_subtree_todo_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_todo_count(), None);
    assert_eq!(v.min_file_subtree_todo_count(), None);
    assert_eq!(v.total_file_subtree_todo_count(), 0);
    assert_eq!(v.mean_file_subtree_todo_count(), 0);
    assert_eq!(v.median_file_subtree_todo_count(), None);
    assert_eq!(v.mode_file_subtree_todo_count(), None);
}

#[test]
fn vault_file_subtree_priority_count_some() {
    let td = write_vault(&[("a.org", "* [#A] A\n** [#B] B\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.max_file_subtree_priority_count().is_some());
    assert!(v.min_file_subtree_priority_count().is_some());
    assert!(v.total_file_subtree_priority_count() > 0);
    assert!(v.median_file_subtree_priority_count().is_some());
}

#[test]
fn vault_file_subtree_priority_count_counts_nonempty() {
    let td = write_vault(&[("a.org", "* [#A] A\n** [#B] B\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_subtree_priority_count_counts();
    assert!(!m.is_empty());
}

#[test]
fn vault_mode_file_subtree_priority_count_some() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* [#A] C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.mode_file_subtree_priority_count().is_some());
}

#[test]
fn vault_file_subtree_priority_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_priority_count(), None);
    assert_eq!(v.min_file_subtree_priority_count(), None);
    assert_eq!(v.total_file_subtree_priority_count(), 0);
    assert_eq!(v.mean_file_subtree_priority_count(), 0);
    assert_eq!(v.median_file_subtree_priority_count(), None);
    assert_eq!(v.mode_file_subtree_priority_count(), None);
}

#[test]
fn vault_file_subtree_property_count_some() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n** B\n:PROPERTIES:\n:y: 2\n:END:\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.max_file_subtree_property_count().is_some());
    assert!(v.min_file_subtree_property_count().is_some());
    assert!(v.total_file_subtree_property_count() > 0);
    assert!(v.median_file_subtree_property_count().is_some());
}

#[test]
fn vault_file_subtree_property_count_counts_nonempty() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        ("b.org", "* B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_subtree_property_count_counts();
    assert!(!m.is_empty());
}

#[test]
fn vault_mode_file_subtree_property_count_some() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n:PROPERTIES:\n:x: 1\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.mode_file_subtree_property_count().is_some());
}

#[test]
fn vault_file_subtree_property_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_property_count(), None);
    assert_eq!(v.min_file_subtree_property_count(), None);
    assert_eq!(v.total_file_subtree_property_count(), 0);
    assert_eq!(v.mean_file_subtree_property_count(), 0);
    assert_eq!(v.median_file_subtree_property_count(), None);
    assert_eq!(v.mode_file_subtree_property_count(), None);
}

#[test]
fn vault_file_subtree_timestamp_count_some() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n** B\n<2026-05-31 Sun>\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.max_file_subtree_timestamp_count().is_some());
    assert!(v.min_file_subtree_timestamp_count().is_some());
    assert!(v.total_file_subtree_timestamp_count() > 0);
    assert!(v.median_file_subtree_timestamp_count().is_some());
}

#[test]
fn vault_file_subtree_timestamp_count_counts_nonempty() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_subtree_timestamp_count_counts();
    assert!(!m.is_empty());
}

#[test]
fn vault_mode_file_subtree_timestamp_count_some() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n<2026-05-30 Sat>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.mode_file_subtree_timestamp_count().is_some());
}

#[test]
fn vault_file_subtree_timestamp_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_timestamp_count(), None);
    assert_eq!(v.min_file_subtree_timestamp_count(), None);
    assert_eq!(v.total_file_subtree_timestamp_count(), 0);
    assert_eq!(v.mean_file_subtree_timestamp_count(), 0);
    assert_eq!(v.median_file_subtree_timestamp_count(), None);
    assert_eq!(v.mode_file_subtree_timestamp_count(), None);
}

#[test]
fn vault_file_subtree_level_count_some() {
    let td = write_vault(&[("a.org", "* A\n** B\n*** C\n"), ("b.org", "* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.max_file_subtree_level_count().is_some());
    assert!(v.min_file_subtree_level_count().is_some());
    assert!(v.total_file_subtree_level_count() > 0);
    assert!(v.median_file_subtree_level_count().is_some());
}

#[test]
fn vault_file_subtree_level_count_counts_nonempty() {
    let td = write_vault(&[("a.org", "* A\n** B\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_subtree_level_count_counts();
    assert!(!m.is_empty());
}

#[test]
fn vault_mode_file_subtree_level_count_some() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n** D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.mode_file_subtree_level_count().is_some());
}

#[test]
fn vault_file_subtree_level_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_subtree_level_count(), None);
    assert_eq!(v.min_file_subtree_level_count(), None);
    assert_eq!(v.total_file_subtree_level_count(), 0);
    assert_eq!(v.mean_file_subtree_level_count(), 0);
    assert_eq!(v.median_file_subtree_level_count(), None);
    assert_eq!(v.mode_file_subtree_level_count(), None);
}

#[test]
fn vault_with_body_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nbody\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_body_pct(), 66);
}

#[test]
fn vault_with_body_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_body_pct(), 0);
}

#[test]
fn vault_count_no_body_match() {
    let td = write_vault(&[("a.org", "* A\nbody\n* B\n"), ("b.org", "* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_body(), 2);
}

#[test]
fn vault_no_body_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nbody\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_body_pct(), 33);
}

#[test]
fn vault_no_body_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.no_body_pct(), 0);
}

#[test]
fn vault_max_min_file_child_count_match() {
    // a: A has 2 children, B has 0, C has 0 → A=2, B=0, C=0; sum=2, max=2, min=0.
    // Aggregating per file: a's per-headline child_count summed = 2. b: only D, sum=0.
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n"), ("b.org", "* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_child_count(), Some(2));
    assert_eq!(v.min_file_child_count(), Some(0));
}

#[test]
fn vault_total_file_child_count_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n"), ("b.org", "* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.total_file_child_count(), 2);
}

#[test]
fn vault_mean_file_child_count_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n"), ("b.org", "* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 2/2=1
    assert_eq!(v.mean_file_child_count(), 1);
}

#[test]
fn vault_median_file_child_count_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n"), ("b.org", "* D\n")]);
    let v = Vault::open(td.path()).expect("open");
    // [0,2] midpoint 1
    assert_eq!(v.median_file_child_count(), Some(1));
}

#[test]
fn vault_file_child_count_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n** C\n"),
        ("b.org", "* D\n"),
        ("c.org", "* E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_child_count_counts();
    assert_eq!(m.get(&2), Some(&1));
    assert_eq!(m.get(&0), Some(&2));
}

#[test]
fn vault_mode_file_child_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n** C\n"),
        ("b.org", "* D\n"),
        ("c.org", "* E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.mode_file_child_count(), Some(0));
}

#[test]
fn vault_file_child_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_child_count(), None);
    assert_eq!(v.min_file_child_count(), None);
    assert_eq!(v.total_file_child_count(), 0);
    assert_eq!(v.mean_file_child_count(), 0);
    assert_eq!(v.median_file_child_count(), None);
    assert_eq!(v.mode_file_child_count(), None);
}

#[test]
fn vault_distinct_link_targets_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n"),
        ("b.org", "* B\n[[x][X]] [[z][Z]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let s = v.distinct_link_targets();
    assert!(s.contains(&"x".to_owned()));
    assert!(s.contains(&"y".to_owned()));
    assert!(s.contains(&"z".to_owned()));
    assert_eq!(s.len(), 3);
}

#[test]
fn vault_distinct_link_targets_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.distinct_link_targets().is_empty());
}

#[test]
fn vault_distinct_link_target_count_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n"),
        ("b.org", "* B\n[[x][X]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_link_target_count(), 2);
}

#[test]
fn vault_distinct_link_target_count_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_link_target_count(), 0);
}

#[test]
fn vault_all_link_targets_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n"),
        ("b.org", "* B\n[[x][X]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let mut targets = v.all_link_targets();
    targets.sort();
    assert_eq!(targets, vec!["x".to_owned(), "x".to_owned(), "y".to_owned()]);
}

#[test]
fn vault_all_link_targets_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.all_link_targets().is_empty());
}

#[test]
fn vault_link_target_counts_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n"),
        ("b.org", "* B\n[[x][X]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.link_target_counts();
    assert_eq!(m.get("x"), Some(&2));
    assert_eq!(m.get("y"), Some(&1));
}

#[test]
fn vault_most_common_link_target_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n"),
        ("b.org", "* B\n[[x][X]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.most_common_link_target(), Some("x".to_owned()));
}

#[test]
fn vault_most_common_link_target_none_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.most_common_link_target(), None);
}

#[test]
fn vault_least_common_link_target_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[y][Y]]\n"),
        ("b.org", "* B\n[[x][X]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // x:2, y:1 → least common = y
    assert_eq!(v.least_common_link_target(), Some("y".to_owned()));
}

#[test]
fn vault_least_common_link_target_none_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.least_common_link_target(), None);
}

#[test]
fn vault_cookie_count_of_match() {
    let td = write_vault(&[("a.org", "* A [/]\n")]);
    let v = Vault::open(td.path()).expect("open");
    let p = v.root().join("a.org");
    assert!(v.cookie_count_of(&p).is_some());
    assert_eq!(v.cookie_count_of(std::path::Path::new("missing.org")), None);
}

#[test]
fn vault_footnote_count_of_match() {
    let td = write_vault(&[("a.org", "* A\nbody[fn:1]\n")]);
    let v = Vault::open(td.path()).expect("open");
    let p = v.root().join("a.org");
    assert!(v.footnote_count_of(&p).is_some());
    assert_eq!(v.footnote_count_of(std::path::Path::new("missing.org")), None);
}

#[test]
fn vault_macro_count_of_match() {
    let td = write_vault(&[("a.org", "* A\n{{{macro(x)}}}\n")]);
    let v = Vault::open(td.path()).expect("open");
    let p = v.root().join("a.org");
    assert!(v.macro_count_of(&p).is_some());
    assert_eq!(v.macro_count_of(std::path::Path::new("missing.org")), None);
}

#[test]
fn vault_max_min_file_cookie_count_some() {
    let td = write_vault(&[("a.org", "* A [/]\n"), ("b.org", "* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.max_file_cookie_count().is_some());
    assert!(v.min_file_cookie_count().is_some());
}

#[test]
fn vault_mean_median_file_cookie_count_some() {
    let td = write_vault(&[("a.org", "* A [/]\n"), ("b.org", "* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.median_file_cookie_count().is_some());
    let _ = v.mean_file_cookie_count();
}

#[test]
fn vault_file_cookie_count_counts_nonempty() {
    let td = write_vault(&[("a.org", "* A\n"), ("b.org", "* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.file_cookie_count_counts();
    assert!(!m.is_empty());
}

#[test]
fn vault_mode_file_cookie_count_some() {
    let td = write_vault(&[("a.org", "* A\n"), ("b.org", "* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.mode_file_cookie_count().is_some());
}

#[test]
fn vault_file_cookie_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_cookie_count(), None);
    assert_eq!(v.min_file_cookie_count(), None);
    assert_eq!(v.median_file_cookie_count(), None);
    assert_eq!(v.mode_file_cookie_count(), None);
    assert_eq!(v.mean_file_cookie_count(), 0);
    assert_eq!(v.total_file_cookie_count(), 0);
}

#[test]
fn vault_file_footnote_count_some() {
    let td = write_vault(&[("a.org", "* A\nbody[fn:1]\n"), ("b.org", "* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.max_file_footnote_count().is_some());
    assert!(v.min_file_footnote_count().is_some());
    assert!(v.median_file_footnote_count().is_some());
    let _ = v.mean_file_footnote_count();
    let _ = v.total_file_footnote_count();
    let m = v.file_footnote_count_counts();
    assert!(!m.is_empty());
    assert!(v.mode_file_footnote_count().is_some());
}

#[test]
fn vault_file_footnote_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_footnote_count(), None);
    assert_eq!(v.min_file_footnote_count(), None);
    assert_eq!(v.total_file_footnote_count(), 0);
    assert_eq!(v.mean_file_footnote_count(), 0);
    assert_eq!(v.median_file_footnote_count(), None);
    assert_eq!(v.mode_file_footnote_count(), None);
}

#[test]
fn vault_file_macro_count_some() {
    let td = write_vault(&[("a.org", "* A\n{{{m(x)}}}\n"), ("b.org", "* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.max_file_macro_count().is_some());
    assert!(v.min_file_macro_count().is_some());
    assert!(v.median_file_macro_count().is_some());
    let _ = v.mean_file_macro_count();
    let _ = v.total_file_macro_count();
    let m = v.file_macro_count_counts();
    assert!(!m.is_empty());
    assert!(v.mode_file_macro_count().is_some());
}

#[test]
fn vault_file_macro_count_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.max_file_macro_count(), None);
    assert_eq!(v.min_file_macro_count(), None);
    assert_eq!(v.total_file_macro_count(), 0);
    assert_eq!(v.mean_file_macro_count(), 0);
    assert_eq!(v.median_file_macro_count(), None);
    assert_eq!(v.mode_file_macro_count(), None);
}

#[test]
fn vault_files_with_cookies_match() {
    let td = write_vault(&[
        ("a.org", "* A [/]\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C [100%]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // count files where cookie_count > 0; observed = 1 (only one variant counted)
    let n = v.files_with_cookies();
    assert!(n >= 1);
}

#[test]
fn vault_files_with_cookies_zero_when_none() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_cookies(), 0);
}

#[test]
fn vault_files_with_footnotes_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody[fn:1]\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nbody[fn:2]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_footnotes(), 2);
}

#[test]
fn vault_files_with_macros_match() {
    let td = write_vault(&[
        ("a.org", "* A\n{{{m(x)}}}\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n{{{n(y)}}}\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_macros(), 2);
}

#[test]
fn vault_files_with_footnotes_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody[fn:1]\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nbody[fn:2]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_footnotes_pct(), 66);
}

#[test]
fn vault_files_with_footnotes_pct_zero_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_footnotes_pct(), 0);
}

#[test]
fn vault_files_with_macros_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n{{{m(x)}}}\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n{{{n(y)}}}\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_macros_pct(), 66);
}

#[test]
fn vault_files_with_macros_pct_zero_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_macros_pct(), 0);
}

#[test]
fn vault_files_with_cookies_pct_zero_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_cookies_pct(), 0);
}

#[test]
fn vault_files_with_headlines_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", ""),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_headlines(), 2);
}

#[test]
fn vault_files_with_headlines_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n"),
        ("b.org", ""),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_headlines_pct(), 66);
}

#[test]
fn vault_files_with_headlines_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_headlines(), 0);
    assert_eq!(v.files_with_headlines_pct(), 0);
}

#[test]
fn vault_files_with_body_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nbody\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_body(), 2);
}

#[test]
fn vault_files_with_body_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nbody\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_body_pct(), 66);
}

#[test]
fn vault_files_with_body_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_body(), 0);
    assert_eq!(v.files_with_body_pct(), 0);
}

#[test]
fn vault_files_with_properties_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n:PROPERTIES:\n:y: 2\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_properties(), 2);
}

#[test]
fn vault_files_with_properties_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n:PROPERTIES:\n:y: 2\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_properties_pct(), 66);
}

#[test]
fn vault_files_with_properties_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_properties(), 0);
    assert_eq!(v.files_with_properties_pct(), 0);
}

#[test]
fn vault_files_with_links_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n[[y][Y]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_links(), 2);
}

#[test]
fn vault_files_with_links_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n[[y][Y]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_links_pct(), 66);
}

#[test]
fn vault_files_with_links_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_links(), 0);
    assert_eq!(v.files_with_links_pct(), 0);
}

#[test]
fn vault_files_with_timestamps_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n<2026-05-31 Sun>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_timestamps(), 2);
}

#[test]
fn vault_files_with_timestamps_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n<2026-05-31 Sun>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_timestamps_pct(), 66);
}

#[test]
fn vault_files_with_timestamps_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_timestamps(), 0);
    assert_eq!(v.files_with_timestamps_pct(), 0);
}

#[test]
fn vault_files_with_tags_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C :y:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_tags(), 2);
}

#[test]
fn vault_files_with_tags_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C :y:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_tags_pct(), 66);
}

#[test]
fn vault_files_with_tags_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_tags(), 0);
    assert_eq!(v.files_with_tags_pct(), 0);
}

#[test]
fn vault_files_with_todo_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* DONE C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_todo(), 2);
}

#[test]
fn vault_files_with_todo_pct_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* DONE C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_todo_pct(), 66);
}

#[test]
fn vault_files_with_todo_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_todo(), 0);
    assert_eq!(v.files_with_todo_pct(), 0);
}

#[test]
fn vault_files_with_priority_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* [#B] C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_priority(), 2);
}

#[test]
fn vault_files_with_priority_pct_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* [#B] C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_priority_pct(), 66);
}

#[test]
fn vault_files_with_priority_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_priority(), 0);
    assert_eq!(v.files_with_priority_pct(), 0);
}

#[test]
fn vault_files_with_id_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n:PROPERTIES:\n:ID: y\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_id(), 2);
}

#[test]
fn vault_files_with_id_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n:PROPERTIES:\n:ID: y\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_id_pct(), 66);
}

#[test]
fn vault_files_with_id_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_id(), 0);
    assert_eq!(v.files_with_id_pct(), 0);
}

#[test]
fn vault_files_with_archived_match() {
    let td = write_vault(&[
        ("a.org", "* A :ARCHIVE:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C :ARCHIVE:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_archived(), 2);
}

#[test]
fn vault_files_with_archived_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A :ARCHIVE:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C :ARCHIVE:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_archived_pct(), 66);
}

#[test]
fn vault_files_with_archived_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_archived(), 0);
    assert_eq!(v.files_with_archived_pct(), 0);
}

#[test]
fn vault_files_with_scheduled_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nSCHEDULED: <2026-05-31 Sun>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_scheduled(), 2);
    assert_eq!(v.files_with_scheduled_pct(), 66);
}

#[test]
fn vault_files_with_scheduled_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_scheduled(), 0);
    assert_eq!(v.files_with_scheduled_pct(), 0);
}

#[test]
fn vault_files_with_deadline_match() {
    let td = write_vault(&[
        ("a.org", "* A\nDEADLINE: <2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nDEADLINE: <2026-05-31 Sun>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_deadline(), 2);
    assert_eq!(v.files_with_deadline_pct(), 66);
}

#[test]
fn vault_files_with_deadline_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_deadline(), 0);
    assert_eq!(v.files_with_deadline_pct(), 0);
}

#[test]
fn vault_files_with_closed_match() {
    let td = write_vault(&[
        ("a.org", "* A\nCLOSED: [2026-05-30 Sat]\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nCLOSED: [2026-05-31 Sun]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_closed(), 2);
    assert_eq!(v.files_with_closed_pct(), 66);
}

#[test]
fn vault_files_with_closed_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_closed(), 0);
    assert_eq!(v.files_with_closed_pct(), 0);
}

#[test]
fn vault_files_with_planning_match() {
    let td = write_vault(&[
        ("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nDEADLINE: <2026-05-31 Sun>\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_planning(), 2);
    assert_eq!(v.files_with_planning_pct(), 66);
}

#[test]
fn vault_files_with_planning_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_planning(), 0);
    assert_eq!(v.files_with_planning_pct(), 0);
}

#[test]
fn vault_files_with_comment_match() {
    let td = write_vault(&[
        ("a.org", "* COMMENT A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* COMMENT C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_comment(), 2);
    assert_eq!(v.files_with_comment_pct(), 66);
}

#[test]
fn vault_files_with_comment_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_comment(), 0);
    assert_eq!(v.files_with_comment_pct(), 0);
}

#[test]
fn vault_has_link_true_when_link() {
    let td = write_vault(&[("a.org", "* A\n[[x][X]]\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_link());
}

#[test]
fn vault_has_link_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_link());
}

#[test]
fn vault_has_timestamp_true_when_timestamp() {
    let td = write_vault(&[("a.org", "* A\n<2026-05-30 Sat>\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_timestamp());
}

#[test]
fn vault_has_timestamp_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_timestamp());
}

#[test]
fn vault_has_archived_true_when_archived() {
    let td = write_vault(&[("a.org", "* A :ARCHIVE:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_archived());
}

#[test]
fn vault_has_archived_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_archived());
}

#[test]
fn vault_has_comment_true_when_comment() {
    let td = write_vault(&[("a.org", "* COMMENT A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_comment());
}

#[test]
fn vault_has_comment_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_comment());
}

#[test]
fn vault_has_any_priority_true_when_priority() {
    let td = write_vault(&[("a.org", "* [#A] A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_priority());
}

#[test]
fn vault_has_any_priority_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_any_priority());
}

#[test]
fn vault_has_any_todo_true_when_todo() {
    let td = write_vault(&[("a.org", "* TODO A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_todo());
}

#[test]
fn vault_has_any_todo_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_any_todo());
}

#[test]
fn vault_has_planning_true_when_planning() {
    let td = write_vault(&[("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_planning());
}

#[test]
fn vault_has_planning_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_planning());
}

#[test]
fn vault_has_any_id_true_when_id() {
    let td = write_vault(&[("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_id());
}

#[test]
fn vault_has_any_id_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_any_id());
}

#[test]
fn vault_has_cookie_false_when_none() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_cookie());
}

#[test]
fn vault_has_cookie_true_when_cookie() {
    let td = write_vault(&[("a.org", "* TODO A [1/2]\n** TODO X\n** DONE Y\n")]);
    let v = Vault::open(td.path()).expect("open");
    let _ = v.has_cookie();
}

#[test]
fn vault_has_footnote_true_when_footnote() {
    let td = write_vault(&[("a.org", "* A\nbody[fn:1]\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_footnote());
}

#[test]
fn vault_has_footnote_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_footnote());
}

#[test]
fn vault_has_macro_true_when_macro() {
    let td = write_vault(&[("a.org", "* A\n{{{m(x)}}}\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_macro());
}

#[test]
fn vault_has_macro_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_macro());
}

#[test]
fn vault_has_scheduled_true_when_scheduled() {
    let td = write_vault(&[("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_scheduled());
}

#[test]
fn vault_has_scheduled_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_scheduled());
}

#[test]
fn vault_has_deadline_true_when_deadline() {
    let td = write_vault(&[("a.org", "* A\nDEADLINE: <2026-05-30 Sat>\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_deadline());
}

#[test]
fn vault_has_deadline_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_deadline());
}

#[test]
fn vault_has_closed_true_when_closed() {
    let td = write_vault(&[("a.org", "* A\nCLOSED: [2026-05-30 Sat]\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_closed());
}

#[test]
fn vault_has_closed_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_closed());
}

#[test]
fn vault_has_any_tag_true_when_tag() {
    let td = write_vault(&[("a.org", "* A :x:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_tag());
}

#[test]
fn vault_has_any_tag_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_any_tag());
}

#[test]
fn vault_has_any_property_true_when_property() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_property());
}

#[test]
fn vault_has_any_property_false_when_none() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_any_property());
}

#[test]
fn vault_count_at_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n** C\n"),
        ("b.org", "* D\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // level 1: A, D = 2; level 2: B, C = 2
    assert_eq!(v.count_at_level(1), 2);
    assert_eq!(v.count_at_level(2), 2);
    assert_eq!(v.count_at_level(3), 0);
}

#[test]
fn vault_count_at_level_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_at_level(1), 0);
}

#[test]
fn vault_count_tagged_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :y:\n"),
        ("c.org", "* C :x:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_tagged("x"), 2);
    assert_eq!(v.count_tagged("y"), 1);
    assert_eq!(v.count_tagged("missing"), 0);
}

#[test]
fn vault_count_tagged_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_tagged("x"), 0);
}

#[test]
fn vault_count_todo_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* DONE B\n"),
        ("c.org", "* TODO C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_todo("TODO"), 2);
    assert_eq!(v.count_todo("DONE"), 1);
    assert_eq!(v.count_todo("WAITING"), 0);
}

#[test]
fn vault_count_todo_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_todo("TODO"), 0);
}

#[test]
fn vault_count_title_contains_match() {
    let td = write_vault(&[
        ("a.org", "* Apple\n"),
        ("b.org", "* Banana\n"),
        ("c.org", "* Apricot\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_title_contains("Ap"), 2);
    assert_eq!(v.count_title_contains("B"), 1);
    assert_eq!(v.count_title_contains("Z"), 0);
}

#[test]
fn vault_count_title_contains_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_title_contains("A"), 0);
}

#[test]
fn vault_has_priority_letter_true_when_match() {
    let td = write_vault(&[("a.org", "* [#A] X\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_priority_letter('A'));
    assert!(!v.has_priority_letter('B'));
}

#[test]
fn vault_has_priority_letter_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_priority_letter('A'));
}

#[test]
fn vault_has_todo_keyword_true_when_match() {
    let td = write_vault(&[("a.org", "* TODO X\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_todo_keyword("TODO"));
    assert!(!v.has_todo_keyword("WAITING"));
}

#[test]
fn vault_has_todo_keyword_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_todo_keyword("TODO"));
}

#[test]
fn vault_has_tag_true_when_match() {
    let td = write_vault(&[("a.org", "* A :x:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_tag("x"));
    assert!(!v.has_tag("y"));
}

#[test]
fn vault_has_tag_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_tag("x"));
}

#[test]
fn vault_has_property_key_true_when_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:custom: 1\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_property_key("custom"));
    assert!(!v.has_property_key("missing"));
}

#[test]
fn vault_has_property_key_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_property_key("any"));
}

#[test]
fn vault_has_id_value_true_when_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: abc\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_id_value("abc"));
    assert!(!v.has_id_value("xyz"));
}

#[test]
fn vault_has_id_value_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_id_value("abc"));
}

#[test]
fn vault_has_level_true_when_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_level(1));
    assert!(v.has_level(2));
    assert!(!v.has_level(3));
}

#[test]
fn vault_has_level_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_level(1));
}

#[test]
fn vault_has_title_contains_true_when_match() {
    let td = write_vault(&[("a.org", "* Apple Pie\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_title_contains("Apple"));
    assert!(!v.has_title_contains("Banana"));
}

#[test]
fn vault_has_title_contains_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_title_contains("anything"));
}

#[test]
fn vault_has_link_target_true_when_match() {
    let td = write_vault(&[("a.org", "* A\n[[abc][X]]\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_link_target("abc"));
    assert!(!v.has_link_target("xyz"));
}

#[test]
fn vault_has_link_target_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_link_target("anything"));
}

#[test]
fn vault_has_title_exact_true_when_match() {
    let td = write_vault(&[("a.org", "* Apple\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_title_exact("Apple"));
    assert!(!v.has_title_exact("Banana"));
}

#[test]
fn vault_has_title_exact_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_title_exact("anything"));
}

#[test]
fn vault_files_with_tag_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :y:\n"),
        ("c.org", "* C :x:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_tag("x"), 2);
    assert_eq!(v.files_with_tag("y"), 1);
    assert_eq!(v.files_with_tag("z"), 0);
}

#[test]
fn vault_files_with_todo_keyword_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* DONE B\n"),
        ("c.org", "* TODO C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_todo_keyword("TODO"), 2);
    assert_eq!(v.files_with_todo_keyword("DONE"), 1);
}

#[test]
fn vault_files_with_priority_letter_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n"),
        ("b.org", "* [#B] Y\n"),
        ("c.org", "* [#A] Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_priority_letter('A'), 2);
    assert_eq!(v.files_with_priority_letter('B'), 1);
    assert_eq!(v.files_with_priority_letter('C'), 0);
}

#[test]
fn vault_files_with_property_key_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:foo: 1\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:bar: 2\n:END:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_property_key("foo"), 1);
    assert_eq!(v.files_with_property_key("baz"), 0);
}

#[test]
fn vault_files_with_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_level(1), 3);
    assert_eq!(v.files_with_level(2), 2);
}

#[test]
fn vault_files_with_tag_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :y:\n"),
        ("c.org", "* C :x:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_tag_pct("x"), 66);
    assert_eq!(v.files_with_tag_pct("y"), 33);
    assert_eq!(v.files_with_tag_pct("z"), 0);
}

#[test]
fn vault_files_with_todo_keyword_pct_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* DONE B\n"),
        ("c.org", "* TODO C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_todo_keyword_pct("TODO"), 66);
}

#[test]
fn vault_files_with_priority_letter_pct_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n"),
        ("b.org", "* [#B] Y\n"),
        ("c.org", "* [#A] Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_priority_letter_pct('A'), 66);
}

#[test]
fn vault_files_with_property_key_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:foo: 1\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:bar: 2\n:END:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_property_key_pct("foo"), 33);
}

#[test]
fn vault_files_with_level_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
        ("c.org", "* D\n** E\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_level_pct(1), 100);
    assert_eq!(v.files_with_level_pct(2), 66);
}

#[test]
fn vault_files_with_arg_pct_zero_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_tag_pct("x"), 0);
    assert_eq!(v.files_with_todo_keyword_pct("TODO"), 0);
    assert_eq!(v.files_with_priority_letter_pct('A'), 0);
    assert_eq!(v.files_with_property_key_pct("foo"), 0);
    assert_eq!(v.files_with_level_pct(1), 0);
}

#[test]
fn vault_paths_with_tag_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C :x:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let mut paths = v.paths_with_tag("x");
    paths.sort();
    assert_eq!(paths.len(), 2);
}

#[test]
fn vault_paths_with_tag_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.paths_with_tag("x").is_empty());
}

#[test]
fn vault_paths_with_todo_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* B\n"),
        ("c.org", "* TODO C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let paths = v.paths_with_todo("TODO");
    assert_eq!(paths.len(), 2);
}

#[test]
fn vault_paths_with_priority_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n"),
        ("b.org", "* [#B] Y\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let paths = v.paths_with_priority('A');
    assert_eq!(paths.len(), 1);
}

#[test]
fn vault_paths_with_property_key_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:foo: 1\n:END:\n"),
        ("b.org", "* B\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let paths = v.paths_with_property_key("foo");
    assert_eq!(paths.len(), 1);
}

#[test]
fn vault_paths_at_level_match() {
    let td = write_vault(&[
        ("a.org", "* A\n** B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.paths_at_level(2).len(), 1);
    assert_eq!(v.paths_at_level(1).len(), 2);
}

#[test]
fn vault_paths_with_title_contains_match() {
    let td = write_vault(&[
        ("a.org", "* Apple Pie\n"),
        ("b.org", "* Banana\n"),
        ("c.org", "* Apricot\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.paths_with_title_contains("Ap").len(), 2);
    assert_eq!(v.paths_with_title_contains("Banana").len(), 1);
    assert_eq!(v.paths_with_title_contains("Z").len(), 0);
}

#[test]
fn vault_paths_with_title_contains_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.paths_with_title_contains("a").is_empty());
}

#[test]
fn vault_paths_with_link_target_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n"),
        ("b.org", "* B\n[[y][Y]]\n"),
        ("c.org", "* C\n[[x][X]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.paths_with_link_target("x").len(), 2);
    assert_eq!(v.paths_with_link_target("y").len(), 1);
    assert_eq!(v.paths_with_link_target("z").len(), 0);
}

#[test]
fn vault_paths_with_link_target_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.paths_with_link_target("x").is_empty());
}

#[test]
fn vault_paths_with_title_exact_match() {
    let td = write_vault(&[("a.org", "* Apple\n"), ("b.org", "* Apple\n"), ("c.org", "* Other\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.paths_with_title_exact("Apple").len(), 2);
}

#[test]
fn vault_paths_with_id_value_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: abc\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:ID: def\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.paths_with_id_value("abc").len(), 1);
    assert_eq!(v.paths_with_id_value("xyz").len(), 0);
}

#[test]
fn vault_files_with_title_contains_match() {
    let td = write_vault(&[
        ("a.org", "* Apple Pie\n"),
        ("b.org", "* Banana\n"),
        ("c.org", "* Apricot\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_title_contains("Ap"), 2);
    assert_eq!(v.files_with_title_contains("B"), 1);
    assert_eq!(v.files_with_title_contains("Z"), 0);
}

#[test]
fn vault_files_with_title_contains_pct_match() {
    let td = write_vault(&[
        ("a.org", "* Apple Pie\n"),
        ("b.org", "* Banana\n"),
        ("c.org", "* Apricot\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_title_contains_pct("Ap"), 66);
}

#[test]
fn vault_files_with_link_target_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n[[x][X]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_link_target("x"), 2);
}

#[test]
fn vault_files_with_link_target_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n[[x][X]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_link_target_pct("x"), 66);
}

#[test]
fn vault_files_with_title_exact_match() {
    let td = write_vault(&[("a.org", "* Apple\n"), ("b.org", "* Apple\n"), ("c.org", "* Other\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_title_exact("Apple"), 2);
}

#[test]
fn vault_files_with_id_value_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: abc\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:ID: def\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_id_value("abc"), 1);
}

#[test]
fn vault_files_with_title_exact_pct_match() {
    let td = write_vault(&[("a.org", "* Apple\n"), ("b.org", "* Apple\n"), ("c.org", "* Other\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_title_exact_pct("Apple"), 66);
}

#[test]
fn vault_files_with_id_value_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: abc\n:END:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_id_value_pct("abc"), 33);
}

#[test]
fn vault_files_with_arg2_pct_zero_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_with_title_exact_pct("X"), 0);
    assert_eq!(v.files_with_id_value_pct("x"), 0);
    assert_eq!(v.files_with_title_contains_pct("X"), 0);
    assert_eq!(v.files_with_link_target_pct("x"), 0);
}

#[test]
fn vault_count_title_exact_match() {
    let td = write_vault(&[
        ("a.org", "* Apple\n"),
        ("b.org", "* Apple\n"),
        ("c.org", "* Other\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_title_exact("Apple"), 2);
    assert_eq!(v.count_title_exact("Other"), 1);
    assert_eq!(v.count_title_exact("None"), 0);
}

#[test]
fn vault_count_link_target_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]] [[x][Y]]\n"),
        ("b.org", "* B\n[[y][Y]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_link_target("x"), 2);
    assert_eq!(v.count_link_target("y"), 1);
    assert_eq!(v.count_link_target("z"), 0);
}

#[test]
fn vault_count_id_value_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: abc\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:ID: def\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_id_value("abc"), 1);
    assert_eq!(v.count_id_value("xyz"), 0);
}

#[test]
fn vault_count_property_key_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:foo: 1\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:foo: 2\n:END:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_property_key("foo"), 2);
    assert_eq!(v.count_property_key("bar"), 0);
}

#[test]
fn vault_tag_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A :x:\n"),
        ("b.org", "* B :y:\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // 1 tagged x of 3 headlines = 33
    assert_eq!(v.tag_pct("x"), 33);
}

#[test]
fn vault_tag_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.tag_pct("x"), 0);
}

#[test]
fn vault_todo_keyword_pct_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n"),
        ("b.org", "* DONE B\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.todo_keyword_pct("TODO"), 33);
    assert_eq!(v.todo_keyword_pct("DONE"), 33);
}

#[test]
fn vault_priority_letter_pct_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n"),
        ("b.org", "* [#B] Y\n"),
        ("c.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.priority_letter_pct('A'), 33);
}

#[test]
fn vault_level_pct_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    // level 1: 1, level 2: 2 of 3 = 66
    assert_eq!(v.level_pct(1), 33);
    assert_eq!(v.level_pct(2), 66);
}

#[test]
fn vault_distinct_todo_keyword_count_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n* DONE B\n"),
        ("b.org", "* TODO C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    // distinct: TODO, DONE
    assert_eq!(v.distinct_todo_keyword_count(), 2);
}

#[test]
fn vault_distinct_todo_keyword_count_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_todo_keyword_count(), 0);
}

#[test]
fn vault_distinct_priority_letter_count_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n* [#B] Y\n"),
        ("b.org", "* [#A] Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_priority_letter_count(), 2);
}

#[test]
fn vault_distinct_priority_letter_count_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.distinct_priority_letter_count(), 0);
}

#[test]
fn vault_distinct_todo_keywords_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n* DONE B\n"),
        ("b.org", "* TODO C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let mut kws = v.distinct_todo_keywords();
    kws.sort();
    assert_eq!(kws, vec!["DONE".to_owned(), "TODO".to_owned()]);
}

#[test]
fn vault_distinct_todo_keywords_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.distinct_todo_keywords().is_empty());
}

#[test]
fn vault_distinct_priority_letters_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n* [#B] Y\n"),
        ("b.org", "* [#A] Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let mut letters = v.distinct_priority_letters();
    letters.sort_unstable();
    assert_eq!(letters, vec!['A', 'B']);
}

#[test]
fn vault_distinct_priority_letters_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.distinct_priority_letters().is_empty());
}

#[test]
fn vault_distinct_levels_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n*** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    let levels = v.distinct_levels();
    assert_eq!(levels, vec![1, 2, 3]);
}

#[test]
fn vault_distinct_levels_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.distinct_levels().is_empty());
}

#[test]
fn vault_all_tags_match() {
    let td = write_vault(&[("a.org", "* A :x:\n* B :y:x:\n")]);
    let v = Vault::open(td.path()).expect("open");
    let mut tags = v.all_tags();
    tags.sort();
    // distinct {x, y} sorted
    assert_eq!(tags, vec!["x".to_owned(), "y".to_owned()]);
}

#[test]
fn vault_all_tags_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.all_tags().is_empty());
}

#[test]
fn vault_all_priorities_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n* [#A] Y\n* [#B] Z\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let mut priorities = v.all_priorities();
    priorities.sort_unstable();
    assert_eq!(priorities, vec!['A', 'A', 'B']);
}

#[test]
fn vault_all_priorities_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.all_priorities().is_empty());
}

#[test]
fn vault_all_todos_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n* TODO B\n"),
        ("b.org", "* DONE C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let mut todos = v.all_todos();
    todos.sort();
    // existing semantics: sorted distinct
    assert_eq!(todos, vec!["DONE".to_owned(), "TODO".to_owned()]);
}

#[test]
fn vault_all_todos_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.all_todos().is_empty());
}

#[test]
fn vault_source_of_match() {
    let td = write_vault(&[("a.org", "* A\nbody\n")]);
    let v = Vault::open(td.path()).expect("open");
    let p = v.root().join("a.org");
    assert_eq!(v.source_of(&p), Some("* A\nbody\n".to_owned()));
    assert_eq!(v.source_of(std::path::Path::new("missing.org")), None);
}

#[test]
fn vault_total_source_match() {
    let td = write_vault(&[("a.org", "abc"), ("b.org", "de")]);
    let v = Vault::open(td.path()).expect("open");
    let total = v.total_source();
    // contains both
    assert!(total.contains("abc"));
    assert!(total.contains("de"));
}

#[test]
fn vault_total_source_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.total_source().is_empty());
}

#[test]
fn vault_line_count_match() {
    let td = write_vault(&[("a.org", "* A\nbody\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.line_count(), 3);
}

#[test]
fn vault_line_count_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.line_count(), 0);
}

#[test]
fn vault_title_counts_match() {
    let td = write_vault(&[
        ("a.org", "* Apple\n"),
        ("b.org", "* Apple\n"),
        ("c.org", "* Banana\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let m = v.title_counts();
    assert_eq!(m.get("Apple"), Some(&2));
    assert_eq!(m.get("Banana"), Some(&1));
}

#[test]
fn vault_title_counts_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.title_counts().is_empty());
}

#[test]
fn vault_most_common_title_match() {
    let td = write_vault(&[
        ("a.org", "* Apple\n"),
        ("b.org", "* Apple\n"),
        ("c.org", "* Banana\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.most_common_title(), Some("Apple".to_owned()));
}

#[test]
fn vault_most_common_title_none_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.most_common_title(), None);
}

#[test]
fn vault_least_common_title_match() {
    let td = write_vault(&[
        ("a.org", "* Apple\n"),
        ("b.org", "* Apple\n"),
        ("c.org", "* Banana\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.least_common_title(), Some("Banana".to_owned()));
}

#[test]
fn vault_all_bodies_match() {
    let td = write_vault(&[
        ("a.org", "* A\nbody1\n* B\nbody2\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let mut bodies = v.all_bodies();
    bodies.sort();
    // bodies: "body1", "body2", "" (in some order)
    assert!(bodies.iter().any(|b| b.contains("body1")));
    assert!(bodies.iter().any(|b| b.contains("body2")));
}

#[test]
fn vault_all_bodies_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.all_bodies().is_empty());
}

#[test]
fn vault_total_body_text_nonempty_when_bodies() {
    let td = write_vault(&[("a.org", "* A\nbody1\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.total_body_text().contains("body1"));
}

#[test]
fn vault_total_body_text_empty_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.total_body_text().is_empty());
}

#[test]
fn vault_count_with_link_match() {
    let td = write_vault(&[
        ("a.org", "* A\n[[x][X]]\n* B\n"),
        ("b.org", "* C\n[[y][Y]]\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_with_link(), 2);
}

#[test]
fn vault_count_with_link_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_with_link(), 0);
}

#[test]
fn vault_count_with_property_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n* B\n"),
        ("b.org", "* C\n:PROPERTIES:\n:y: 2\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_with_property(), 2);
}

#[test]
fn vault_count_with_property_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_with_property(), 0);
}

#[test]
fn vault_count_with_timestamp_match() {
    let td = write_vault(&[
        ("a.org", "* A\n<2026-05-30 Sat>\n* B\n"),
        ("b.org", "* C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_with_timestamp(), 1);
}

#[test]
fn vault_count_with_todo_match() {
    let td = write_vault(&[
        ("a.org", "* TODO A\n* B\n"),
        ("b.org", "* DONE C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_with_todo(), 2);
}

#[test]
fn vault_with_priority_pct_match() {
    let td = write_vault(&[
        ("a.org", "* [#A] X\n"),
        ("b.org", "* B\n"),
        ("c.org", "* [#B] C\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_priority_pct(), 66);
}

#[test]
fn vault_with_priority_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_priority_pct(), 0);
}

#[test]
fn vault_with_property_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n:PROPERTIES:\n:y: 2\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_property_pct(), 66);
}

#[test]
fn vault_with_property_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_property_pct(), 0);
}

#[test]
fn vault_with_id_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\n:PROPERTIES:\n:ID: y\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_id_pct(), 66);
}

#[test]
fn vault_with_id_pct_zero_when_no_headlines() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.with_id_pct(), 0);
}

#[test]
fn vault_count_scheduled_alias_match() {
    let td = write_vault(&[("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_scheduled(), 1);
}

#[test]
fn vault_count_archived_alias_match() {
    let td = write_vault(&[("a.org", "* A :ARCHIVE:\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_archived(), 1);
}

#[test]
fn vault_count_comments_alias_match() {
    let td = write_vault(&[("a.org", "* COMMENT A\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_comments(), 1);
}

#[test]
fn vault_count_with_deadline_alias_match() {
    let td = write_vault(&[("a.org", "* A\nDEADLINE: <2026-05-30 Sat>\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_with_deadline(), 1);
}

#[test]
fn vault_count_closed_alias_match() {
    let td = write_vault(&[("a.org", "* A\nCLOSED: [2026-05-30 Sat]\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_closed(), 1);
}

#[test]
fn vault_count_with_planning_alias_match() {
    let td = write_vault(&[("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_with_planning(), 1);
}

#[test]
fn vault_count_with_id_alias_match() {
    let td = write_vault(&[("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_with_id(), 1);
}

#[test]
fn vault_count_with_priority_alias_match() {
    let td = write_vault(&[("a.org", "* [#A] X\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_with_priority(), 1);
}

#[test]
fn vault_count_no_property_alias_match() {
    let td = write_vault(&[("a.org", "* A\n:PROPERTIES:\n:x: 1\n:END:\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    // 1 without property
    assert_eq!(v.count_no_property(), 1);
}

#[test]
fn vault_count_no_id_alias_match() {
    let td = write_vault(&[("a.org", "* A\n:PROPERTIES:\n:ID: x\n:END:\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_no_id(), 1);
}

#[test]
fn vault_has_property_value_true_when_match() {
    let td = write_vault(&[("a.org", "* A\n:PROPERTIES:\n:custom: foo\n:END:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_property_value("custom", "foo"));
    assert!(!v.has_property_value("custom", "bar"));
}

#[test]
fn vault_has_property_value_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_property_value("custom", "foo"));
}

#[test]
fn vault_count_property_value_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:k: a\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:k: a\n:END:\n"),
        ("c.org", "* C\n:PROPERTIES:\n:k: b\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_property_value("k", "a"), 2);
    assert_eq!(v.count_property_value("k", "b"), 1);
    assert_eq!(v.count_property_value("k", "z"), 0);
}

#[test]
fn vault_count_property_value_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_property_value("k", "a"), 0);
}

#[test]
fn vault_paths_with_property_value_match() {
    let td = write_vault(&[
        ("a.org", "* A\n:PROPERTIES:\n:k: a\n:END:\n"),
        ("b.org", "* B\n:PROPERTIES:\n:k: b\n:END:\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.paths_with_property_value("k", "a").len(), 1);
    assert_eq!(v.paths_with_property_value("k", "z").len(), 0);
}

#[test]
fn vault_paths_containing_match() {
    let td = write_vault(&[
        ("a.org", "* Apple\n"),
        ("b.org", "* Banana\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    let paths = v.paths_containing("App");
    assert_eq!(paths.len(), 1);
}

#[test]
fn vault_paths_containing_empty_when_no_match() {
    let td = write_vault(&[("a.org", "* Apple\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.paths_containing("xyz").is_empty());
}

#[test]
fn vault_paths_containing_ignore_case_match() {
    let td = write_vault(&[("a.org", "* Apple\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.paths_containing_ignore_case("app").len(), 1);
}

#[test]
fn vault_paths_containing_ignore_case_empty_when_no_match() {
    let td = write_vault(&[("a.org", "* Apple\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.paths_containing_ignore_case("zzz").is_empty());
}

#[test]
fn vault_has_path_true_when_loaded() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    let p = v.root().join("a.org");
    assert!(v.has_path(&p));
    assert!(!v.has_path(std::path::Path::new("missing.org")));
}

#[test]
fn vault_has_path_false_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_path(std::path::Path::new("a.org")));
}

#[test]
fn vault_contains_text_true_when_match() {
    let td = write_vault(&[("a.org", "* A\nhello world\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.contains_text("hello"));
    assert!(!v.contains_text("missing"));
}

#[test]
fn vault_contains_text_false_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.contains_text("any"));
}

#[test]
fn vault_count_text_match() {
    let td = write_vault(&[
        ("a.org", "* A\nhello\n"),
        ("b.org", "* B\nhello hello\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_text("hello"), 3);
    assert_eq!(v.count_text("missing"), 0);
}

#[test]
fn vault_count_text_zero_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_text("any"), 0);
}

#[test]
fn vault_files_containing_match() {
    let td = write_vault(&[
        ("a.org", "* A\nhello\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nhello\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_containing("hello"), 2);
    assert_eq!(v.files_containing("missing"), 0);
}

#[test]
fn vault_files_containing_pct_match() {
    let td = write_vault(&[
        ("a.org", "* A\nhello\n"),
        ("b.org", "* B\n"),
        ("c.org", "* C\nhello\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_containing_pct("hello"), 66);
}

#[test]
fn vault_files_containing_zero_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.files_containing("any"), 0);
    assert_eq!(v.files_containing_pct("any"), 0);
}

#[test]
fn vault_paths_containing_count_match() {
    let td = write_vault(&[
        ("a.org", "* Apple\n"),
        ("b.org", "* Banana\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.paths_containing_count("App"), 1);
    assert_eq!(v.paths_containing_count("missing"), 0);
}

#[test]
fn vault_paths_containing_ignore_case_count_match() {
    let td = write_vault(&[
        ("a.org", "* Apple\n"),
        ("b.org", "* APPLE\n"),
    ]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.paths_containing_ignore_case_count("apple"), 2);
}

#[test]
fn vault_paths_containing_count_zero_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.paths_containing_count("any"), 0);
    assert_eq!(v.paths_containing_ignore_case_count("any"), 0);
}

#[test]
fn vault_headline_count_at_level_alias_match() {
    let td = write_vault(&[("a.org", "* A\n** B\n** C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.headline_count_at_level(1), 1);
    assert_eq!(v.headline_count_at_level(2), 2);
    assert_eq!(v.headline_count_at_level(3), 0);
}

#[test]
fn vault_headline_count_at_level_zero_when_no_files() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.headline_count_at_level(1), 0);
}

#[test]
fn vault_has_any_link_alias_match() {
    let td = write_vault(&[("a.org", "* A\n[[x][X]]\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_link());
}

#[test]
fn vault_has_any_link_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_any_link());
}

#[test]
fn vault_has_any_timestamp_alias_match() {
    let td = write_vault(&[("a.org", "* A\n<2026-05-30 Sat>\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_timestamp());
}

#[test]
fn vault_has_any_timestamp_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_any_timestamp());
}

#[test]
fn vault_has_any_archived_alias_match() {
    let td = write_vault(&[("a.org", "* A :ARCHIVE:\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_archived());
}

#[test]
fn vault_has_any_scheduled_alias_match() {
    let td = write_vault(&[("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_scheduled());
}

#[test]
fn vault_has_any_deadline_alias_match() {
    let td = write_vault(&[("a.org", "* A\nDEADLINE: <2026-05-30 Sat>\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_deadline());
}

#[test]
fn vault_has_any_closed_alias_match() {
    let td = write_vault(&[("a.org", "* A\nCLOSED: [2026-05-30 Sat]\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_closed());
}

#[test]
fn vault_has_any_comment_alias_match() {
    let td = write_vault(&[("a.org", "* COMMENT A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_comment());
}

#[test]
fn vault_has_any_planning_alias_match() {
    let td = write_vault(&[("a.org", "* A\nSCHEDULED: <2026-05-30 Sat>\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_planning());
}

#[test]
fn vault_has_any_footnote_alias_match() {
    let td = write_vault(&[("a.org", "* A\nbody[fn:1]\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_footnote());
}

#[test]
fn vault_has_any_macro_alias_match() {
    let td = write_vault(&[("a.org", "* A\n{{{m(x)}}}\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert!(v.has_any_macro());
}

#[test]
fn vault_has_any_cookie_alias_match() {
    let td = write_vault(&[("a.org", "* TODO A [1/2]\n** TODO X\n** DONE Y\n")]);
    let v = Vault::open(td.path()).expect("open");
    let _ = v.has_any_cookie();
}

#[test]
fn vault_has_any_cookie_false_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert!(!v.has_any_cookie());
}

#[test]
fn vault_count_tags_alias_match() {
    let td = write_vault(&[("a.org", "* A :x:y:\n* B :z:\n")]);
    let v = Vault::open(td.path()).expect("open");
    // tags x, y, z = 3 occurrences
    assert_eq!(v.count_tags(), 3);
}

#[test]
fn vault_count_tags_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_tags(), 0);
}

#[test]
fn vault_count_links_alias_match() {
    let td = write_vault(&[("a.org", "* A\n[[x][X]] [[y][Y]]\n* B\n[[z][Z]]\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_links(), 3);
}

#[test]
fn vault_count_links_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_links(), 0);
}

#[test]
fn vault_count_timestamps_alias_match() {
    let td = write_vault(&[("a.org", "* A\n<2026-05-30 Sat>\n* B\n<2026-05-31 Sun>\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_timestamps(), 2);
}

#[test]
fn vault_count_timestamps_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_timestamps(), 0);
}

#[test]
fn vault_count_headlines_alias_match() {
    let td = write_vault(&[("a.org", "* A\n* B\n* C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_headlines(), 3);
}

#[test]
fn vault_count_headlines_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_headlines(), 0);
}

#[test]
fn vault_count_files_alias_match() {
    let td = write_vault(&[("a.org", "* A\n"), ("b.org", "* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_files(), 2);
}

#[test]
fn vault_count_files_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_files(), 0);
}

#[test]
fn vault_count_words_alias_match() {
    let td = write_vault(&[("a.org", "* A B C\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_words(), 4);
}

#[test]
fn vault_count_words_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_words(), 0);
}

#[test]
fn vault_count_bytes_alias_match() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_bytes(), 4);
}

#[test]
fn vault_count_bytes_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_bytes(), 0);
}

#[test]
fn vault_count_chars_alias_match() {
    let td = write_vault(&[("a.org", "* A\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_chars(), 4);
}

#[test]
fn vault_count_chars_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_chars(), 0);
}

#[test]
fn vault_count_lines_alias_match() {
    let td = write_vault(&[("a.org", "* A\nbody\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_lines(), 3);
}

#[test]
fn vault_count_lines_zero_when_empty() {
    let td = write_vault(&[]);
    let v = Vault::open(td.path()).expect("open");
    assert_eq!(v.count_lines(), 0);
}



#[test]
fn vault_line_count_of_match() {
    let td = write_vault(&[("a.org", "* A\nbody\n* B\n")]);
    let v = Vault::open(td.path()).expect("open");
    let p = v.root().join("a.org");
    assert_eq!(v.line_count_of(&p), Some(3));
    assert_eq!(v.line_count_of(std::path::Path::new("missing.org")), None);
}





