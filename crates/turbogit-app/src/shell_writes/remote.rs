use std::path::Path;

use turbogit_domain::model::RootId;
use turbogit_services::remote_service;

use crate::operation::Operation;
use crate::root_caches::Affected;

pub fn add(root: &RootId, path: &Path, name: &str, fetch: &str, push: &str) -> Operation {
    let p = path.to_path_buf();
    let name = name.to_string();
    let fetch = fetch.to_string();
    let add_push = push.to_string();
    Operation::custom(
        format!("Add remote {name}"),
        Affected::Root(root.clone()),
        move |v| {
            let url = if add_push.is_empty() {
                fetch.clone()
            } else {
                add_push.clone()
            };
            remote_service::add(v, &p, &name, &url)
        },
    )
}

pub fn rename(root: &RootId, path: &Path, name: &str, new: &str) -> Operation {
    let p = path.to_path_buf();
    let name = name.to_string();
    let new = new.to_string();
    Operation::custom(
        format!("Rename remote {name}"),
        Affected::Root(root.clone()),
        move |v| remote_service::rename(v, &p, &name, &new),
    )
}

pub fn update(root: &RootId, path: &Path, name: &str, fetch: &str, push: &str) -> Operation {
    let p = path.to_path_buf();
    let name = name.to_string();
    let fetch = fetch.to_string();
    let push = push.to_string();
    Operation::custom(
        format!("Update remote {name}"),
        Affected::Root(root.clone()),
        move |v| {
            let fetch = if fetch.is_empty() { None } else { Some(fetch) };
            let push = if push.is_empty() { None } else { Some(push) };
            remote_service::set_url(v, &p, &name, fetch.as_deref(), push.as_deref())
        },
    )
}

pub fn set_upstream(root: &RootId, path: &Path, branch: &str, upstream: &str) -> Operation {
    let p = path.to_path_buf();
    let b = branch.to_string();
    let upstream = upstream.to_string();
    Operation::custom(
        format!("Set upstream {b} → {upstream}"),
        Affected::Root(root.clone()),
        move |v| remote_service::set_upstream(v, &p, &b, &upstream),
    )
}

pub fn remove(root: &RootId, path: &Path, name: &str) -> Operation {
    let p = path.to_path_buf();
    let name = name.to_string();
    Operation::custom(
        format!("Remove remote {name}"),
        Affected::Root(root.clone()),
        move |v| remote_service::remove(v, &p, &name),
    )
}
