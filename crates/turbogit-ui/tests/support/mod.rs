//! The app-root harness the ported suites mount: the engine config the
//! gallery harness builds, over `AppRoot` and a prepared `AppState`, plus
//! the git fixtures and the pick stubbing the screens' tests share.

// Every suite compiles this module in, and each uses its own slice of it.
#![allow(unreachable_pub, dead_code)]

use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use nsh_core::element::Root;
use nsh_core::platform::Appearance;
use nsh_engine::{Clock, ColorScheme, EngineConfig, Environment, Idiom, Os, Size};
use nsh_testing::Headless;
use test_support::git_seed::{commit, git};
use turbogit_app::recents::RecentProject;
use turbogit_app::state::AppState;
use turbogit_ui::{AppRoot, AppRootProps};

/// The reference viewport the harness and the live lanes share.
pub const VIEWPORT: (f32, f32) = (1280.0, 820.0);

/// The engine config the ported suites mount: `AppRoot` over the state the
/// (once-called) builder prepares on the engine thread, 1280x820 dark,
/// manual clock.
pub fn app_config(build: impl FnOnce() -> AppState + Send + 'static) -> EngineConfig {
    let build = Mutex::new(Some(build));
    let root = Root::new::<AppRoot>(move || {
        let build = build
            .lock()
            .expect("the props builder is not shared")
            .take()
            .expect("the props closure runs once");
        AppRootProps {
            handle: turbogit_ui::bridge::AppHandle::new(build()),
        }
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

/// The welcome app with the folder-pick answer `paths`: the driver's
/// standing answer, an empty vec cancelling, exactly as the seam's
/// `Ok(None)` cancel path reads.
pub fn welcome_with_picks(paths: Vec<PathBuf>) -> Mounted {
    let config = tempfile::tempdir().expect("temp config dir");
    let config_for_state = config.path().to_path_buf();
    let mut app = Headless::start(app_config(move || {
        AppState::launch_in(None, Some(config_for_state.clone()))
    }))
    .expect("the welcome mounts");
    app.on_pick_paths(paths).expect("stub the open dialog");
    Mounted {
        app,
        _config: Some(config),
        _project: None,
    }
}

/// A plain welcome over its own empty recents config, for the paint
/// assertions. Returns the headless app and the config dir keeper.
pub fn welcome_app_standalone() -> (Headless, tempfile::TempDir) {
    let config = tempfile::tempdir().expect("temp config dir");
    let config_for_state = config.path().to_path_buf();
    let app = Headless::start(app_config(move || {
        AppState::launch_in(None, Some(config_for_state.clone()))
    }))
    .expect("the welcome mounts");
    (app, config)
}

/// The welcome app over a recents config dir.
pub fn welcome_over(config: &Path) -> Mounted {
    let config = config.to_path_buf();
    let config_for_state = config.clone();
    let app = Headless::start(app_config(move || {
        AppState::launch_in(None, Some(config_for_state.clone()))
    }))
    .expect("the welcome mounts");
    Mounted {
        app,
        // The caller's fixture keeps the seeded config dir alive.
        _config: None,
        _project: None,
    }
}

/// The shell over `launch(Some(dir))`: the production open path.
pub fn launched(dir: &Path) -> Mounted {
    let dir = dir.to_path_buf();
    let config = tempfile::tempdir().expect("temp config dir");
    let config_for_state = config.path().to_path_buf();
    let app = Headless::start(app_config(move || {
        AppState::launch_in(Some(dir.clone()), Some(config_for_state.clone()))
    }))
    .expect("the shell mounts");
    Mounted {
        app,
        _config: Some(config),
        _project: None,
    }
}

/// The shell over `launch_in(Some(project), Some(config))`: the roots the
/// rescan discovers under `project`, with `config`'s recents loaded — the
/// production launch path, for the suites that read the store.
pub fn launched_over(project: &Path, config: &Path) -> Mounted {
    let project = project.to_path_buf();
    let config_for_state = config.to_path_buf();
    let app = Headless::start(app_config(move || {
        AppState::launch_in(Some(project.clone()), Some(config_for_state.clone()))
    }))
    .expect("the shell mounts");
    Mounted {
        app,
        // The caller's fixture keeps the config dir alive.
        _config: None,
        _project: None,
    }
}

/// The shell over `for_roots(project, roots)`: exactly the registered
/// roots, the shape the shell and log suites mount.
pub fn for_roots(project: &Path, roots: &[PathBuf]) -> Mounted {
    for_roots_seeded(project, roots, |_| {})
}

/// [`for_roots`] after `seed` has touched the prepared state: the fixtures
/// that exist only as app state (a shelf record, a durable selection) start
/// where the suite puts them, before the root mounts and reads.
pub fn for_roots_seeded(
    project: &Path,
    roots: &[PathBuf],
    seed: impl FnOnce(&mut AppState) + Send + 'static,
) -> Mounted {
    let project = project.to_path_buf();
    let roots = roots.to_vec();
    let config = tempfile::tempdir().expect("temp config dir");
    let config_for_state = config.path().to_path_buf();
    let app = Headless::start(app_config(move || {
        let mut state = AppState::for_roots(&project, &roots);
        state.recents_config_dir = Some(config_for_state.clone());
        seed(&mut state);
        // `for_roots` registers without the reads a launch fires; the
        // registration's own refresh is what fills status, ahead/behind,
        // and the first log batch, as `AppState::launch_in`'s rescan does.
        state.refresh(turbogit_app::root_caches::Affected::All);
        state.drain_events();
        state
    }))
    .expect("the shell mounts");
    Mounted {
        app,
        _config: Some(config),
        _project: None,
    }
}

/// A mounted app root and the fixtures its state was built from. The
/// keepers hold the temp dirs for the engine's life.
pub struct Mounted {
    pub app: Headless,
    _config: Option<tempfile::TempDir>,
    _project: Option<tempfile::TempDir>,
}

impl Deref for Mounted {
    type Target = Headless;

    fn deref(&self) -> &Headless {
        &self.app
    }
}

impl DerefMut for Mounted {
    fn deref_mut(&mut self) -> &mut Headless {
        &mut self.app
    }
}

impl Mounted {
    /// Waits until the control `selector` reads enabled.
    pub fn wait_for_enabled(&mut self, selector: &str) -> Result<(), String> {
        wait_for_enabled(&mut self.app, selector)
    }
}

/// Whether any painted text contains `needle` (the exact matcher the
/// selector vocabulary keeps is for whole-node texts).
pub fn shows_part(app: &Headless, needle: &str) -> bool {
    app.screen()
        .texts()
        .iter()
        .any(|text| text.contains(needle))
}

/// Waits until some painted text contains `needle`.
pub fn wait_for_part(app: &mut Headless, needle: &str) -> Result<(), String> {
    app.wait_until(
        &format!("text containing {needle}"),
        Duration::from_secs(10),
        |screen| screen.texts().iter().any(|text| text.contains(needle)),
    )
}

/// Waits until the control `selector` reads enabled, on any mounted app.
pub fn wait_for_enabled(app: &mut Headless, selector: &str) -> Result<(), String> {
    app.wait_until(
        &format!("enabled:{selector}"),
        Duration::from_secs(10),
        |screen| screen.enabled(selector).unwrap_or(false),
    )
}

/// A recents store seeded with `projects`, for the welcome tests.
pub fn seed_recents(config_dir: &Path, projects: &[RecentProject]) {
    let file = turbogit_app::recents::recents_file(config_dir);
    std::fs::create_dir_all(file.parent().unwrap()).expect("create config dir");
    turbogit_app::recents::save(
        config_dir,
        &turbogit_app::recents::Recents {
            projects: projects.to_vec(),
        },
    )
    .expect("seed recents file");
}

/// A recent-project record for one seeded repository.
pub fn recent_project(path: PathBuf, name: &str) -> RecentProject {
    RecentProject {
        path,
        name: name.to_string(),
        last_opened: 1_755_000_000_000,
        kind: turbogit_app::recents::RecentKind::Project,
        repo_count: None,
    }
}

/// A temporary project directory the fixtures seed into. The `TempDir`
/// lives as long as the struct, which the suite keeps.
pub struct Project {
    dir: PathBuf,
    _keep: tempfile::TempDir,
}

impl Project {
    pub fn new() -> Self {
        let keep = tempfile::tempdir().expect("temp project dir");
        let dir = keep.path().to_path_buf();
        Project { dir, _keep: keep }
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// A path the project owns that outlives a temp seed's own dir.
    pub fn adopt(&self, path: PathBuf) -> PathBuf {
        path
    }

    /// A repository with one commit, seeded under the project dir.
    pub fn seed_repo(&self, name: &str) -> PathBuf {
        seeded_repo(&self.dir, name)
    }

    /// A bare `origin` for `repo`, pushed and upstreamed.
    pub fn seed_bare_remote(&self, name: &str) -> PathBuf {
        let work = self.seed_repo(&format!("{name}-work"));
        for n in 2..=3 {
            std::fs::write(work.join(format!("f{n}.txt")), format!("commit {n}"))
                .expect("write tracked file");
            git(&work, &["add", "."]);
            commit(
                &work,
                &format!("f{n}.txt"),
                &format!("{n}\n"),
                &format!("c{n}"),
            );
        }
        let bare = self.dir.join(format!("{name}.git"));
        let work_s = work.to_string_lossy().to_string();
        let bare_s = bare.to_string_lossy().to_string();
        git(&self.dir, &["clone", "--bare", "-q", &work_s, &bare_s]);
        bare
    }
}

/// A repository with one commit on `main`, seeded under `parent`.
pub fn seeded_repo(parent: &Path, name: &str) -> PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).expect("create repo dir");
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "user.name", "Test"]);
    commit(&dir, "README.md", "x\n", "init");
    dir
}

