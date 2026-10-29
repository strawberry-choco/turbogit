use turbogit_domain::model::{RootId, TagSpec};
use turbogit_services::tag_service;

use crate::operation::Operation;
use crate::root_caches::Affected;

pub fn create(root: &RootId, spec: &TagSpec, push: bool) -> Operation {
    let spec = spec.clone();
    let name = spec.name.clone();
    let root = root.clone();
    Operation::custom(
        format!("Create tag {name}"),
        Affected::Root(root.clone()),
        move |v| {
            let r = root.as_path();
            tag_service::create(v, r, &spec)?;
            if push {
                tag_service::push_new(v, r, &name)?;
            }
            Ok(())
        },
    )
}
