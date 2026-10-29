use std::path::Path;

use turbogit_domain::model::RootId;
use turbogit_services::changes;

use crate::operation::Operation;
use crate::root_caches::Affected;

pub fn set_staged(root: &RootId, path: &Path, stage: bool) -> Operation {
    let name = path.display();
    let label = if stage {
        format!("Stage {name}")
    } else {
        format!("Unstage {name}")
    };
    let p = path.to_path_buf();
    let root = root.clone();
    Operation::custom(label, Affected::Root(root.clone()), move |v| {
        let paths = [p];
        if stage {
            changes::stage_paths(v, root.as_path(), &paths)
        } else {
            changes::unstage_paths(v, root.as_path(), &paths)
        }
    })
}
