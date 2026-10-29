use std::path::{Path, PathBuf};

use turbogit_domain::model::{Change, ChangeStatus, RootId, Shelf, ShelfSource};
use turbogit_services::shelve_stash::{self, shelf_path};

#[derive(serde::Serialize)]
struct LegacyShelf {
    name: String,
    changes: Vec<Change>,
    created_at: chrono::DateTime<chrono::Utc>,
}

fn change(path: &str) -> Change {
    Change {
        path: PathBuf::from(path),
        status: ChangeStatus::Modified,
        chunks: Vec::new(),
        staged: false,
        unstaged: false,
        orig_path: None,
    }
}

fn write_legacy_store(project_dir: &Path, names: &[&str]) {
    let legacy: Vec<LegacyShelf> = names
        .iter()
        .map(|name| LegacyShelf {
            name: (*name).to_string(),
            changes: vec![change("src/main.rs")],
            created_at: chrono::DateTime::parse_from_rfc3339("2026-03-01T09:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        })
        .collect();
    let text = ron::ser::to_string_pretty(&legacy, ron::ser::PrettyConfig::new()).unwrap();
    std::fs::create_dir_all(project_dir.join(".turbogit")).unwrap();
    std::fs::write(shelf_path(project_dir), text).unwrap();
}

#[test]
fn a_legacy_shelf_row_without_a_root_loads_bound_to_the_project_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    write_legacy_store(&project_dir, &["wip: parser", "wip: cli"]);

    let shelves = shelve_stash::load_shelves(&project_dir)
        .expect("a legacy shelf store must load, not fail the launch");

    let names: Vec<&str> = shelves.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["wip: parser", "wip: cli"]);
    assert_eq!(shelves[0].changes.len(), 1);
    assert_eq!(shelves[0].changes[0].path, PathBuf::from("src/main.rs"));
    for shelf in &shelves {
        assert_eq!(
            shelf.root.as_path(),
            project_dir.as_path(),
            "a legacy row carries no root, so it must bind to the project's own root"
        );
    }
}

#[test]
fn a_write_after_an_unreadable_load_leaves_the_unreadable_records_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(project_dir.join(".turbogit")).unwrap();
    let unreadable = "[this is not RON";
    std::fs::write(shelf_path(&project_dir), unreadable).unwrap();

    let load = shelve_stash::load_shelves(&project_dir);
    assert!(
        load.is_err(),
        "an unparseable store reports itself rather than painting an empty pane"
    );

    let written = shelve_stash::save_shelves(&project_dir, &[] as &[Shelf]);
    assert!(
        written.is_err(),
        "save must refuse: its slice does not contain the records on disk"
    );

    assert_eq!(
        std::fs::read_to_string(shelf_path(&project_dir)).unwrap(),
        unreadable,
        "the unreadable records survived"
    );
}

#[test]
fn an_empty_store_is_an_empty_shelf_not_an_unreadable_one() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(project_dir.join(".turbogit")).unwrap();
    std::fs::write(shelf_path(&project_dir), "").unwrap();

    assert!(
        shelve_stash::load_shelves(&project_dir)
            .expect("a blank store reads as an empty shelf")
            .is_empty()
    );

    let root = RootId(std::sync::Arc::from(project_dir.as_path()));
    let shelf = shelve_stash::make_shelf("wip: parser", &[], root);
    shelve_stash::save_shelves(&project_dir, std::slice::from_ref(&shelf))
        .expect("a blank store holds no records, so writing it is not a loss");
    assert_eq!(
        shelve_stash::load_shelves(&project_dir)
            .unwrap()
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["wip: parser"]
    );
}

#[test]
fn a_create_after_an_unreadable_store_refuses_and_leaves_the_bytes_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(project_dir.join(".turbogit")).unwrap();
    let unreadable = r#"[{ name: "wip: parser", changes: [], created_at: "#;
    std::fs::write(shelf_path(&project_dir), unreadable).unwrap();

    let root = RootId(std::sync::Arc::from(project_dir.as_path()));
    let created = shelve_stash::make_shelf("wip: cli", &[], root);
    let refused = shelve_stash::save_shelves(&project_dir, std::slice::from_ref(&created))
        .expect_err("a create cannot carry the rows on disk, so it must not write them away");
    assert!(
        refused
            .to_string()
            .contains("Refused to rewrite the shelf store"),
        "the refusal names itself: {refused}"
    );
    assert_eq!(
        std::fs::read_to_string(shelf_path(&project_dir)).unwrap(),
        unreadable,
        "a genuinely corrupt file is preserved, not overwritten"
    );
}

