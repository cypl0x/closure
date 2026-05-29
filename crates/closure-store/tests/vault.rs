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
