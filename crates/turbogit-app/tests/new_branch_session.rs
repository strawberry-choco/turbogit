use std::path::PathBuf;
use turbogit_app::new_branch::{
    DialogReply, NameFrame, NameKeys, NewBranchSession, SuggestState, advance, show_list,
};
use turbogit_app::state::NewBranchBase;
use turbogit_domain::model::{BranchSpec, RootId};
use turbogit_services::branch_names::Prefix;

fn family(text: &str, branches: usize) -> Prefix {
    Prefix {
        text: text.into(),
        branches,
    }
}

/// The rows `completing` answers for `f` in a repository holding two `feat/`
/// branches, one `feature/` branch and three under `release/`.
fn rows() -> Vec<Prefix> {
    vec![
        family("feat/", 2),
        family("feature/", 1),
        family("release/", 3),
    ]
}

/// The rows the same repository answers for one more character typed.
fn rows_for(typed: &str) -> Vec<Prefix> {
    rows()
        .into_iter()
        .filter(|p| p.text.chars().count() > typed.chars().count() && p.text.starts_with(typed))
        .collect()
}

fn session() -> NewBranchSession {
    NewBranchSession::open(
        1,
        Some(RootId(PathBuf::from("/repo").into())),
        Some("main".into()),
        NewBranchBase::Head,
        true,
    )
}

/// A session mid-frame: the kit field has already written the reader's typing
/// into the buffer, which is the only copy in the process.
fn typing(text: &str) -> NewBranchSession {
    let mut held = session();
    held.name = text.to_owned();
    held
}

fn frame<'a>(live: Option<&'a str>, rows: &'a [Prefix]) -> NameFrame<'a> {
    NameFrame {
        live_text: live,
        candidates: rows,
        keys: NameKeys::default(),
        create_clicked: false,
        cancel_clicked: false,
        clicked_candidate: None,
    }
}

fn with_keys<'a>(live: Option<&'a str>, rows: &'a [Prefix], keys: NameKeys) -> NameFrame<'a> {
    NameFrame {
        keys,
        ..frame(live, rows)
    }
}

fn pressed(keys: impl FnOnce(&mut NameKeys)) -> NameKeys {
    let mut out = NameKeys::default();
    keys(&mut out);
    out
}

#[test]
fn open_replaces_the_whole_session_so_an_invocation_inherits_nothing() {
    let mut held = session();
    held.name = "half-typed-name".into();
    held.checkout = false;
    held.start = NewBranchBase::Branch(BranchSpec::local("release/2.4"));
    held.suggest = SuggestState {
        armed: Some("feat/".into()),
        folded_at: Some("fe".into()),
    };

    let fresh = NewBranchSession::open(
        held.generation + 1,
        Some(RootId(PathBuf::from("/repo").into())),
        Some("main".into()),
        NewBranchBase::Head,
        true,
    );
    assert_eq!(fresh.name, "main", "the buffer is this call's pre-fill");
    assert!(fresh.checkout);
    assert_eq!(fresh.start, NewBranchBase::Head);
    assert_eq!(fresh.suggest, SuggestState::default());
    assert_ne!(fresh, held, "and every field of the old one is gone");
}

#[test]
fn opening_twice_converges_on_one_state_apart_from_the_salt() {
    let base = session();
    let mut drifted = base.clone();
    drifted.name = "typed-over".into();
    drifted.suggest.armed = Some("feat/".into());

    let reopened = NewBranchSession::open(
        drifted.generation + 1,
        base.root.clone(),
        Some("main".into()),
        NewBranchBase::Head,
        true,
    );
    let again = NewBranchSession::open(
        reopened.generation + 1,
        base.root.clone(),
        Some("main".into()),
        NewBranchBase::Head,
        true,
    );
    assert_eq!(
        reopened.name, again.name,
        "same name, because the only input is the pre-fill"
    );
    assert_eq!(reopened.suggest, again.suggest);
    assert_eq!(reopened.start, again.start);
    assert_ne!(
        reopened.generation, again.generation,
        "and the salt is the one field that never repeats"
    );
}

#[test]
fn enter_with_nothing_armed_creates_the_text_in_the_field() {
    let rows = rows();
    let mut held = typing("fe");
    assert_eq!(
        held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.enter = true))),
        DialogReply::Create
    );
    assert_eq!(held.name, "fe", "exactly what the field reads");
}

#[test]
fn enter_accepts_the_armed_row_and_creates_nothing() {
    let rows = rows();
    let mut held = typing("fe");
    held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.down = true)));
    let reply = held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.enter = true)));
    assert_eq!(
        reply,
        DialogReply::Nothing,
        "Enter does what the highlighted row says"
    );
    assert_eq!(held.name, "feat/");
}