#[test]
fn a_parseable_but_wrongly_shaped_store_is_refused_too() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(project_dir.join(".turbogit")).unwrap();
    std::fs::write(shelf_path(&project_dir), "\"not a shelf list\"").unwrap();

    assert!(shelve_stash::load_shelves(&project_dir).is_err());
    shelve_stash::save_shelves(
        &project_dir,
        &[shelve_stash::make_shelf(
            "wip: cli",
            &[],
            RootId(std::sync::Arc::from(project_dir.as_path())),
        )],
    )
    .expect_err("a file whose shape this build cannot read stays out of reach of a write");
    assert_eq!(
        std::fs::read_to_string(shelf_path(&project_dir)).unwrap(),
        "\"not a shelf list\""
    );
}

/// * **an `Option` on the wire**, [`Change::orig_path`] (a rename) is the shelf's
///   only optional field, and RON 0.12 deserialises an `Option` solely from its
///   `Some(..)` spelling. The store writes what it reads only if the serializer
///   emits that spelling, so a rename-bearing shelf is the case that proves it:
///   were the serializer writing a plain value, this round trip would fail and the
///   follow-up write would be refused forever.
#[test]
fn a_store_this_build_wrote_reads_back_and_stays_writable() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    let root = RootId(std::sync::Arc::from(project_dir.as_path()));

    let mut renamed = change("src/new.rs");
    renamed.orig_path = Some(PathBuf::from("src/old.rs"));
    renamed.staged = true;
    renamed.unstaged = true;
    let first =
        shelve_stash::make_shelf("wip: rename", std::slice::from_ref(&renamed), root.clone());

    shelve_stash::save_shelves(&project_dir, std::slice::from_ref(&first)).unwrap();
    let loaded = shelve_stash::load_shelves(&project_dir)
        .expect("the store this build wrote must read back into this build");
    assert_eq!(
        loaded[0].changes[0].orig_path.as_deref(),
        Some(Path::new("src/old.rs"))
    );
    assert_eq!(loaded[0].changes[0].path, PathBuf::from("src/new.rs"));
    assert!(loaded[0].changes[0].staged && loaded[0].changes[0].unstaged);
    assert_eq!(
        loaded[0].root, root,
        "the root this build wrote is the root it reads"
    );

    let second = shelve_stash::make_shelf("wip: cli", &[], root);
    shelve_stash::save_shelves(&project_dir, &[first, second]).unwrap();
    assert_eq!(
        shelve_stash::load_shelves(&project_dir)
            .expect("a store this build wrote never locks shelving out")
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["wip: rename", "wip: cli"]
    );
}

#[test]
fn rewriting_a_legacy_row_does_not_give_the_store_a_root_it_never_had() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    write_legacy_store(&project_dir, &["wip: parser", "wip: cli"]);

    let loaded = shelve_stash::load_shelves(&project_dir).unwrap();
    for shelf in &loaded {
        assert_eq!(
            shelf.root.as_path(),
            project_dir.as_path(),
            "the row is bound in-session, which is what makes it usable"
        );
    }

    shelve_stash::save_shelves(&project_dir, &loaded).unwrap();
    let text = std::fs::read_to_string(shelf_path(&project_dir)).unwrap();
    assert!(
        !text.contains("root:"),
        "the bound path must not be persisted as a recorded root:\n{text}"
    );

    let again = shelve_stash::load_shelves(&project_dir).unwrap();
    assert_eq!(
        again.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        vec!["wip: parser", "wip: cli"]
    );
    assert_eq!(again[0].root.as_path(), project_dir.as_path());
    assert_eq!(again[0].changes.len(), 1);
}

#[derive(serde::Deserialize)]
struct WireRow {
    name: String,
    #[serde(default)]
    root: String,
}

fn wire_rows(project_dir: &Path) -> Vec<(String, Option<PathBuf>)> {
    let text = std::fs::read_to_string(shelf_path(project_dir)).unwrap();
    ron::from_str::<Vec<WireRow>>(&text)
        .unwrap_or_else(|e| panic!("the store this build wrote must parse: {e}\n{text}"))
        .into_iter()
        .map(|row| {
            let root = (!row.root.is_empty()).then(|| PathBuf::from(row.root));
            (row.name, root)
        })
        .collect()
}