/// A log fixture under `parent`: one repository, `n` commits on `main`,
/// a local `topic` branch ahead, a remote-qualified `origin/main`, and a
/// tag. Answers "what the log panes read".
pub fn log_repo(parent: &Path, name: &str, n: usize) -> PathBuf {
    let work = repo_with_origin_public(parent, name);
    for i in 2..=n {
        commit_into(&work, &format!("f{i}.txt"), &format!("commit number {i}"));
    }
    git(&work, &["tag", "v1"]);
    work
}

/// `repo_with_origin` re-exported for the fixture above.
fn repo_with_origin_public(parent: &Path, name: &str) -> PathBuf {
    let bare = parent.join(format!("{name}.git"));
    git(
        parent,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    let work = seeded_repo(parent, name);
    let bare_s = bare.to_string_lossy().to_string();
    git(&work, &["remote", "add", "origin", &bare_s]);
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    work
}

/// One more commit into `repo`, with its own file.
pub fn commit_into(repo: &Path, file: &str, message: &str) {
    std::fs::write(repo.join(file), format!("{message}\n")).expect("write file");
    git(repo, &["add", "."]);
    commit(repo, file, &format!("{message}\n"), message);
}

/// A repository with three committed files, all modified afterwards: the
/// changes list's shelf of rows.
pub fn dirty_files_repo(parent: &Path, name: &str) -> PathBuf {
    let repo = seeded_repo(parent, name);
    for file in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(repo.join(file), "baseline\n").expect("write baseline");
    }
    git(&repo, &["add", "."]);
    commit(&repo, "a.txt", "baseline\n", "files");
    for file in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(repo.join(file), "modified\n").expect("write change");
    }
    repo
}

