//! The branches pane's and the branch popup's checkout writes.
//!
//! Four verbs reach git from a branch row, a popup row, or the delete-undo
//! banner. Three of them are checkouts that differ only in *what* they point at
//! — a local branch, a remote branch that needs a local created for it, a tag
//! left detached — and the fourth restores a deleted branch at the tip the undo
//! record captured.

use turbogit_domain::model::RootId;
use turbogit_services::branch_service;

use crate::operation::Operation;
use crate::root_caches::Affected;
use crate::state::AppState;

/// Create the branch `name` in `root` from `start`, optionally checking it out.
pub fn create(root: &RootId, name: &str, start: Option<&str>, checkout: bool) -> Operation {
    let n = name.to_string();
    let start = start.map(str::to_owned);
    let root = root.clone();
    Operation::custom(
        format!("Create branch {name}"),
        Affected::Root(root.clone()),
        move |v| branch_service::create(v, root.as_path(), &n, start.as_deref(), checkout),
    )
}

/// Check `name` out in `root`. `affected` is the caller's scope because the
/// popup computes one for the whole row loop.
pub fn checkout(root: &RootId, name: &str, affected: Affected) -> Operation {
    let nm = name.to_string();
    let root = root.clone();
    Operation::custom(format!("Checkout {nm}"), affected, move |v| {
        v.branch_checkout(root.as_path(), &nm)
    })
}

/// Check `name` out in `root` as a **new local branch** tracking
/// `origin/{name}` — the remote branch row's "I want to work on this".
pub fn checkout_new_local(root: &RootId, name: &str, affected: Affected) -> Operation {
    let nm = name.to_string();
    let start = format!("origin/{name}");
    let root = root.clone();
    Operation::custom(format!("Checkout {nm} (new local)"), affected, move |v| {
        v.branch_create(root.as_path(), &nm, true, Some(&start))
    })
}

/// Check the **tag** `name` out in `root`, leaving HEAD detached.
pub fn checkout_detached(root: &RootId, name: &str, affected: Affected) -> Operation {
    let nm = name.to_string();
    let root = root.clone();
    Operation::custom(format!("Checkout {nm}"), affected, move |v| {
        v.checkout_detached(root.as_path(), &nm)
    })
}

/// Bring the deleted branch `name` back in `root` at `sha` — the tip the undo
/// banner captured before the delete ran, which is the only place that tip
/// exists.
pub fn restore(root: &RootId, name: &str, sha: &str) -> Operation {
    let n = name.to_string();
    let sha = sha.to_string();
    let root = root.clone();
    Operation::custom(
        format!("Restore branch {n}"),
        Affected::Root(root.clone()),
        move |v| v.branch_create(root.as_path(), &n, false, Some(&sha)),
    )
}

/// Flip `name`'s favourite flag in `root`.
///
/// The one entry here that touches no git: the star is app state, so it lives
/// beside the branch writes rather than in a renderer, and a renderer that
/// reached `state.multi` would be a writer for the app's own domain state.
pub fn toggle_favorite(state: &mut AppState, root: &RootId, name: &str) {
    branch_service::toggle_favorite(&mut state.multi, root, name);
}