#[test]
fn a_legacy_row_stays_rootless_while_the_list_is_mutated_around_it() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    write_legacy_store(&project_dir, &["wip: parser", "wip: cli"]);

    let mut shelves = shelve_stash::load_shelves(&project_dir).unwrap();

    let other = project_dir.join("sibling");
    let other_root = RootId(std::sync::Arc::from(other.as_path()));
    shelves.push(shelve_stash::make_shelf(
        "wip: notes",
        &[],
        other_root.clone(),
    ));
    shelve_stash::save_shelves(&project_dir, &shelves).unwrap();

    let rows = wire_rows(&project_dir);
    assert_eq!(
        rows.iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["wip: parser", "wip: cli", "wip: notes"],
        "the add wrote all three rows"
    );
    assert_eq!(
        rows.iter()
            .filter(|(_, root)| root.is_none())
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["wip: parser", "wip: cli"],
        "the neighbour rows the mutation never touched are still rootless"
    );
    assert_eq!(
        rows.iter()
            .find(|(name, _)| name == "wip: notes")
            .and_then(|(_, root)| root.as_deref()),
        Some(other.as_path()),
        "a real root is written, not dropped with the legacy key"
    );

    shelves.retain(|s| s.name != "wip: cli");
    shelve_stash::save_shelves(&project_dir, &shelves).unwrap();
    let rows = wire_rows(&project_dir);
    assert_eq!(
        rows.iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["wip: parser", "wip: notes"],
        "the remove took exactly the one row it named"
    );
    assert_eq!(
        rows.iter()
            .filter(|(_, root)| root.is_none())
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["wip: parser"],
        "and the surviving legacy row is still rootless"
    );

    let again = shelve_stash::load_shelves(&project_dir).unwrap();
    let bound: Vec<(&str, PathBuf)> = again
        .iter()
        .map(|s| (s.name.as_str(), s.root.as_path().to_path_buf()))
        .collect();
    assert_eq!(
        bound,
        vec![
            ("wip: parser", project_dir.clone()),
            ("wip: notes", other.clone()),
        ],
        "the legacy row re-binds to the project dir and the recorded row keeps its own root"
    );
    assert_eq!(
        again.iter().find(|s| s.name == "wip: notes").unwrap().root,
        other_root,
        "the recorded root round-trips as the same `RootId` value"
    );
}

#[test]
fn a_root_equal_to_the_project_dir_omits_and_rebinds_as_the_same_value() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    let project_root = RootId(std::sync::Arc::from(project_dir.as_path()));
    let nested_dir = project_dir.join("nested");
    let nested_root = RootId(std::sync::Arc::from(nested_dir.as_path()));

    let at_project = shelve_stash::make_shelf("wip: project root", &[], project_root.clone());
    let at_nested = shelve_stash::make_shelf("wip: nested root", &[], nested_root.clone());
    shelve_stash::save_shelves(&project_dir, &[at_project, at_nested.clone()]).unwrap();

    let rows = wire_rows(&project_dir);
    assert_eq!(
        rows.iter()
            .map(|(name, root)| (name.as_str(), root.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("wip: project root", None),
            ("wip: nested root", Some(nested_dir.as_path())),
        ],
        "only the row rooted at the store's own directory is written rootless; a \
         second, genuine root of a multi-root project is recorded"
    );

    let loaded = shelve_stash::load_shelves(&project_dir).unwrap();
    assert_eq!(
        loaded
            .iter()
            .map(|s| s.root.clone())
            .collect::<Vec<RootId>>(),
        vec![project_root, nested_root],
        "every row comes back with the ROOT it was written with — identity, not a re-binding"
    );
    assert_eq!(
        loaded.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        vec!["wip: project root", "wip: nested root"],
        "and no row was lost to the omission"
    );
}

fn source_keys(project_dir: &Path) -> Vec<(String, Option<String>)> {
    let text = std::fs::read_to_string(shelf_path(project_dir)).unwrap();
    let value: ron::Value = ron::from_str(&text)
        .unwrap_or_else(|e| panic!("the store this build wrote must parse: {e}\n{text}"));
    let rows = match value {
        ron::Value::Seq(rows) => rows,
        other => panic!("the shelf store is a list of rows, got: {other:?}"),
    };
    rows.iter()
        .map(|row| {
            let ron::Value::Map(fields) = row else {
                panic!("each row is a map, got: {row:?}");
            };
            let name = match fields.get(&ron::Value::String("name".to_string())) {
                Some(ron::Value::String(name)) => name.clone(),
                other => panic!("a row's name is a string, got: {other:?}"),
            };
            let file = fields
                .get(&ron::Value::String("source".to_string()))
                .map(payload_file);
            (name, file)
        })
        .collect()
}

fn payload_file(value: &ron::Value) -> String {
    match value {
        ron::Value::Map(fields) => match fields.get(&ron::Value::String("file".to_string())) {
            Some(ron::Value::String(file)) => file.clone(),
            other => panic!("a recorded `source` must name its payload file, got: {other:?}"),
        },
        other => panic!("unexpected `source` spelling on the wire: {other:?}"),
    }
}

