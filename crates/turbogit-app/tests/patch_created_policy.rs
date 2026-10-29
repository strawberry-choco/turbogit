use std::path::{Path, PathBuf};
use tempfile::TempDir;
use test_support::git_seed::{commit, git};
use turbogit_app::state::{AppState, Dialog, PatchSource};
use turbogit_domain::model::{PatchCreatedPolicy, PatchEncoding, PatchOrigin, VcsSettings};

const TWO_FILE_PATCH: &str = "diff --git a/src/App.java b/src/App.java\n\
                              --- a/src/App.java\n\
                              +++ b/src/App.java\n\
                              @@ -1 +1 @@\n\
                              -one\n\
                              +ONE\n\
                              diff --git a/README.md b/README.md\n\
                              --- a/README.md\n\
                              +++ b/README.md\n\
                              @@ -1 +1 @@\n\
                              -readme\n\
                              +README\n";

fn repo_at(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    git(dir, &["init", "-q", "-b", "main", dir.to_str().unwrap()]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test"]);
    commit(dir, "README.md", "readme\n", "init");
    commit(dir, "src/App.java", "one\n", "Align the parser output");
    dir.to_path_buf()
}

struct Fixture {
    #[allow(dead_code)]
    project: TempDir,
    out: TempDir,
    #[allow(dead_code)]
    root: PathBuf,
    state: AppState,
}

fn fixture(policy: PatchCreatedPolicy) -> Fixture {
    let project = TempDir::new().unwrap();
    let root = repo_at(project.path());
    let out = TempDir::new().unwrap();
    let mut state = AppState::for_roots(project.path(), std::slice::from_ref(&root)).with_settings(
        VcsSettings {
            when_patch_is_created: policy,
            ..VcsSettings::default()
        },
    );
    let id = state.multi.roots[0].id.clone();
    state.selected_root = Some(id.clone());
    let d = &mut state.ui.dlg;
    d.create_patch_root = Some(id);
    d.create_patch_origin = Some(PatchOrigin::LocalChanges { paths: Vec::new() });
    d.create_patch_raw = Some(TWO_FILE_PATCH.as_bytes().to_vec());
    d.create_patch_base_path.clear();
    d.create_patch_strip_leading = 0;
    d.create_patch_reverse = false;
    d.create_patch_encoding = PatchEncoding::Utf8;
    d.create_patch_to_clipboard = false;
    d.create_patch_location = out.path().to_string_lossy().into_owned();
    d.create_patch_name = "align-the-parser-output.patch".to_string();
    Fixture {
        project,
        out,
        root,
        state,
    }
}

fn written(fx: &Fixture) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(fx.out.path())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    files.sort();
    files.into_iter().next()
}

fn armed_ask(fx: &Fixture) -> Option<PathBuf> {
    fx.state.ui.patch_created.as_ref().map(|a| a.path.clone())
}

#[test]
fn asking_leaves_the_created_patch_waiting_for_an_answer() {
    let mut fx = fixture(PatchCreatedPolicy::Ask);
    turbogit_app::create_patch::finish(&mut fx.state);

    let file = written(&fx).expect("the write itself is not the policy's job");
    assert_eq!(
        armed_ask(&fx).as_deref(),
        Some(file.as_path()),
        "the ask has to name the very file the route wrote"
    );
    assert!(
        fx.state.ui.dialog.is_none(),
        "asking is not opening a window: the developer has not chosen one yet"
    );
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("src/App.java"),
        "the patch on disk is the payload the dialog held"
    );
    let expected = fx.state.multi.roots[0].id.clone();
    assert!(
        fx.state
            .ui
            .patch_created
            .as_ref()
            .is_some_and(|a| a.root == expected),
        "the ask carries the root it was created for"
    );
}

#[test]
fn showing_in_files_hands_the_file_to_the_surface_that_reads_patches() {
    let mut fx = fixture(PatchCreatedPolicy::ShowInFiles);
    turbogit_app::create_patch::finish(&mut fx.state);

    let file = written(&fx).expect("the patch is written whatever the policy is");
    assert!(
        armed_ask(&fx).is_none(),
        "a policy that already acted must not also ask"
    );
    assert_eq!(fx.state.ui.dialog, Some(Dialog::ApplyPatch));
    assert_eq!(
        fx.state.ui.dlg.apply_patch_path,
        file.to_string_lossy(),
        "the window shows the file that was just created, not an empty one"
    );
    assert_eq!(fx.state.ui.dlg.apply_patch_source, PatchSource::File);
}

#[test]
fn doing_nothing_arms_no_ask_and_opens_no_window() {
    let mut fx = fixture(PatchCreatedPolicy::DoNothing);
    turbogit_app::create_patch::finish(&mut fx.state);

    assert!(written(&fx).is_some(), "the patch itself was still created");
    assert!(armed_ask(&fx).is_none());
    assert!(fx.state.ui.dialog.is_none());
    assert!(
        fx.state.ui.toast.is_none(),
        "`Do nothing` is not `Do nothing then mention it`"
    );
}

