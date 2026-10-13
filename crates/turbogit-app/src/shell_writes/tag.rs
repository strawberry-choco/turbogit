//! The Create tag dialog's one write.
//!
//! Create and publish are one verb, because the dialog's "Push immediately"
//! box is part of the same ask: a tag created and not pushed is a different
//! outcome from one created and refused, and the failure message says so.

use turbogit_domain::model::{RootId, TagSpec};
use turbogit_services::tag_service;

use crate::operation::Operation;
use crate::root_caches::Affected;

/// Create `spec`'s tag in `root`, pushing it to the first remote when `push`.
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
                // Reports the outcome: the error carries the
                // created-tag context when the push fails.
                tag_service::push_new(v, r, &name)?;
            }
            Ok(())
        },
    )
}