#[test]
fn enter_refuses_to_create_an_empty_name() {
    for blank in ["", "   "] {
        let rows = rows();
        let mut held = typing(blank);
        assert_eq!(
            held.reduce(&with_keys(Some(blank), &rows, pressed(|k| k.enter = true))),
            DialogReply::Nothing,
            "the gate the footer button wears cannot be stepped around"
        );
    }
}

#[test]
fn down_arms_the_first_row_and_up_arms_the_last() {
    let rows = rows();
    let mut held = typing("f");
    held.reduce(&with_keys(Some("f"), &rows, pressed(|k| k.down = true)));
    assert_eq!(held.suggest.armed.as_deref(), Some("feat/"));

    let mut held = typing("f");
    held.reduce(&with_keys(Some("f"), &rows, pressed(|k| k.up = true)));
    assert_eq!(held.suggest.armed.as_deref(), Some("release/"));
}

#[test]
fn the_highlight_wraps_at_both_ends() {
    let rows = rows();
    let mut held = typing("f");
    for expected in ["feat/", "feature/", "release/", "feat/"] {
        held.reduce(&with_keys(Some("f"), &rows, pressed(|k| k.down = true)));
        assert_eq!(held.suggest.armed.as_deref(), Some(expected));
    }
    for expected in ["release/", "feature/", "feat/", "release/"] {
        held.reduce(&with_keys(Some("f"), &rows, pressed(|k| k.up = true)));
        assert_eq!(held.suggest.armed.as_deref(), Some(expected));
    }
}

#[test]
fn arrows_over_no_rows_arm_nothing() {
    let none: Vec<Prefix> = Vec::new();
    assert_eq!(advance(None, true, &none), None);
    assert_eq!(advance(Some("feat/"), false, &none), None);
    assert_eq!(
        advance(Some("feat/"), true, &rows()).as_deref(),
        Some("feature/"),
        "and the walk is by text, so a list rebuilt under a highlight cannot reseat it"
    );

    let mut held = typing("z");
    held.reduce(&with_keys(Some("z"), &[], pressed(|k| k.down = true)));
    assert_eq!(held.suggest.armed, None, "a closed list arms nothing");
    assert_eq!(
        held.reduce(&with_keys(Some("z"), &[], pressed(|k| k.enter = true))),
        DialogReply::Create,
        "and Enter is still Create"
    );
}

#[test]
fn down_from_a_row_that_left_the_list_restarts_at_the_top() {
    let rows = rows();
    let mut held = typing("f");
    held.suggest.armed = Some("bugfix/".into());
    held.reduce(&with_keys(Some("f"), &rows, pressed(|k| k.down = true)));
    assert_eq!(held.suggest.armed.as_deref(), Some("feat/"));
}

#[test]
fn tab_arms_the_first_row_and_a_second_tab_accepts_it() {
    let rows = rows();
    let mut held = typing("fe");
    held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.tab = true)));
    assert_eq!(
        held.suggest.armed.as_deref(),
        Some("feat/"),
        "Tab arms a list nothing has armed yet, and focus stays in the field"
    );
    let reply = held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.tab = true)));
    assert_eq!(reply, DialogReply::Nothing);
    assert_eq!(held.name, "feat/");
}

#[test]
fn tab_with_no_list_on_screen_is_eguis() {
    let rows = rows();
    let mut held = typing("fe");
    let reply = held.reduce(&with_keys(None, &rows, pressed(|k| k.tab = true)));
    assert_eq!(reply, DialogReply::Nothing);
    assert_eq!(held.suggest.armed, None);
}

#[test]
fn clicking_a_row_accepts_it_ahead_of_the_highlight() {
    let rows = rows();
    let mut held = typing("f");
    held.suggest.armed = Some("feat/".into());
    let mut clicked = frame(Some("f"), &rows);
    clicked.clicked_candidate = Some("feature/");
    assert_eq!(held.reduce(&clicked), DialogReply::Nothing);
    assert_eq!(held.name, "feature/", "the click wins over the highlight");
}

#[test]
fn accepting_closes_the_list_because_the_row_stops_extending_the_text() {
    let rows = rows();
    let mut held = typing("fe");
    held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.down = true)));
    assert_eq!(
        held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.enter = true))),
        DialogReply::Nothing
    );
    assert_eq!(held.name, "feat/");

    let next = rows_for("feat/");
    assert!(
        next.is_empty(),
        "the derivation drops the row that was just taken, with no flag for it"
    );
    assert!(!show_list(Some("feat/"), &next, &held.suggest));
    assert_eq!(
        held.reduce(&with_keys(
            Some("feat/"),
            &next,
            pressed(|k| k.enter = true)
        )),
        DialogReply::Create,
        "and the next Enter creates the branch the field reads"
    );
}

