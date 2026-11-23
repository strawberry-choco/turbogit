//! The nsh harness: a headless engine at 1280x820, dark, on a manual
//! clock, over `TestRoot` and a prepared `AppState`, plus the repo
//! fixtures the screens' tests mount.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use nsh_core::element::Root;
use nsh_core::platform::Appearance;
use nsh_engine::{Clock, ColorScheme, EngineConfig, Environment, Idiom, Os, Size};
use nsh_testing::Headless;
use turbogit_app::state::AppState;

/// The reference viewport the harness and the live lanes share.
pub const VIEWPORT: (f32, f32) = (1280.0, 820.0);

/// The engine config both roots run under: the spike's shape, over the
/// crate's `TestRoot` and the theme the crate ships. The state is built
/// inside the root's props closure — the root is mounted on the engine
/// thread, and the closure it stores must be `Send`, so the state cannot
/// cross it from the test.
pub fn config(project_dir: Option<PathBuf>) -> EngineConfig {
    let root = Root::new::<turbogit_ui::TestRoot>(move || turbogit_ui::TestRootProps {
        handle: turbogit_ui::bridge::AppHandle::new(match &project_dir {
            Some(dir) => AppState::for_roots(dir, std::slice::from_ref(dir)),
            None => AppState::launch(None),
        }),
    })
    .tokens(turbogit_ui::tokens())
    .appearance(Appearance::Dark);
    let mut config = EngineConfig::new(root);
    config.viewport = Size::new(
        nsh_engine::geometry::Points(VIEWPORT.0),
        nsh_engine::geometry::Points(VIEWPORT.1),
    );
    config.environment = Environment {
        window: Size::new(
            nsh_engine::geometry::Points(VIEWPORT.0),
            nsh_engine::geometry::Points(VIEWPORT.1),
        ),
        color_scheme: ColorScheme::Dark,
        os: Os::MacOs,
        idiom: Idiom::Desktop,
        ..Environment::default()
    };
    config.clock = Clock::manual(SystemTime::UNIX_EPOCH);
    config
}

/// A mounted gallery over a plain state with no project.
pub fn gallery() -> std::io::Result<Headless> {
    Headless::start(config(None))
}

/// A mounted gallery over `AppState::for_roots` of the repo at `root`, the
/// shape the screens' behavior tests mount. The caller keeps the fixture
/// dir alive for the engine's life.
pub fn with_repo(root: &Path) -> std::io::Result<Headless> {
    Headless::start(config(Some(root.to_path_buf())))
}

/// A one-commit repo under a temp dir, for a test that needs a project.
/// The caller keeps the `TempDir` alive.
pub fn one_commit_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = crate::git_seed::repo_with_one_commit(dir.path(), "repo");
    (dir, repo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_commit_repo_yields_a_repository() {
        let (_keep, repo) = one_commit_repo();
        assert!(repo.join(".git").exists(), "the fixture seeds a repository");
    }
}