#[test]
fn every_arm_records_the_created_patch_in_the_durable_feed() {
    for policy in [
        PatchCreatedPolicy::Ask,
        PatchCreatedPolicy::ShowInFiles,
        PatchCreatedPolicy::DoNothing,
    ] {
        let mut fx = fixture(policy);
        turbogit_app::create_patch::finish(&mut fx.state);
        let file = written(&fx).expect("a created patch");
        let named = fx
            .state
            .ui
            .activity
            .entries
            .iter()
            .any(|e| e.message.contains("align-the-parser-output.patch"));
        assert!(
            named,
            "{policy:?} left the feed without the created patch: {:?} ({})",
            fx.state
                .ui
                .activity
                .entries
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>(),
            file.display()
        );
    }
}

#[test]
fn the_clipboard_route_creates_no_file_and_so_asks_nothing() {
    for policy in [PatchCreatedPolicy::Ask, PatchCreatedPolicy::DoNothing] {
        let mut fx = fixture(policy);
        fx.state.ui.dlg.create_patch_to_clipboard = true;
        turbogit_app::create_patch::finish(&mut fx.state);

        assert!(
            written(&fx).is_none(),
            "{policy:?}: the clipboard arm must not also write a file"
        );
        assert!(
            armed_ask(&fx).is_none(),
            "{policy:?}: nothing was created, so the policy is not consulted"
        );
        assert!(fx.state.ui.dialog.is_none());
        assert!(
            fx.state.ui.dlg.create_patch_clipboard.is_some(),
            "{policy:?}: the payload still reaches the frame that owns the clipboard"
        );
    }
}

#[test]
fn the_policy_survives_a_restart() {
    let project = TempDir::new().unwrap();
    let root = repo_at(project.path());
    let state = AppState::for_roots(project.path(), std::slice::from_ref(&root));
    turbogit_app::persistence::save_settings(
        project.path(),
        &VcsSettings {
            when_patch_is_created: PatchCreatedPolicy::DoNothing,
            ..state.settings.clone()
        },
    )
    .unwrap();

    let reloaded = turbogit_app::persistence::load_settings(project.path()).unwrap();
    assert_eq!(
        reloaded.when_patch_is_created,
        PatchCreatedPolicy::DoNothing,
        "a written policy comes back as it was written"
    );
}

#[test]
fn a_state_file_written_before_the_policy_existed_still_loads_and_asks() {
    #[derive(serde::Serialize)]
    struct LegacyProjectState {
        mappings: Vec<turbogit_domain::model::DirMapping>,
        settings: LegacySettings,
    }

    #[derive(serde::Serialize)]
    struct LegacySettings {
        git_executable: String,
        staging_area: bool,
        synchronous_branches: bool,
        update_method: turbogit_domain::model::UpdateMethod,
        clean_tree_method: turbogit_domain::model::CleanTreeMethod,
        protected_branch_patterns: Vec<String>,
        warn_crlf: bool,
        warn_detached: bool,
        commit_template: String,
        restore_workspace: bool,
        gutter_markers: bool,
        date_format: turbogit_domain::model::DateFormat,
        no_commit_hooks: bool,
        in_process_diffs: bool,
    }

    let project = TempDir::new().unwrap();
    std::fs::create_dir_all(project.path().join(".turbogit")).unwrap();
    let old = LegacySettings {
        git_executable: String::new(),
        staging_area: false,
        synchronous_branches: false,
        update_method: turbogit_domain::model::UpdateMethod::default(),
        clean_tree_method: turbogit_domain::model::CleanTreeMethod::default(),
        protected_branch_patterns: vec!["main".to_string()],
        warn_crlf: true,
        warn_detached: true,
        commit_template: String::new(),
        restore_workspace: false,
        gutter_markers: true,
        date_format: turbogit_domain::model::DateFormat::default(),
        no_commit_hooks: false,
        in_process_diffs: true,
    };
    std::fs::write(
        project.path().join(".turbogit").join("state.ron"),
        ron::ser::to_string_pretty(
            &LegacyProjectState {
                mappings: Vec::new(),
                settings: old,
            },
            ron::ser::PrettyConfig::new(),
        )
        .unwrap(),
    )
    .unwrap();

    let loaded = turbogit_app::persistence::load_settings(project.path());
    assert!(
        !loaded
            .as_ref()
            .is_err_and(|e| e.to_string().contains("when_patch_is_created")),
        "a missing key must not make the whole settings store unreadable: {loaded:?}"
    );
    assert_eq!(
        loaded.unwrap().when_patch_is_created,
        PatchCreatedPolicy::Ask,
        "the default is the captured `Ask`, not whichever arm this build happens to like"
    );
}

#[test]
fn a_state_file_in_the_current_shape_round_trips_the_chosen_arm() {
    let project = TempDir::new().unwrap();
    let settings = VcsSettings {
        when_patch_is_created: PatchCreatedPolicy::ShowInFiles,
        ..Default::default()
    };
    turbogit_app::persistence::save_settings(project.path(), &settings).unwrap();
    let text = std::fs::read_to_string(project.path().join(".turbogit").join("state.ron"))
        .expect("the store is where the choice lives");
    assert!(
        text.contains("when_patch_is_created"),
        "the chosen arm is written down, not inferred: {text}"
    );
    assert_eq!(
        turbogit_app::persistence::load_settings(project.path())
            .unwrap()
            .when_patch_is_created,
        PatchCreatedPolicy::ShowInFiles
    );
}