#[test]
fn escape_folds_the_list_and_drops_the_highlight() {
    let rows = rows();
    let mut held = typing("fe");
    held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.down = true)));
    assert_eq!(held.suggest.armed.as_deref(), Some("feat/"));
    assert_eq!(
        held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.escape = true))),
        DialogReply::Nothing,
        "the first Escape belongs to the list"
    );
    assert_eq!(held.suggest.folded_at.as_deref(), Some("fe"));
    assert_eq!(held.suggest.armed, None, "and the highlight goes with it");
    assert!(!show_list(Some("fe"), &rows, &held.suggest));
}

#[test]
fn a_second_escape_closes_the_dialog() {
    let rows = rows();
    let mut held = typing("fe");
    held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.down = true)));
    held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.escape = true)));
    assert_eq!(
        held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.escape = true))),
        DialogReply::Close
    );
    assert_eq!(held.name, "fe", "closing creates nothing");
}

#[test]
fn escape_with_no_list_on_screen_closes_the_dialog() {
    let rows = rows();
    let nothing = rows_for("z");
    for (live, on_screen) in [
        (None, rows.as_slice()),
        (Some(""), rows.as_slice()),
        (Some("z"), nothing.as_slice()),
    ] {
        let mut held = typing(live.unwrap_or("main"));
        assert_eq!(
            held.reduce(&with_keys(live, on_screen, pressed(|k| k.escape = true))),
            DialogReply::Close,
            "text that claims no list is no reason to swallow the key"
        );
    }
}

#[test]
fn typing_differently_after_a_fold_brings_the_list_back() {
    let rows = rows();
    let mut held = typing("fe");
    held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.escape = true)));
    assert!(!show_list(Some("fe"), &rows, &held.suggest));
    let fea = rows_for("fea");
    assert!(
        show_list(Some("fea"), &fea, &held.suggest),
        "the fold is keyed to the text it happened at, not to the whole session"
    );
    let zeta = rows_for("zeta");
    assert!(
        !show_list(Some("zeta"), &zeta, &held.suggest),
        "and a text no row extends stays closed anyway"
    );
}

#[test]
fn an_armed_row_is_pruned_when_the_text_stops_matching_it() {
    let fe = rows();
    let mut held = typing("fe");
    held.reduce(&with_keys(Some("fe"), &fe, pressed(|k| k.down = true)));
    assert_eq!(held.suggest.armed.as_deref(), Some("feat/"));

    let relea = rows_for("relea");
    held.reduce(&frame(Some("relea"), &relea));
    assert_eq!(
        held.suggest.armed, None,
        "the row left the list, so Enter cannot accept it"
    );
    assert_eq!(
        held.reduce(&with_keys(
            Some("relea"),
            &relea,
            pressed(|k| k.enter = true)
        )),
        DialogReply::Create,
        "and Enter creates the typed name instead"
    );
}

#[test]
fn the_footer_buttons_reach_the_same_reply_as_the_keys() {
    let rows = rows();
    let mut held = typing("fe");
    let mut create = frame(Some("fe"), &rows);
    create.create_clicked = true;
    assert_eq!(held.reduce(&create), DialogReply::Create);

    let mut held = typing("fe");
    let mut cancel = frame(Some("fe"), &rows);
    cancel.cancel_clicked = true;
    assert_eq!(held.reduce(&cancel), DialogReply::Close);
}

#[test]
fn a_folded_list_never_arms_from_the_arrow_keys() {
    let rows = rows();
    let mut held = typing("fe");
    held.suggest.folded_at = Some("fe".into());
    held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.down = true)));
    assert_eq!(held.suggest.armed, None);
    assert_eq!(
        held.reduce(&with_keys(Some("fe"), &rows, pressed(|k| k.enter = true))),
        DialogReply::Create
    );
}

#[test]
fn show_list_needs_live_text_and_a_row_that_extends_it() {
    let rows = rows();
    let clear = SuggestState::default();
    assert!(
        !show_list(None, &rows, &clear),
        "a field that does not hold focus is not writing a prefix"
    );
    assert!(
        !show_list(Some(""), &rows, &clear),
        "neither is an empty one"
    );
    let nothing: Vec<Prefix> = Vec::new();
    assert!(
        !show_list(Some("fe"), &nothing, &clear),
        "and no rows means no list"
    );
    assert!(show_list(Some("fe"), &rows, &clear));
    let folded = SuggestState {
        armed: None,
        folded_at: Some("fe".into()),
    };
    assert!(
        !show_list(Some("fe"), &rows, &folded),
        "folded at exactly this text"
    );
}