/// The Commit tab over `roots`, with the changes row set settled.
pub fn commit_tab(project: &Path, roots: &[PathBuf]) -> Mounted {
    let mut app = for_roots(project, roots);
    app.first_render().expect("the shell mounts");
    app.press("id:tab:Commit").expect("the Commit tab");
    app.wait_for("id:route:changes")
        .expect("the changes surface");
    app.wait_for("id:changes:header").expect("the changes card");
    app
}

/// Expands the repository group named `name`.
pub fn open_group(app: &mut Headless, name: &str) {
    app.press(&format!("id:changes:group:{name}"))
        .expect("the repository group");
    app.wait_for(&format!("id:changes:group:{name}"))
        .expect("the group stays");
}

/// Opens the surface whose palette command reads `label`: the entry point the
/// shell's action band used to carry for the N5/N6 surfaces.
///
/// The press is the row's label, not a test id: nsh-base's command renders the
/// dialog's rows itself and gives them none. It is scoped to the dialog because
/// the same label names a control behind it (`Blame` is also the changes pane's
/// footer), and a press on two matches fails.
pub fn open_surface(app: &mut Headless, label: &str) {
    app.press("id:toolbar:palette")
        .expect("the palette trigger");
    app.wait_for("id:dialog:command-palette")
        .expect("the palette opens");
    app.type_text("id:palette:search", label)
        .expect("the query types");
    app.press(&format!("id:dialog:command-palette >> text:{label}"))
        .expect("the command runs");
}