#[test]
fn an_imported_patch_row_survives_the_store_as_a_patch_row_with_its_file() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    let root = RootId(std::sync::Arc::from(project_dir.join("repo").as_path()));
    let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-02T10:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let imported = shelve_stash::make_patch_shelf(
        "imported: api",
        std::slice::from_ref(&change("src/api.rs")),
        root.clone(),
        created_at,
        "imported-api-1775.patch",
    );
    let shelved = shelve_stash::make_shelf("wip: parser", &[], root.clone());
    shelve_stash::save_shelves(&project_dir, &[imported, shelved]).unwrap();

    let loaded = shelve_stash::load_shelves(&project_dir).unwrap();
    assert_eq!(
        loaded[0].source,
        ShelfSource::Patch {
            file: "imported-api-1775.patch".to_string(),
        },
        "the variant and the payload file it addresses both come back: {:?}",
        loaded[0].source
    );
    assert_eq!(
        loaded[1].source,
        ShelfSource::Stash,
        "and a row shelved through git is still a stash row"
    );
    assert_eq!(
        loaded[0].changes.len(),
        1,
        "the imported row's own file list rode along"
    );

    assert_eq!(
        source_keys(&project_dir),
        vec![
            (
                "imported: api".to_string(),
                Some("imported-api-1775.patch".to_string())
            ),
            ("wip: parser".to_string(), None),
        ],
        "the patch row records where its bytes are; the stash row is given no key it does \
         not need"
    );
    let text = std::fs::read_to_string(shelf_path(&project_dir)).unwrap();
    assert_eq!(
        text.matches("Patch(").count(),
        1,
        "the variant is written by name, once, over the row that holds a patch:\n{text}"
    );
    assert!(
        !text.contains("Stash"),
        "a stash row spells its mechanism by carrying no key, not by naming it:\n{text}"
    );

    let mut shelves = loaded.clone();
    shelves.push(shelve_stash::make_shelf(
        "wip: cli",
        &[],
        RootId(std::sync::Arc::from(project_dir.join("other").as_path())),
    ));
    shelve_stash::save_shelves(&project_dir, &shelves).unwrap();

    let again = shelve_stash::load_shelves(&project_dir).unwrap();
    assert_eq!(
        again
            .iter()
            .map(|s| s.source.clone())
            .collect::<Vec<ShelfSource>>(),
        vec![
            ShelfSource::Patch {
                file: "imported-api-1775.patch".to_string()
            },
            ShelfSource::Stash,
            ShelfSource::Stash,
        ],
        "a rewrite that adds a row degrades no row it did not touch"
    );
    assert_eq!(
        source_keys(&project_dir)
            .into_iter()
            .map(|(name, file)| (name, file.is_some()))
            .collect::<Vec<_>>(),
        vec![
            ("imported: api".to_string(), true),
            ("wip: parser".to_string(), false),
            ("wip: cli".to_string(), false),
        ],
        "and the key stays exactly where it was written — one row's own fact, never a \
         default spelled out for the others"
    );
    assert_eq!(
        std::fs::read_to_string(shelf_path(&project_dir))
            .unwrap()
            .matches("Patch(")
            .count(),
        1,
        "one tag for one patch row, after the rewrite"
    );
}

#[test]
fn a_store_of_stash_rows_gains_no_source_key_when_it_is_rewritten() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path().to_path_buf();
    let root = RootId(std::sync::Arc::from(project_dir.join("repo").as_path()));
    let nested = RootId(std::sync::Arc::from(
        project_dir.join("repo/nested").as_path(),
    ));
    shelve_stash::save_shelves(
        &project_dir,
        &[
            shelve_stash::make_shelf("wip: parser", &[], root),
            shelve_stash::make_shelf("wip: cli", &[], nested),
        ],
    )
    .unwrap();

    assert_eq!(
        source_keys(&project_dir),
        vec![
            ("wip: parser".to_string(), None),
            ("wip: cli".to_string(), None),
        ],
        "a stash row is written back as the keyless row it was read as"
    );

    let loaded = shelve_stash::load_shelves(&project_dir).unwrap();
    shelve_stash::save_shelves(&project_dir, &loaded).unwrap();
    let text = std::fs::read_to_string(shelf_path(&project_dir)).unwrap();
    assert!(
        !text.contains("source"),
        "a rewrite of a keyless store invents nothing:\n{text}"
    );
    assert_eq!(
        loaded.into_iter().map(|s| s.name).collect::<Vec<String>>(),
        vec!["wip: parser", "wip: cli"],
        "and both rows are still there afterwards"
    );
}
