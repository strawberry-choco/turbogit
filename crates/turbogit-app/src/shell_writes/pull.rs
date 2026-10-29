use std::path::PathBuf;

use crate::operation::Operation;
use crate::root_caches::Affected;

pub fn selection(paths: Vec<PathBuf>) -> Operation {
    let n = paths.len();
    Operation::custom(format!("Pull · {n} repos"), Affected::All, move |v| {
        for p in &paths {
            v.pull(p, false)?;
        }
        Ok(())
    })
}
