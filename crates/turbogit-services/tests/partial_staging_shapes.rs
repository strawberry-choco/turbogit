use turbogit_domain::model::Patch;
use turbogit_engine_api::patch::parse_patch;
use turbogit_services::partial::{HunkSelection, Selection, compose};

const NO_CONTEXT: &str = "diff --git a/tight.txt b/tight.txt\n\
     --- a/tight.txt\n\
     +++ b/tight.txt\n\
     @@ -1 +1 @@\n\
     -before\n\
     +after\n";

const ONE_HUNK_THREE_LINES: &str = "diff --git a/words.txt b/words.txt\n\
     index 2d9efe4..0a173d3 100644\n\
     --- a/words.txt\n\
     +++ b/words.txt\n\
     @@ -1,4 +1,4 @@\n\
      head\n\
     -one\n\
     +ONE\n\
     -two\n\
     +TWO\n\
      tail\n";

const RENAMED_WITH_EDIT: &str = "diff --git a/src/old.rs b/src/new.rs\n\
     index aaa..bbb 100644\n\
     similarity index 92%\n\
     rename from src/old.rs\n\
     rename to src/new.rs\n\
     --- a/src/old.rs\n\
     +++ b/src/new.rs\n\
     @@ -1,2 +1,2 @@\n\
     -a\n\
     +b\n\
      keep\n\
     @@ -20,2 +20,2 @@\n\
     -c\n\
     +d\n\
      keep2\n";

const BINARY_CHANGE: &str = "diff --git a/logo.png b/logo.png\n\
     index aaa..bbb 100644\n\
     Binary files a/logo.png and b/logo.png differ\n";

fn selected(hunk: usize, selection: HunkSelection) -> Selection {
    Selection {
        hunks: [(hunk, selection)].into_iter().collect(),
    }
}

fn compose_from(text: &str, selection: &Selection) -> String {
    compose(
        &parse_patch(text),
        selection,
        turbogit_engine_api::ApplyDirection::Forward,
    )
    .to_string()
}

#[test]
fn a_hunk_with_no_context_survives_whole_byte_for_byte() {
    assert_eq!(
        compose_from(NO_CONTEXT, &selected(0, HunkSelection::Whole)),
        NO_CONTEXT,
        "composing a whole selection must not disturb the patch git produced"
    );
}

#[test]
fn a_selection_narrowed_to_one_line_rewrites_only_the_counts() {
    assert_eq!(
        compose_from(
            ONE_HUNK_THREE_LINES,
            &selected(0, HunkSelection::Lines([0].into_iter().collect()))
        ),
        "diff --git a/words.txt b/words.txt\nindex 2d9efe4..0a173d3 100644\n\
         --- a/words.txt\n+++ b/words.txt\n@@ -1,4 +1,3 @@\n\
         \x20head\n-one\n\x20two\n\x20tail\n",
        "an unselected deletion demotes to context and an unselected addition drops"
    );
}

#[test]
fn a_rename_keeps_its_section_headers_and_loses_only_unselected_hunks() {
    assert_eq!(
        compose_from(RENAMED_WITH_EDIT, &selected(1, HunkSelection::Whole)),
        "diff --git a/src/old.rs b/src/new.rs\nindex aaa..bbb 100644\n\
         similarity index 92%\nrename from src/old.rs\nrename to src/new.rs\n\
         --- a/src/old.rs\n+++ b/src/new.rs\n@@ -20,2 +20,2 @@\n\
         -c\n+d\n\x20keep2\n"
    );
}

#[test]
fn a_binary_change_is_never_a_partial_staging_target() {
    let patch: Patch = parse_patch(BINARY_CHANGE);
    assert!(patch.hunk_count() == 0, "a binary section has no hunks");
    for selection in [
        selected(0, HunkSelection::Whole),
        selected(0, HunkSelection::Lines([0, 1].into_iter().collect())),
    ] {
        assert_eq!(
            compose(
                &patch,
                &selection,
                turbogit_engine_api::ApplyDirection::Forward,
            )
            .to_string(),
            "",
            "a whole-**Binary change** file stages as a file, never as a hunk"
        );
    }
}

#[test]
fn an_empty_selection_composes_nothing_at_all() {
    assert_eq!(
        compose(
            &parse_patch(ONE_HUNK_THREE_LINES),
            &Selection::default(),
            turbogit_engine_api::ApplyDirection::Forward,
        )
        .to_string(),
        ""
    );
}
