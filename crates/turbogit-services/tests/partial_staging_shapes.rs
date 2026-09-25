//! Issue 11 (deepen-git-engine) — every staging shape, composed from the value.
//!
//! **Partial staging** now selects, composes and applies from the patch value
//! rather than from diff text, so a hunk or line the user can see is exactly a
//! hunk or line the app can stage. These need no `git` binary: the input is a
//! patch, the selection is a set of hunk and line positions, and every expected
//! value below is a hand-written literal — the guarantee `ADR-0013` protected
//! (that a composed selection is appliable) is asserted on the composed bytes
//! themselves, and the engine's own apply answer is the last word.

use turbogit_domain::model::Patch;
use turbogit_engine::patch::parse_patch;
use turbogit_services::partial::{HunkSelection, Selection, compose};

/// A hunk with no context lines at all: the whole hunk is the change.
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

/// Compose from git's syntax and read the answer back as the bytes `git apply`
/// will be fed — which is what `ADR-0013` guarantees about them.
fn compose_from(text: &str, selection: &Selection) -> String {
    compose(&parse_patch(text), selection).to_string()
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
    // Changed lines: 0=`-one`, 1=`+ONE`, 2=`-two`, 3=`+TWO`. Staging ord 0
    // alone keeps the first deletion; `+ONE` and `+TWO` are unselected
    // additions and drop out entirely, while `-two` survives on both sides and
    // is re-emitted as context. The old side still spans 4 lines, the new side
    // now 3 — the header has to say so, because `git apply` trusts it.
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
    // The second hunk alone: the rename metadata is part of *which file* the
    // patch touches, so it survives the selection untouched.
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
    // No hunks, so no position is selectable — and the composed patch is empty,
    // which every staging verb treats as a no-op before the engine is touched.
    let patch: Patch = parse_patch(BINARY_CHANGE);
    assert!(patch.hunk_count() == 0, "a binary section has no hunks");
    for selection in [
        selected(0, HunkSelection::Whole),
        selected(0, HunkSelection::Lines([0, 1].into_iter().collect())),
    ] {
        assert_eq!(
            compose(&patch, &selection).to_string(),
            "",
            "a whole-**Binary change** file stages as a file, never as a hunk"
        );
    }
}

#[test]
fn an_empty_selection_composes_nothing_at_all() {
    assert_eq!(
        compose(&parse_patch(ONE_HUNK_THREE_LINES), &Selection::default(),).to_string(),
        ""
    );
}
