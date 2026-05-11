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
