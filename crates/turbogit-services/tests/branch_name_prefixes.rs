use chrono::{TimeZone, Utc};
use turbogit_domain::model::{Branch, BranchKind};
use turbogit_services::branch_names::{Prefix, completing};

fn row(name: &str, kind: BranchKind, remote: Option<&str>) -> Branch {
    Branch {
        name: name.to_owned(),
        kind,
        tracking: None,
        favorite: false,
        protected: false,
        exists: true,
        ahead: 0,
        behind: 0,
        gone: false,
        last_touched: None,
        tip: None,
        remote: remote.map(str::to_owned),
    }
}

fn local(name: &str) -> Branch {
    row(name, BranchKind::Local, None)
}

fn remote_row(remote: &str, name: &str) -> Branch {
    row(name, BranchKind::Remote, Some(remote))
}

fn texts(rows: &[Prefix]) -> Vec<&str> {
    rows.iter().map(|p| p.text.as_str()).collect()
}

fn family(rows: &[Prefix], text: &str) -> usize {
    rows.iter()
        .find(|p| p.text == text)
        .unwrap_or_else(|| panic!("{text} was not offered, of {:?}", texts(rows)))
        .branches
}

/// The seeded set the reference's sentence describes: two families in use, one
/// deeper family, one flat name, and a remote row that is not a family at all.
fn seeded() -> Vec<Branch> {
    vec![
        local("main"),
        local("feat/blame-window"),
        local("feat/build-cache"),
        local("feature/login"),
        local("release/2.4"),
        local("users/kye/scratch"),
        local("zeta"),
        remote_row("origin", "release/2.4"),
    ]
}

#[test]
fn every_offered_row_ends_with_a_slash() {
    for b in completing(&seeded(), "", 20) {
        assert!(b.text.ends_with('/'), "{} does not", b.text);
    }
}

#[test]
fn one_character_offers_every_family_that_extends_it() {
    assert_eq!(texts(&completing(&seeded(), "f", 6)), ["feat/", "feature/"]);
}

#[test]
fn the_count_says_how_many_branches_live_in_the_family() {
    let rows = completing(&seeded(), "f", 6);
    assert_eq!(family(&rows, "feat/"), 2);
    assert_eq!(family(&rows, "feature/"), 1);
}

#[test]
fn shallower_families_come_before_deeper_ones() {
    assert_eq!(
        texts(&completing(&seeded(), "users", 6)),
        ["users/", "users/kye/"]
    );
}

#[test]
fn a_bigger_family_wins_the_tie_at_the_same_depth() {
    let branches = vec![
        local("feat/a"),
        local("feature/b"),
        local("feature/c"),
        local("feature/d"),
    ];
    assert_eq!(texts(&completing(&branches, "f", 6)), ["feature/", "feat/"]);
}

#[test]
fn a_flat_name_offers_nothing() {
    assert!(
        texts(&completing(&seeded(), "zet", 6)).is_empty(),
        "`zeta` has no leading path to offer"
    );
}

#[test]
fn a_row_that_is_not_strictly_longer_than_the_text_is_not_offered() {
    assert!(
        texts(&completing(&seeded(), "feat/", 6)).is_empty(),
        "a row equal to the text would complete nothing"
    );
    assert_eq!(
        texts(&completing(&seeded(), "users/ky", 6)),
        ["users/kye/"],
        "the already-typed `users/` does not come back"
    );
}

#[test]
fn no_row_is_a_complete_branch_name() {
    let all = completing(&seeded(), "", 50);
    let rows = texts(&all);
    for name in ["feat/build-cache", "feat/blame-window", "feature/login"] {
        assert!(!rows.contains(&name), "{name} was offered as a row");
    }
}

#[test]
fn remote_rows_contribute_no_family() {
    let branches = vec![local("main"), remote_row("origin", "release/2.4")];
    assert!(
        texts(&completing(&branches, "release", 6)).is_empty(),
        "the remote row's own remote is not a naming convention"
    );
}

#[test]
fn a_local_branch_never_opens_a_family_named_for_a_remote_the_repo_has() {
    let branches = vec![
        local("origin/experiment"),
        remote_row("origin", "main"),
        local("upstream/work"),
    ];
    assert!(
        texts(&completing(&branches, "origin/", 6)).is_empty(),
        "`git branch <new> origin/x` is the ambiguous refname"
    );
    assert_eq!(texts(&completing(&branches, "upstr", 6)), ["upstream/"]);
}

#[test]
fn a_family_named_like_a_remote_is_offered_when_no_such_remote_exists() {
    let branches = vec![local("origin/experiment")];
    assert_eq!(texts(&completing(&branches, "orig", 6)), ["origin/"]);
}

#[test]
fn matching_ignores_case_and_the_row_keeps_the_repositorys_spelling() {
    let branches = vec![local("Feat/A"), local("feat/B")];
    let rows = completing(&branches, "feat", 6);
    assert_eq!(
        texts(&rows),
        ["Feat/", "feat/"],
        "two spellings, two refs, two rows, ordered by exact text"
    );
    assert_eq!(family(&rows, "Feat/"), 1);
    assert_eq!(family(&rows, "feat/"), 1);
}

#[test]
fn the_limit_caps_the_rows_and_the_order_is_the_rank() {
    let branches: Vec<Branch> = (1..=9).map(|i| local(&format!("a{i}/x"))).collect();
    let rows = completing(&branches, "a", 6);
    assert_eq!(rows.len(), 6);
    assert_eq!(
        texts(&rows),
        ["a1/", "a2/", "a3/", "a4/", "a5/", "a6/"],
        "and the cap takes from the head of the rank, not from the tail"
    );
}

#[test]
fn ranking_reads_names_only_so_a_touched_date_cannot_reorder_them() {
    let newer = local("amber/x");
    let mut older = local("alpha/x");
    older.last_touched = Some(Utc.timestamp_opt(1_700_000_000, 0).unwrap());
    for branches in [vec![newer.clone(), older.clone()], vec![older, newer]] {
        assert_eq!(
            texts(&completing(&branches, "a", 6)),
            ["alpha/", "amber/"],
            "a row's recency is not a sort key, so no branch set can move it"
        );
    }
}