/// The staged patch of `repo`, as `git diff --cached` prints it.
pub fn staged_patch(repo: &Path) -> String {
    git(repo, &["diff", "--cached"])
}

/// The working-tree patch of `repo`, as `git diff` prints it.
pub fn local_patch(repo: &Path) -> String {
    git(repo, &["diff"])
}

/// A repository with one commit on `main` and the given local branches at
/// that commit, plus one tag.
pub fn branch_repo(parent: &Path, name: &str, branches: &[&str]) -> PathBuf {
    let repo = seeded_repo(parent, name);
    for branch in branches {
        git(&repo, &["branch", *branch, "main"]);
    }
    git(&repo, &["tag", "v1.0"]);
    repo
}

/// A repository on `main` with `feature` ahead of `main`.
pub fn repo_with_unmerged_feature(parent: &Path, name: &str) -> PathBuf {
    let repo = seeded_repo(parent, name);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit_into(&repo, "feature.txt", "feature work");
    git(&repo, &["checkout", "-q", "main"]);
    repo
}

/// The Branches tab over `roots`, mounted and settled.
pub fn branches_tab(project: &Path, roots: &[PathBuf]) -> Mounted {
    let mut app = for_roots(project, roots);
    app.first_render().expect("the shell mounts");
    app.press("id:tab:Branches").expect("the Branches tab");
    app.wait_for("id:route:branches")
        .expect("the branches surface");
    app.wait_for("id:branches:tree").expect("the branch tree");
    app
}

/// The branch row of `root` named `branch`.
pub fn branch_sel(root: &str, branch: &str) -> String {
    format!("id:branches:row:{root}:{branch}")
}

/// Right-clicks the branch row and asserts the menu opened.
pub fn open_branch_menu(app: &mut Mounted, root: &str, branch: &str) {
    app.right_click(&branch_sel(root, branch))
        .expect("the branch row takes a secondary press");
    assert!(app.menu().is_some(), "the branch menu opens");
}

/// The labels the live menu offers, in order.
pub fn menu_labels(app: &Mounted) -> Vec<String> {
    app.menu()
        .expect("a menu is open")
        .into_iter()
        .map(|entry| entry.label)
        .collect()
}

/// Whether the live menu's item `label` is disabled.
pub fn menu_disabled(app: &Mounted, label: &str) -> bool {
    app.menu()
        .expect("a menu is open")
        .into_iter()
        .find(|entry| entry.label == label)
        .unwrap_or_else(|| panic!("the menu offers {label}"))
        .disabled
}

/// Paints the menu's item `label`.
pub fn press_menu(app: &mut Mounted, label: &str) {
    app.choose_menu_item(label)
        .unwrap_or_else(|e| panic!("the menu chooses {label}: {e}"));
}
