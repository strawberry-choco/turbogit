use turbogit_domain::model::RootId;
use turbogit_services::branch_service;

use crate::operation::Operation;
use crate::root_caches::Affected;
use crate::state::AppState;

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

pub fn checkout(root: &RootId, name: &str, affected: Affected) -> Operation {
    let nm = name.to_string();
    let root = root.clone();
    Operation::custom(format!("Checkout {nm}"), affected, move |v| {
        v.branch_checkout(root.as_path(), &nm)
    })
}

pub fn checkout_new_local(root: &RootId, name: &str, affected: Affected) -> Operation {
    let nm = name.to_string();
    let start = format!("origin/{name}");
    let root = root.clone();
    Operation::custom(format!("Checkout {nm} (new local)"), affected, move |v| {
        v.branch_create(root.as_path(), &nm, true, Some(&start))
    })
}

pub fn checkout_detached(root: &RootId, name: &str, affected: Affected) -> Operation {
    let nm = name.to_string();
    let root = root.clone();
    Operation::custom(format!("Checkout {nm}"), affected, move |v| {
        v.checkout_detached(root.as_path(), &nm)
    })
}

/// Bring the deleted branch `name` back in `root` at `sha`, the tip the undo
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

pub fn toggle_favorite(state: &mut AppState, root: &RootId, name: &str) {
    branch_service::toggle_favorite(&mut state.multi, root, name);
}
