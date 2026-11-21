# TurboGit Native Shell migration plan

TurboGit's presentation layer moves from egui to Native Shell (nsh) 0.5.0, in Rust, on macOS, Windows, and Linux.
Every user-facing surface in `crates/turbogit-ui/src/ui` appears in the coverage tables and belongs to exactly one phase.
The user keeps every git workflow and gets a redesigned visual system on natively rendered controls.
The rule is one frozen egui UI, one new UI, one shared `AppState`, and no abstraction between them.
N0 is a go/no-go spike and gates every later PR.
N1 builds the bridge, tokens, controls, and test harness.
N2 to N6 port the full surface in vertical slices, riskiest first, each behind the operator's redesign review.
N7 deletes egui and ships the nsh bundles.

## How to read this

One box is one unit of work. Every box names the evidence that checks it. A nested box is a sub-step of the box above it. Check a box only when its evidence exists, a file, a log line, a screenshot, a test run, or a SHA. The body is a how-to. The appendices explain and record.

The surface tables are the contract. A phase closes only when every row assigned to it has a route in the nsh app, behavior evidence, and a live lane or a named lane step. Evidence is a ported suite, or a named lane step for a surface that had none. The tables start as `.scratch/nsh-migration/coverage.md` and are ticked row by row.

The program runs `playbooks/autopilot-stack.md`. Each link stops at stack-ready, because the redesign needs the operator's eyes before it lands. N0 and N1 are the operator's merge gates, and N7 is the release sign-off.

Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

## Surface coverage

Every module under `crates/turbogit-ui/src/ui` appears at least once. A shared module appears in the foundation row and in each surface row it serves. The inventory is 50 top-level UI modules, 5 diff modules, 2 kit modules, 13 widget modules, 23 dialog variants, 5 shell tabs, and 3 commit sub-tabs. The suite lists are the 84 suites under `crates/turbogit-ui/tests`, plus the root `diff_parity` suite. A suite is ported by behavior, then deleted with its old screen in N7.

All 23 `Dialog` variants are mapped. N4 owns `Push`, `Merge`, `NewBranch`, `Tag`, `NewWorktree`, `RenameBranch`, `CompareBranches`, `BranchNotFullyMerged`, and `ManageRemotes`. N5 owns `Rebase`, `InteractiveRebase`, `Shelve`, `Stash`, `Unshelve`, `CherryPickTarget`, `CherryPickAcross`, `GetFromBranch`, `ApplyPatch`, `CreatePatchComment`, `PatchFileSettings`, `ImportPatches`, `RewritePreflight`, and `Reword`.

### N1. Foundation

| Surface | Old modules | New file | nsh construct | Suites |
| --- | --- | --- | --- | --- |
| Tokens, type scale, spacing, radii, fonts | `theme.rs` | `src/theme.rs`, `theme.json` | `theme()`, theme tokens | `design_tokens`, `painted_ink` |
| Buttons, inputs, selects, chips, tabs, rows, cards, pane headers, menus, dialogs, toasts, tooltips | `components.rs`, `widgets/*.rs`, `icons.rs`, `column_table.rs`, `patch_chrome.rs` | `src/ui/*.rs` | `button()`, `input()`, `select()`, `tabs()`, `data_table()`, `menu()`, `dialog()`, `toast_stack()` | `widget_library`, `icons`, `menu_item`, `menu_host`, `tab_view` |
| Gallery and visual baselines | `tests/acceptance_matrix.rs`, `tests/snapshots/*.png` | `src/screens/gallery.rs`, `tests/ui/gallery.test.tsx` | `app.screenshot()` on a platform run | `acceptance_matrix` retired and replaced |

### N2. Shell and log

| Surface | Old modules | New file | nsh construct | Suites |
| --- | --- | --- | --- | --- |
| Welcome, recents, initialize, clone, unborn preview | `welcome.rs` | `screens/welcome.rs` | `each()`, `button()`, `text()`, `pick_paths()` | `welcome`, `unborn_preview` |
| Shell frame, five tabs, top bar, status bar, activity panel | `shell.rs`, `activity_panel.rs`, `status_inflight.rs` | `screens/shell.rs` | `tabs()`, `h_resizable()`, `progress()`, `menu()` | `workspace_shell_frame`, `shell_scroll_id_clash`, `status_bar_inflight`, `header_actions`, `activity_log` |
| Banner, confirm, toast, notices | `banner.rs`, `mod.rs`, `dialogs.rs` confirm path | `screens/feedback.rs` | `toast_stack()`, `dialog()`, `alert_dialog()` | `feedback_banner`, `feedback_chrome` |
| Workspace picker | `popups.rs` workspace path, `menu_host.rs` | `screens/workspace_picker.rs` | `popover()`, `virtual_list()` | `workspace_picker` |
| Log tab, commit list, graph, filter bar, multi-root gutter, scope, scroll-to, paging, commit menu | `log_window.rs`, `log_filter_bar.rs`, `commit_menu.rs` | `screens/log.rs`, `screens/log_graph.rs` | `virtual_list()`, `canvas()`, `tree()`, `input()`, `select()` | `git_log`, `log_ux`, `log_filter_bar`, `log_frame_bounds`, `log_scope_batching`, `log_paging_rows`, `log_scroll_to_selected`, `log_commit_pane_width`, `log_commit_actions`, `commit_context_menu` |

### N3. Changes, diff, and commit

| Surface | Old modules | New file | nsh construct | Suites |
| --- | --- | --- | --- | --- |
| Commit tab, Local Changes, buckets, groups, filter, multi-select | `commit_window.rs`, `changes_menu.rs`, `changes_selection.rs` | `screens/changes.rs` | `tree()`, `virtual_list()`, `checkbox()`, `context_menu()` | `commit_window`, `changes_context_menu`, `changes_selection_gestures`, `changes_preview_across_roots`, `feedback_confirm_discard`, `partial_dispatch` |
| Diff viewer, staging gutter, hunks, gaps, horizontal scroll, image and binary | `diff/*.rs`, `patch_chrome.rs`, `hunk_nav.rs` | `screens/diff_view.rs`, `screens/image_diff.rs` | `diff_view()` read-only, `virtual_list()` plus `canvas()` when the staging gutter needs it, `image()` | `diff_viewer`, `line_granularity`, `staging_checkbox`, `staging_upgrades`, `log_diff_view`, root `diff_parity` |
| Shelf and Stash panes | `commit_window.rs`, `dialogs.rs` shelve and stash | `screens/shelf.rs` | `virtual_list()`, `context_menu()`, `dialog()` | `shelf_stash_panes` |
| Commit card, message editor, amend | `commit_window.rs` | `screens/commit.rs` | `input().multiline(true)`, `button()`, `popover()` | `commit_view_cjk_text` |

### N4. Branches and sync

| Surface | Old modules | New file | nsh construct | Suites |
| --- | --- | --- | --- | --- |
| Branch picker in the top bar | `branch_widget.rs` | `screens/branch_picker.rs` | `popover()`, `command()` | `branches_popup` |
| Branches tab, tree, groups, toolbar, scope, keyboard, undo banner | `branches.rs`, `branch_tree_view.rs`, `branches_tree.rs`, `branch_menu.rs` | `screens/branches.rs` | `tree()`, `toolbar()`, `menu()`, `context_menu()` | `branches_tab`, `branches_tree`, `branches_grouping`, `branch_tree_view`, `branch_component_kit`, `branch_context_menu`, `branches_frame_bounds`, `branch_rename_dialog`, `branch_delete_disclosure`, `branch_delete_targets_its_own_repo`, `cascade_branch` |
| New branch, tag, rename dialogs and the name suggester | `dialogs.rs` new branch and tag, `widgets/suggest.rs` | `screens/branch_dialogs.rs` | `combobox()`, `dialog()`, `date_picker()` | `new_branch_suggester`, `new_branch_from_targets_its_own_repo`, `tag_dialog` |
| Divergence and compare | `branch_divergence.rs`, `dialogs.rs` compare | `screens/divergence.rs` | `dialog()`, `diff_view()`, `data_table()` | `branch_divergence_offer` |
| Worktrees and Submodules | `worktrees.rs`, `submodules.rs`, `dialogs.rs` new worktree | `screens/worktrees.rs`, `screens/submodules.rs` | `data_table()`, `dialog()`, `toolbar()` | `worktrees_submodules_tabs` |
| Remotes | `remotes_dialog.rs` | `screens/remotes.rs` | `dialog()`, `input()`, `data_table()` | `remotes_dialog` |
| Push, pull, fetch, vcs operations, rejection | `push_dialog.rs`, `push_control.rs`, `push_rejected.rs`, `popups.rs` vcs path | `screens/sync.rs` | `dialog()`, `checkbox()`, `progress()`, `toast()` | `push_dialog`, `push_rejected`, `pull_guards`, `feedback_confirm_branch_ahead` |
| Merge dialog and preview | `dialogs.rs` merge | `screens/merge.rs` | `dialog()`, `select()`, `checkbox()`, `diff_view()` | `merge_dialog` |
| Branch performance fixtures | `perf_log_branches.rs`, `perf_log_branch_filter.rs` | `tests/perf_branches.rs` | `nsh test --trace` | `perf_log_branches`, `perf_log_branch_filter` |

### N5. Rewrite and conflicts

| Surface | Old modules | New file | nsh construct | Suites |
| --- | --- | --- | --- | --- |
| Rebase dialog and onto fields | `dialogs.rs` rebase | `screens/rebase.rs` | `dialog()`, `select()`, `input()`, `checkbox()` | `rebase_dialog`, `rebase_onto_fields` |
| Interactive rebase editor and reword | `interactive_rebase.rs`, `reword_editor.rs` | `screens/rebase_editor.rs` | `virtual_list()`, drag, `input()`, `menu()` | `interactive_rebase_editor` |
| Cherry-pick target and across roots | `cherry_across.rs`, `dialogs.rs` cherry-pick | `screens/cherry_pick.rs` | `dialog()`, `tree()` | `cherry_pick_across` |
| Get from branch | `get_from_branch.rs` | `screens/get_from_branch.rs` | `dialog()`, `data_table()` | `get_from_branch` |
| Apply, create, and import patches | `apply_patch.rs`, `create_patch.rs`, `dialogs.rs` patch variants | `screens/patches.rs` | `dialog()`, `editor()`, `input()` | `apply_patch`, `create_patch` |
| Unshelve | `unshelve_dialog.rs` | `screens/unshelve.rs` | `dialog()`, `tree()` | `unshelve_dialog` |
| Conflicts, merge editor, bulk resolve | `conflict_resolver.rs`, `conflicts.rs`, `kit.rs`, `kit/conflict_pane.rs` | `screens/conflicts.rs` | `h_resizable()`, `editor()`, `toolbar()` | `conflict_resolver`, `merge_editor` |
| Rewrite preflight, undo, gating | `rewrite_preflight.rs`, `rewrite_undo.rs`, `rewrite_gating.rs` | `screens/rewrite.rs` | `dialog()`, `alert_dialog()` | `rewrite_gating` |
| Bulk preflight, monitor, history | `bulk_preflight.rs`, `bulk_monitor.rs` | `screens/bulk.rs` | `dialog()`, `progress()`, `virtual_list()` | `bulk_run`, `bulk_operations`, `bulk_history` |

### N6. Secondary surfaces and audit

| Surface | Old modules | New file | nsh construct | Suites |
| --- | --- | --- | --- | --- |
| Blame | `blame_view.rs` | `screens/blame.rs` | `virtual_list()`, `context_menu()` | `blame_view` |
| Settings, general, git, updates, date picker | `settings_modal.rs`, `widgets/date_picker.rs` | `screens/settings.rs` | `dialog()`, `tabs()`, `switch()`, `select()`, `date_picker()` | `settings_modal` |
| Shortcuts window | `shortcuts_window.rs` | `screens/shortcuts.rs` | `dialog()`, `data_table()` | none, covered by the keyboard lanes |
| Command palette | `popups.rs` command path | `screens/palette.rs` | `command()` | none, covered by the palette lane |
| Custom command | `popups.rs` action path | `screens/custom_command.rs` | `dialog()`, `input()` | `custom_command` |
| Empty, error, loading, polish, a11y, motion | `r4_polish.rs`, shared feedback paths | across screens | `on_key()`, accessibility props, reduced motion | `r4_polish` |

### N7. Retirement

| Surface | Old modules | New file | Action | Suites |
| --- | --- | --- | --- | --- |
| egui UI crate, harness, snapshots, kittest config | `crates/turbogit-ui/**`, `crates/test-support/**` harness, `kittest.toml` | none | delete | all 84 old suites go with their screens |

## Program checklist

### Arm the program

- [ ] State the protocol and this plan to the operator, then stop. Start execution only on the operator's explicit go.
- [ ] Copy the seven surface tables into `.scratch/nsh-migration/coverage.md`. Tick a row only when its route, behavior evidence, and lane exist.
- [ ] Read these from the workspace at program start. Re-read them at every tick.
  - [ ] `.agents/skills/poteto-mode/playbooks/autopilot-stack.md`
  - [ ] `.agents/skills/swarm/SKILL.md`
  - [ ] `.agents/skills/poteto-mode/playbooks/opening-a-pr.md`
  - [ ] `.agents/skills/principle-prove-it-works/SKILL.md`
  - [ ] `.agents/skills/principle-sequence-verifiable-units/SKILL.md`
- [ ] Read these from trunk and diff them against the workspace copies at every tick, because the skill folder is git-excluded.
  - [ ] `git show origin/main:AGENTS.md`
  - [ ] `git show origin/main:Cargo.toml`
  - [ ] `git show origin/main:docs/nsh-migration-plan.md`
- [ ] On the operator's go, arm the audit tick as `/loop 1h` with the tick prompt below. Never leave the cadence to memory.
- [ ] Use this tick prompt, verbatim. "Re-read the execution playbook from trunk. Audit the operation against it and fix drift in this tick. Probe every active lane and judge progress by side effects only. Stand down a stuck lane and dispatch its replacement now. Then post a short status message to the operator in chat only when the audit found a tracked change that no earlier status message reported, such as a PR opened, a code-ready head, a round launched or closed, a verdict, a merge, a stuck agent and the action taken, a blocker added or cleared, or a decision only the operator can make. Name every such change and nothing else. Do not repeat a table, the merged list, or an unchanged blocker. If the audit found none, end the turn with no reply text. Either way, log this tick's row in your decision trail. The row names the items reported, or none."
- [ ] On the operator's hold or stand-down, send every owner a zero-writes order at once.

### Spawn owners

- [ ] Spawn one owner per PR with the lifecycle `autopilot-stack.md` names.
- [ ] Follow this dependency graph. N0 gates all. N1 after N0. N2 after N1. N3 after N2. N4 after N3. N5 after N4. N6 after N5. N7 after N6. Run the phases serially, because N2 owns the route table every later screen registers in.
- [ ] Hold the file boundaries. N0 touches `spikes/nsh/**`, `docs/adr/0035-nsh-ui-migration.md`, `docs/nsh-spike-report.md`. N1 touches `crates/turbogit-ui-nsh/**`, `crates/test-support/**`, the root `Cargo.toml`, `.github/workflows/**`, `DESIGN.md`. N2 to N6 touch `crates/turbogit-ui-nsh/src/**` plus that phase's old suites. N7 touches deletions, root wiring, `release.yml`, and docs.
- [ ] Freeze `crates/turbogit-ui`. Only release-blocking fixes land there during N2 to N6, and the same owner re-applies each one in the nsh crate.
- [ ] Defer new UI features until N7 unless the operator names them release-blocking. A feature built twice is the one cost this plan refuses.
- [ ] Hold the review gate. Every PR from N1 to N7 changes a visible surface and waits for the operator's screenshots-and-video review before it is stack-ready.

### PR mechanics, for every PR

- [ ] Resolve the forge once. Default to `gh`. If `command -v origin` succeeds and Origin can resolve the repository, use `origin pr`. Record any fallback to `gh`.
- [ ] Open the PR ready, never draft. Branch from the parent id's branch, never from trunk, because the program is one stack.
- [ ] Run `cargo fmt -- --check`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p turbogit-ui-nsh`, and `nsh check` once before the PR-facing push.
- [ ] Run `/deslop` before each commit and `/no-comments` before review, or the harness equivalents that `poteto-mode` names.
- [ ] Triage every Bugbot and security-reviewer comment per `.agents/skills/poteto-mode/references/bugbot-triage.md`.
- [ ] Never rebase a stack link. The root owns the chain topology.

### Verdict and merge, for every PR

- [ ] A full swarm runs once at the stack-ready head. Another runs only for new behavior, a redesign, or a large new diff. A fix round gets a single verifier at the new head.
- [ ] One gating lane. The ten live lanes from the PR's **Verify, live** block. The perf lane from its **Verify, perf** block. Two audit lanes that read the diff and distrust the PR body.
- [ ] Clean only when every lane is PASS. A finding goes back to the owner, including a defect a lane files as a note.
- [ ] The root sets each clean PR's base to its parent branch. The operator lands the chain bottom-up. N0, N1, and N7 stop at merge-ready until the operator clicks.

### Boot recipe, for every live lane

Each live lane runs at the PR head on the reference Mac. Portability lanes repeat on Windows 10 1809 or later and Ubuntu 24.04 with GTK 4.14.

- [ ] `git fetch origin <head-branch> && git checkout <head SHA>`.
- [ ] `cd crates/turbogit-ui-nsh && nsh run --target macos`, or `cargo run -p turbogit-ui-nsh` when the nsh CLI is unavailable. Wait for the shell window.
- [ ] Run headless behavior with `cargo test -p turbogit-ui-nsh -- --nocapture`. Drive native input and take screenshots with `nsh test --target macos` over the phase's `tests/ui/*.test.tsx` file. Read only diagnostics with `nsh run --fps` and `nsh test --trace`.
- [ ] Save every screenshot to `/tmp/swarm-<pr-id>/worker-<n>/<slug>.png` and return the paths with the report.

## Prove the platform with a spike (N0)

**Depends on.** None.

**Files.**

- [ ] Create `spikes/nsh/**` as a throwaway nsh Rust app, outside the Cargo workspace.
- [ ] Create `docs/nsh-spike-report.md` with the numbers, screenshots, the go/no-go table, and the Linux distribution decision.
- [ ] Create `docs/adr/0035-nsh-ui-migration.md` recording the platform choice, the 0.5.0 pin, and the minimum platform floors.

**Build.**

- [ ] Run `nsh init spikes/nsh --template rust`, pin nsh 0.5.0 in the report, and record `nsh --version`.
- [ ] Port the log row shape into `virtual_list(count, |at| row)` with `estimated_item_size(px(24.0))`, a per-row `canvas()` lane strip, and a 100k commit fixture.
- [ ] Port the diff into `diff_view()` and prove the staged-line gutter. If `diff_view()` cannot host the gutter, prove the fallback, a `virtual_list()` with a `canvas()` gutter and a 20k line fixture.
- [ ] Prove the tree surfaces with `tree()`, one grouped file fixture and one 10k branch fixture.
- [ ] Prove the table surfaces with `data_table()`, a 1k worktree fixture and a shortcuts fixture.
- [ ] Prove the editors with `editor()` and `input().multiline(true)`, including CJK, emoji, selection, undo, and paste.
- [ ] Prove the overlays with `menu()`, `context_menu()`, `dialog()`, `alert_dialog()`, `popover()`, `command()`, `toast_stack()`, and `on_key("mod+k")`.
- [ ] Prove the bridge with an `Rc<RefCell<AppState>>`, one revision signal, the event pump, and a 10k row render.
- [ ] Prove the harness with `nsh_testing::Headless` at 1280x820, dark, and a manual clock.
- [ ] Prove screenshots with one `tests/ui/spike.test.tsx` that runs `nsh test --target macos` and writes a PNG.
- [ ] Package with `nsh package --target macos`, `--target windows`, and `--target linux`, and launch each bundle.
- [ ] Decide the Linux shape. Test the bundle on Ubuntu 24.04, then inside an AppImage or Flatpak that carries GTK 4.14 and glibc 2.39, and record the result.
- [ ] Fill the go/no-go table. Fail the spike on any hard must-have, the 60 fps log, the 60 fps diff, aligned graph lanes, keyboard-complete navigation, a working editor and IME, or a launching bundle on all three desktops.

**You see.**

- [ ] `nsh run --target macos` opens the spike with the log, diff, tree, and table fixtures.
- [ ] `nsh package` writes a bundle on each desktop and each bundle launches.
- [ ] `cargo test` in the spike runs a headless press, type, key, and scroll test.
- [ ] `nsh test --target macos` writes a screenshot and a trace.
- [ ] The report carries a measured p95 for log and diff, the editor verdict, and a clear go or no-go.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Headless test presses a log row, filters the list, and asserts the selected commit and the visible label.
- [ ] Headless test scrolls to row 100000 and asserts the row is visible.
- [ ] Headless test selects 500 diff lines and asserts the staging model.
- [ ] Headless test types a CJK commit message and asserts the round trip.
- [ ] Headless test presses `mod+k`, navigates the palette, and asserts the command runs.
- [ ] Run `cargo test` in the spike.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.7-xhigh-fast` at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Run the same 100k fixture on egui trunk and the spike. Save `spike-regression.png`. Pass when both show the same commits and rows and the spike meets the frame budget.
- [ ] Lane 2. Scroll the 100k log with a fling. Save `spike-log-scroll.png`. Pass when no blank rows appear and the frame budget holds.
- [ ] Lane 3. Stop at three scroll offsets and check graph alignment. Save `spike-graph-align.png`. Pass when every lane meets its row within 1 px.
- [ ] Lane 4. Select 500 lines in the 20k diff and stage them. Save `spike-diff-select.png`. Pass when the selection survives scrolling and staging matches the model.
- [ ] Lane 5. Open the 10k branch tree and the 1k worktree table. Save `spike-tree-table.png`. Pass when both virtualize, focus, and scroll.
- [ ] Lane 6. Open every overlay, then dismiss each with Escape and an outside press. Save `spike-overlays.png`. Pass when focus returns to the trigger every time.
- [ ] Lane 7. Drive the keyboard path Backtick, `mod+k`, arrows, Enter, Escape. Save `spike-keyboard.png`. Pass when focus is visible at every step and each action lands.
- [ ] Lane 8. Type Chinese text, an emoji, and a long unbroken path into the editor. Save `spike-cjk.png`. Pass when all three render, select, undo, and commit.
- [ ] Lane 9. Resize to 1024x700 and full screen, then set a 200 percent font scale. Save `spike-resize.png`. Pass when no pane clips its content.
- [ ] Lane 10. Launch the packaged bundle on macOS, Windows, and Ubuntu 24.04. Save `spike-packaged.png`. Pass when each opens the fixtures and quits cleanly.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. p95 frame build time for the 100k log scroll and the 20k diff, plus peak memory.
- [ ] Probe. `nsh run --fps` and the trace output at the same fixture, window, and display as trunk, interleaved with the egui run.
- [ ] Baseline. Record the egui trunk p95 and peak memory first at the same fixture and window.
- [ ] Rule. Head p95 is at most 2 ms over trunk and under 16.7 ms on the reference Mac. Fail above either number. Windows and Linux get a 33 ms absolute budget until each has its own baseline.

**Review gate.** The operator reviews the spike before any porting starts.

- [ ] Copy lane 2, lane 3, lane 4, and lane 6 screenshots into `media/nsh/N0-review` variants.
- [ ] Record a 30 to 60 second video of the log scroll, the diff selection, and the overlays. Save it as `media/nsh/N0-review.mp4`.
- [ ] Post the go/no-go table, the screenshots, and the video in chat. Stop at merge-ready. Wait for the operator's click.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] Only the ADR and the spike report merge. `spikes/nsh/**` is deleted or git-excluded before the merge.

## Lay the foundations (N1)

**Depends on.** N0.

**Files.**

- [ ] Create `crates/turbogit-ui-nsh/Cargo.toml` with exact-pinned `nsh-core`, `nsh-base`, `nsh-platform-macos`, `nsh-platform-windows`, and `nsh-platform-linux` at 0.5.0, plus `nsh-engine` and `nsh-testing` as dev-dependencies.
- [ ] Create `crates/turbogit-ui-nsh/nsh.json` with `name`, `rust`, `root`, `theme`, and `appearance`.
- [ ] Create `crates/turbogit-ui-nsh/theme.json` from the token layer.
- [ ] Create `crates/turbogit-ui-nsh/src/lib.rs`, `src/main.rs`, `src/bridge.rs`, `src/theme.rs`, `src/test_ids.rs`, and `src/ui/**`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/gallery.rs` and `tests/ui/gallery.test.tsx`.
- [ ] Create `DESIGN.md` with the token roles, type scale, density, and a motion inventory for every control.
- [ ] Edit the root `Cargo.toml` to add the member and its workspace dependencies.
- [ ] Edit `crates/test-support/Cargo.toml` and `src/lib.rs` to add an `nsh` harness feature.
- [ ] Edit `.github/workflows/ci.yml` to add a macOS job, `libgtk-4-dev clang` on Ubuntu 24.04, and the Windows runner prerequisites.

**Build.**

- [ ] Build the crate layout below. Every later phase fills one branch of `screens/`.

```text
crates/turbogit-ui-nsh/
  nsh.json
  theme.json
  src/
    lib.rs            App and TestRoot
    main.rs           run_app wiring
    bridge.rs         AppHandle
    theme.rs          typed tokens
    test_ids.rs       selector vocabulary
    ui/               styled controls
    screens/
      welcome.rs shell.rs feedback.rs workspace_picker.rs
      log.rs log_graph.rs branch_picker.rs
      changes.rs commit.rs diff_view.rs image_diff.rs shelf.rs
      branches.rs branch_dialogs.rs divergence.rs worktrees.rs submodules.rs remotes.rs sync.rs merge.rs
      rebase.rs rebase_editor.rs cherry_pick.rs get_from_branch.rs patches.rs unshelve.rs conflicts.rs rewrite.rs bulk.rs
      blame.rs settings.rs shortcuts.rs palette.rs custom_command.rs
```

- [ ] Define the token layer once from the roles in `crates/turbogit-ui/src/theme.rs`. Map the chrome roles to nsh's semantic theme tokens in `theme.json`. Keep diff add and delete, status, refs, lanes, and root colors as data tokens in `src/theme.rs`.
- [ ] Build `AppHandle` with one owner, one revision signal, and short borrows. Never hold a `RefCell` borrow across a callback, an await, or a component body.

```rust
#[derive(Clone)]
pub struct AppHandle {
    state: Rc<RefCell<AppState>>,
    revision: Signal<u64>,
}

impl AppHandle {
    pub fn read<R>(&self, f: impl FnOnce(&AppState) -> R) -> R {
        let _ = self.revision.get();
        f(&self.state.borrow())
    }

    pub fn update(&self, f: impl FnOnce(&mut AppState)) {
        {
            let mut state = self.state.borrow_mut();
            f(&mut state);
        }
        self.revision.update(|n| *n += 1);
    }

    pub fn pump(&self) -> Option<Duration> {
        let mut changed = false;
        let wait;
        {
            let mut state = self.state.borrow_mut();
            changed = state.drain_events() > 0;
            wait = state.tick_incoming_poll(Instant::now());
        }
        if changed {
            self.revision.update(|n| *n += 1);
        }
        wait
    }
}
```

- [ ] Drive `pump` from one nsh timer at the poll cadence, and pump once after every operation settles. Keep the bridge single-threaded on the nsh engine thread. Do not wrap `AppState` in `Arc<Mutex>`.
- [ ] Build the styled controls that every screen imports. Button, icon button, input, text area, checkbox, select, combobox, chip, tab, toolbar, pane header, row, card, data table, tree, menu, context menu, dialog, alert dialog, popover, toast, tooltip, progress, and scrollbar.
- [ ] Give every control `variant`, `size`, state props, and slots, never colors or radii.
- [ ] Add the `TestRoot` component that takes an `AppHandle` prop, so `Headless` can mount a prepared `AppState`.
- [ ] Add the `test-support` harness at 1280x820, dark, with `nsh_testing::Headless`, `EngineConfig`, `Root::new::<TestRoot>`, and the repo fixtures.
- [ ] Define the selector vocabulary in `test_ids.rs`. Rows key by root and commit, files by root and path, dialogs by name.
- [ ] Add the `KitchenSink` gallery that renders every control in every state.
- [ ] Record the compile time, startup, and idle CPU of the gallery.

**You see.**

- [ ] `nsh run` opens the gallery in a window on macOS.
- [ ] `nsh check`, `cargo clippy -p turbogit-ui-nsh -- -D warnings`, and `cargo test -p turbogit-ui-nsh` pass.
- [ ] `nsh test --target macos` writes the gallery screenshot.
- [ ] CI is green on macOS, Linux, and Windows.
- [ ] `DESIGN.md` names every state change of every control.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Token tests assert every role resolves and every ink pair meets the contrast rule, porting the intent of `design_tokens.rs`.
- [ ] Control tests assert default, hover, pressed, focused, disabled, and selected states for each control.
- [ ] Harness smoke test mounts `TestRoot` and asserts the gallery paints. Run `cargo test -p turbogit-ui-nsh`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.7-xhigh-fast` at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Compare the egui widget gallery with the nsh gallery at the same window. Save `n1-regression.png`. Pass when every role exists in both and the nsh one is intentionally redesigned.
- [ ] Lane 2. Tab through the gallery. Save `n1-focus-order.png`. Pass when focus is visible at every stop and follows reading order.
- [ ] Lane 3. Press every control with the keyboard and `mod+k`. Save `n1-keyboard-press.png`. Pass when Enter, Space, arrows, and Escape work.
- [ ] Lane 4. Open every menu, popover, dialog, alert, and toast. Save `n1-overlays.png`. Pass when each dismisses on Escape and outside press and focus returns.
- [ ] Lane 5. Type into every input, including a multiline message with CJK. Save `n1-inputs.png`. Pass when selection, undo, paste, and composition work.
- [ ] Lane 6. Render the gallery in the shipped theme and at a 200 percent font scale. Save `n1-density.png`. Pass when no control clips and no literal color appears.
- [ ] Lane 7. Resize from 1024x700 to full screen. Save `n1-resize.png`. Pass when no control clips and density stays constant.
- [ ] Lane 8. Switch display scale to 2x and back. Save `n1-hidpi.png`. Pass when strokes stay one pixel and text stays crisp.
- [ ] Lane 9. Show Chinese, emoji, and a 200 character label. Save `n1-cjk.png`. Pass when all three fit or truncate as designed.
- [ ] Lane 10. Turn on reduced motion and repeat lane 3. Save `n1-reduced-motion.png`. Pass when every control keeps its state change without motion.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Cold start to first frame, idle CPU, and release memory of the gallery.
- [ ] Probe. Launch the release gallery five times and read `nsh run --fps` and the process monitor, then do the same for the egui shell.
- [ ] Baseline. Record the egui shell's cold start, idle CPU, and memory first.
- [ ] Rule. Fail at more than 2x the egui cold start, more than 1 percent idle CPU, or more than 1.5x the egui memory.

**Review gate.** The operator reviews the design system before any screen is built.

- [ ] Copy lane 2, lane 3, lane 5, lane 6, and lane 9 screenshots into `media/nsh/N1-review` variants.
- [ ] Record a 30 to 60 second video of the gallery, including overlays and font scale. Save it as `media/nsh/N1-review.mp4`.
- [ ] Post the screenshots, the video, and `DESIGN.md` in chat. Stop at stack-ready. Wait for the operator's click.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] The operator lands N1 after the design review.

## Walk the shell and log (N2)

**Depends on.** N1.

**Files.**

- [ ] Create `crates/turbogit-ui-nsh/src/screens/welcome.rs`, replacing `welcome.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/shell.rs`, replacing `shell.rs`, `activity_panel.rs`, and `status_inflight.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/feedback.rs`, replacing `banner.rs` and the toast and confirm paths in `ui/mod.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/workspace_picker.rs`, replacing the workspace path in `popups.rs` and `menu_host.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/log.rs` and `log_graph.rs`, replacing `log_window.rs`, `log_filter_bar.rs`, and `commit_menu.rs`.
- [ ] Create `crates/turbogit-ui-nsh/tests/ui/shell.test.tsx` and `tests/ui/log.test.tsx` for the screenshot lanes.
- [ ] Port the behavior assertions from `welcome`, `unborn_preview`, `workspace_shell_frame`, `shell_scroll_id_clash`, `status_bar_inflight`, `header_actions`, `activity_log`, `feedback_banner`, `feedback_chrome`, `workspace_picker`, `git_log`, `log_ux`, `log_filter_bar`, `log_frame_bounds`, `log_scope_batching`, `log_paging_rows`, `log_scroll_to_selected`, `log_commit_pane_width`, `log_commit_actions`, and `commit_context_menu`.

**Build.**

- [ ] Reproduce `AppState::launch` and the `dir_picker`, `patch_writer`, and `patch_reader` seams with `pick_paths(OpenDialog::folders())` and `pick_save_path(SaveDialog)`.
- [ ] Build Welcome with Open, Initialize, Clone, recents, and the unborn preview.
- [ ] Build the shell with `tabs()` for Commit, Log, Branches, Worktrees, and Submodules, a top bar, a status bar, and the activity panel.
- [ ] Build the feedback layer with `toast_stack()`, a multi-line `dialog()` for confirms that carry disclosure actions, and `alert_dialog()` for simple yes and no.
- [ ] Build the log with `virtual_list(count, |at| row)` and `estimated_item_size(px(24.0))`, a per-row `canvas()` for lane segments, the multi-root gutter, the filter bar with `input()` and `select()`, the branches pane with `tree()`, the scroll-to intent, the hidden jump band, and paging.
- [ ] Keep selection, hover, and open-menu state in signals keyed by root and commit, never in a recycled row.
- [ ] Keep every N3 to N6 route registered as a placeholder that paints a named empty state. This is the route table later phases fill.

**You see.**

- [ ] Launching with no project shows Welcome, and opening a folder enters the shell.
- [ ] All five tabs switch, the status bar reports the repo state, and the activity panel filters by repo.
- [ ] The log lists commits, scrolls to a commit, filters by branch, shows lanes, and pages.
- [ ] A multi-root project shows one gutter cell per root and filters independently.
- [ ] Banner, confirm, and toast paint and dismiss as before.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Welcome tests cover open, initialize, clone, recent launch, and the unborn preview.
- [ ] Shell tests cover tab switching, status items, activity filters, and header actions.
- [ ] Log tests cover selection, range selection, filter, scope, scroll-to, paging, and the pane width.
- [ ] Run `cargo test -p turbogit-ui-nsh`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.7-xhigh-fast` at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Open the same saved project on both and compare the first screen. Save `n2-regression.png`. Pass when the same commits, refs, tabs, and status appear.
- [ ] Lane 2. Open a project from Welcome and enter the shell. Save `n2-open-project.png`. Pass when the log appears within the start budget and recents update.
- [ ] Lane 3. Switch all five tabs and open the activity panel. Save `n2-shell-tabs.png`. Pass when each tab paints its named state and the activity filter changes.
- [ ] Lane 4. Scroll a 100k commit history to the bottom and back. Save `n2-log-scroll.png`. Pass when no row blanks and the graph stays aligned.
- [ ] Lane 5. Select a commit and use scroll-to from a hidden ref. Save `n2-scroll-to.png`. Pass when the target row is centered and selected.
- [ ] Lane 6. Filter by a remote branch and clear it. Save `n2-filter.png`. Pass when only matching commits remain and the count matches trunk.
- [ ] Lane 7. Open a multi-root project and filter one root. Save `n2-multi-root.png`. Pass when the gutter and filter agree and no root leaks.
- [ ] Lane 8. Navigate the log entirely by keyboard. Save `n2-keyboard.png`. Pass when arrows, Enter, Escape, and the palette cover every read-only path.
- [ ] Lane 9. Show a banner, a confirm with a disclosure, and a toast. Save `n2-feedback.png`. Pass when each dismisses and the action lands.
- [ ] Lane 10. Repeat lane 4 on Windows and Ubuntu 24.04. Save `n2-portability.png`. Pass when scroll behavior and fonts match macOS.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. p95 frame build time for log scroll at 100k rows and for a filter change.
- [ ] Probe. `nsh run --fps` on the same fixture, window, and display as trunk, interleaved with the egui run.
- [ ] Baseline. Record the egui trunk p95 first at the same fixture and window.
- [ ] Rule. Head p95 is at most 2 ms over trunk and under 16.7 ms. Fail above either number.

**Review gate.** The operator reviews the shell and log redesign.

- [ ] Copy lane 2, lane 3, lane 4, lane 6, and lane 9 screenshots into `media/nsh/N2-review` variants.
- [ ] Record a 30 to 60 second video of opening a project, switching tabs, scrolling, filtering, and selecting. Save it as `media/nsh/N2-review.mp4`.
- [ ] Post the screenshots and the video in chat. Stop at stack-ready. Wait for the operator's click.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] The operator lands N2 after the redesign review.

## Stage changes and commit (N3)

**Depends on.** N2.

**Files.**

- [ ] Create `crates/turbogit-ui-nsh/src/screens/changes.rs`, replacing `commit_window.rs`, `changes_menu.rs`, and `changes_selection.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/diff_view.rs` and `image_diff.rs`, replacing `diff/mod.rs`, `diff/view.rs`, `diff/actions.rs`, `diff/panes.rs`, `hunk_nav.rs`, and `patch_chrome.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/shelf.rs`, replacing the shelf and stash panes in `commit_window.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/commit.rs`, replacing the commit card and message editor in `commit_window.rs`.
- [ ] Create `crates/turbogit-ui-nsh/tests/ui/changes.test.tsx` and `tests/ui/diff.test.tsx`.
- [ ] Port the behavior assertions from `commit_window`, `changes_context_menu`, `changes_selection_gestures`, `changes_preview_across_roots`, `feedback_confirm_discard`, `partial_dispatch`, `diff_viewer`, `line_granularity`, `staging_checkbox`, `staging_upgrades`, `log_diff_view`, `shelf_stash_panes`, and `commit_view_cjk_text`.
- [ ] Keep `tests/diff_parity.rs` green against the nsh app and the model, then move it under the nsh crate in N7.

**Build.**

- [ ] Build the Commit tab with a Local Changes list, bucket headers, the filter, multi-root groups, and range selection. Use `tree()` for grouped files and `virtual_list()` where the tree does not fit.
- [ ] Build the diff viewer with `diff_view()` for the read-only case. If N0 proved the staging gutter needs it, use `virtual_list()` with a `canvas()` gutter and per-line staging.
- [ ] Carry hunk navigation, gap expansion, horizontal scroll, char drag, caret, and the staged-line oracle from `turbogit-app` without reinterpreting it in the UI.
- [ ] Build image and binary diff panes with `image()` and the fallback pane.
- [ ] Build Shelf and Stash with `virtual_list()`, `context_menu()`, and the shelve and stash dialogs.
- [ ] Build the commit card with `input().multiline(true)`, the commit action, and the amend path.
- [ ] Keep line selection and hunk focus in signals keyed by root, file, and line.
- [ ] Route every mutation through the existing `Operation` dispatch and the existing confirm flow.

**You see.**

- [ ] Selecting a file shows the same diff as trunk, including line numbers and staged lines.
- [ ] Staging a single line moves exactly that line, and the change survives a scroll.
- [ ] The bucket headers, filter, and multi-select match trunk.
- [ ] Shelf and Stash list, apply, drop, and open their dialogs.
- [ ] Committing writes the message and refreshes the log, the status, and the file list.
- [ ] A binary file and an image file show their designed panes without a panic.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Changes tests cover buckets, filter, range selection, multi-root groups, and discard confirms.
- [ ] Diff tests cover line selection, hunk navigation, staging, unstaging, gap expansion, and char drag.
- [ ] Commit tests cover the empty message, CJK text, newline handling, and the amend path.
- [ ] Shelf and Stash tests cover apply, drop, and empty states.
- [ ] Run `cargo test -p turbogit-ui-nsh`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.7-xhigh-fast` at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Stage the same lines in the same fixture on both. Save `n3-regression.png`. Pass when the resulting index and working tree match byte for byte.
- [ ] Lane 2. Open a project and walk the Commit tab. Save `n3-changes-list.png`. Pass when buckets, groups, filter, and selection match trunk.
- [ ] Lane 3. Stage one line, then a hunk, then a whole file. Save `n3-stage.png`. Pass when the staged counts match `git diff --cached`.
- [ ] Lane 4. Discard a changed line behind the confirm. Save `n3-discard.png`. Pass when the file restores and the toast names the action.
- [ ] Lane 5. Commit a CJK message with an emoji. Save `n3-commit-cjk.png`. Pass when the commit message and the log row match the input.
- [ ] Lane 6. Amend the last commit. Save `n3-amend.png`. Pass when the message and tree update and the SHA changes.
- [ ] Lane 7. Scroll a 20k line diff and drag a selection. Save `n3-large-diff.png`. Pass when alignment and selection survive both.
- [ ] Lane 8. Open an image diff and a binary diff. Save `n3-binary.png`. Pass when both render their designed panes.
- [ ] Lane 9. Shelve, view in Shelf, apply, and drop. Save `n3-shelf.png`. Pass when the shelf list and the working tree match.
- [ ] Lane 10. Repeat lane 3 and lane 5 on Windows. Save `n3-portability.png`. Pass when the index result matches macOS.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. p95 frame build time for a 20k line diff during scroll, selection drag, and staging.
- [ ] Probe. `nsh run --fps` on the same fixture and window as trunk, interleaved with the egui run.
- [ ] Baseline. Record the egui trunk p95 first at the same fixture and window.
- [ ] Rule. Head p95 is at most 2 ms over trunk and under 16.7 ms. Fail above either number.

**Review gate.** The operator reviews the diff, changes, and commit redesign.

- [ ] Copy lane 2, lane 3, lane 5, and lane 8 screenshots into `media/nsh/N3-review` variants.
- [ ] Record a 30 to 60 second video of staging, discarding, shelving, and committing. Save it as `media/nsh/N3-review.mp4`.
- [ ] Post the screenshots and the video in chat. Stop at stack-ready. Wait for the operator's click.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] The operator lands N3 after the redesign review.

## Branch, sync, and inspect refs (N4)

**Depends on.** N3.

**Files.**

- [ ] Create `crates/turbogit-ui-nsh/src/screens/branch_picker.rs`, replacing `branch_widget.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/branches.rs`, replacing `branches.rs`, `branch_tree_view.rs`, `branches_tree.rs`, and `branch_menu.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/branch_dialogs.rs`, replacing the new branch, tag, and rename paths in `dialogs.rs` and `widgets/suggest.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/divergence.rs`, replacing `branch_divergence.rs` and the compare path in `dialogs.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/worktrees.rs`, `submodules.rs`, `remotes.rs`, `sync.rs`, and `merge.rs`, replacing `worktrees.rs`, `submodules.rs`, `remotes_dialog.rs`, `push_dialog.rs`, `push_control.rs`, `push_rejected.rs`, the vcs operations path in `popups.rs`, and the merge path in `dialogs.rs`.
- [ ] Create `crates/turbogit-ui-nsh/tests/ui/branches.test.tsx` and `tests/ui/sync.test.tsx`.
- [ ] Port the behavior assertions from `branches_popup`, `branches_tab`, `branches_tree`, `branches_grouping`, `branch_tree_view`, `branch_component_kit`, `branch_context_menu`, `branches_frame_bounds`, `branch_rename_dialog`, `branch_delete_disclosure`, `branch_delete_targets_its_own_repo`, `cascade_branch`, `new_branch_suggester`, `new_branch_from_targets_its_own_repo`, `tag_dialog`, `branch_divergence_offer`, `worktrees_submodules_tabs`, `remotes_dialog`, `push_dialog`, `push_rejected`, `pull_guards`, `feedback_confirm_branch_ahead`, and `merge_dialog`.
- [ ] Move the branch perf fixtures into `tests/perf_branches.rs` and keep their numbers comparable.

**Build.**

- [ ] Build the top-bar branch picker with `popover()` and `command()`, including filter and keyboard.
- [ ] Build the Branches tab with `tree()`, the toolbar, scope chip, grouping toggles, keyboard navigation, and the undo banner.
- [ ] Build checkout, create, rename, delete, compare, tag, cascade, and the not-fully-merged disclosure with `menu()`, `context_menu()`, `dialog()`, and `alert_dialog()`.
- [ ] Build the new branch field with `combobox()` and the name suggester, and the tag dialog with `date_picker()`.
- [ ] Build divergence and compare with `dialog()`, `diff_view()`, and `data_table()`.
- [ ] Build Worktrees and Submodules with `data_table()`, `toolbar()`, and their dialogs.
- [ ] Build Remotes with `dialog()`, `input()`, and `data_table()`.
- [ ] Build push, pull, fetch, preview, rejection, force-with-lease, and the guards with `dialog()`, `checkbox()`, `progress()`, and `toast()`.
- [ ] Build the merge dialog with `select()`, `checkbox()`, and the preview.
- [ ] Route every mutation through `Operation` and the existing preflight checks.

**You see.**

- [ ] The branch picker, tree, groups, and labels match trunk, including the upstream ellipsis rule.
- [ ] Create, checkout, rename, delete, compare, and tag each complete or disclose their consequence.
- [ ] Worktrees, submodules, and remotes list, add, edit, and remove.
- [ ] A rejected push shows the rejection pane and the force-with-lease path.
- [ ] The merge dialog previews and lands.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Branch tests cover grouping, collapse, filter, keyboard, rename, delete, cascade, and checkout.
- [ ] Dialog tests cover new branch, tag, compare, divergence, and not fully merged.
- [ ] Sync tests cover push, pull, fetch, rejection, guards, and ahead and behind counts.
- [ ] Worktree, submodule, and remote tests cover add, edit, remove, and empty states.
- [ ] Run `cargo test -p turbogit-ui-nsh`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.7-xhigh-fast` at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Open the same branch fixture on both. Save `n4-regression.png`. Pass when every branch, tag, upstream, and worktree row matches.
- [ ] Lane 2. Expand, collapse, group, and filter the branch tree. Save `n4-branch-tree.png`. Pass when groups behave and the log follows the filter.
- [ ] Lane 3. Check out a local branch and a remote tracking branch. Save `n4-checkout.png`. Pass when HEAD moves and the status bar updates.
- [ ] Lane 4. Create, rename, and delete a branch, then tag a commit. Save `n4-branch-actions.png`. Pass when each confirmation lands and the tree refreshes.
- [ ] Lane 5. Compare two branches and open the divergence surface. Save `n4-compare.png`. Pass when the rows and the offer match trunk.
- [ ] Lane 6. Add and remove a worktree, then a submodule. Save `n4-worktrees.png`. Pass when the tables reflect each lifecycle step.
- [ ] Lane 7. Open Remotes and edit both URLs. Save `n4-remotes.png`. Pass when the values persist.
- [ ] Lane 8. Push a branch, push again with nothing to send, then trigger a rejection and retry. Save `n4-push.png`. Pass when the plan, the up-to-date state, and the retry all match.
- [ ] Lane 9. Open the merge dialog and land a merge. Save `n4-merge.png`. Pass when the preview and the resulting graph match trunk.
- [ ] Lane 10. Repeat lane 2 and lane 8 on Windows and Ubuntu 24.04. Save `n4-portability.png`. Pass when the rows and plans match macOS.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. p95 frame build time for the branch pane with 10k local branches, 15k remote branches, and 8k tags.
- [ ] Probe. Run the branch perf fixture against both UIs, interleaved, with the same window and display.
- [ ] Baseline. Record the egui trunk p95 first for open, idle scroll, expand, and filter.
- [ ] Rule. Head p95 is at most 2 ms over trunk and under 16.7 ms. Fail above either number.

**Review gate.** The operator reviews the branches and sync redesign.

- [ ] Copy lane 2, lane 4, lane 8, and lane 9 screenshots into `media/nsh/N4-review` variants.
- [ ] Record a 30 to 60 second video of branch actions, a push, a rejection, and a merge. Save it as `media/nsh/N4-review.mp4`.
- [ ] Post the screenshots and the video in chat. Stop at stack-ready. Wait for the operator's click.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] The operator lands N4 after the redesign review.

## Rewrite history and resolve conflicts (N5)

**Depends on.** N4.

**Files.**

- [ ] Create `crates/turbogit-ui-nsh/src/screens/rebase.rs`, `rebase_editor.rs`, and `cherry_pick.rs`, replacing the rebase and cherry paths in `dialogs.rs`, `interactive_rebase.rs`, `reword_editor.rs`, and `cherry_across.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/get_from_branch.rs`, `patches.rs`, and `unshelve.rs`, replacing `get_from_branch.rs`, `apply_patch.rs`, `create_patch.rs`, `unshelve_dialog.rs`, and the patch paths in `dialogs.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/conflicts.rs`, replacing `conflict_resolver.rs`, `conflicts.rs`, `kit.rs`, and `kit/conflict_pane.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/rewrite.rs` and `bulk.rs`, replacing `rewrite_preflight.rs`, `rewrite_undo.rs`, `rewrite_gating.rs`, `bulk_preflight.rs`, and `bulk_monitor.rs`.
- [ ] Create `crates/turbogit-ui-nsh/tests/ui/rewrite.test.tsx` and `tests/ui/conflicts.test.tsx`.
- [ ] Port the behavior assertions from `rebase_dialog`, `rebase_onto_fields`, `interactive_rebase_editor`, `cherry_pick_across`, `get_from_branch`, `apply_patch`, `create_patch`, `unshelve_dialog`, `conflict_resolver`, `merge_editor`, `rewrite_gating`, `bulk_run`, `bulk_operations`, and `bulk_history`.

**Build.**

- [ ] Build the rebase dialog and onto fields with `dialog()`, `select()`, `input()`, and `checkbox()`.
- [ ] Build the interactive rebase editor with `virtual_list()`, drag reorder, the verb menu, and the reword field.
- [ ] Build cherry-pick target and across with `dialog()` and `tree()`.
- [ ] Build Get from branch with `dialog()` and `data_table()`.
- [ ] Build apply, create, and import patches with `dialog()`, `editor()`, and `input()`.
- [ ] Build unshelve with `dialog()` and `tree()`.
- [ ] Build the conflict resolver with `h_resizable()`, three `editor()` panes or `diff_view()` panes, the toolbar, and bulk resolve.
- [ ] Build the merge editor for conflicted files.
- [ ] Build rewrite preflight, undo, and gating with `dialog()` and `alert_dialog()`.
- [ ] Build bulk preflight, monitor, and history with `dialog()`, `progress()`, and `virtual_list()`.
- [ ] Keep drag reorder state in signals keyed by commit id, never in recycled rows.
- [ ] Route every plan through `Operation` and the existing preflight and undo values.

**You see.**

- [ ] Reordering a rebase plan and running it produces the same history as trunk.
- [ ] A conflicting rebase opens the resolver, and saving resolves every hunk.
- [ ] A cherry-pick across roots lands in the target root only.
- [ ] Patches apply, fail with a file and line, and import.
- [ ] A bulk run reports each root's outcome and offers undo.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Rebase tests cover reorder, squash, fixup, drop, reword, onto, and the one-line-per-commit rule.
- [ ] Conflict tests cover take ours, take theirs, edit, save, and bulk resolve.
- [ ] Patch tests cover create, apply, reverse apply, import, and a malformed patch.
- [ ] Bulk tests cover preflight, per-root reports, history, and undo.
- [ ] Run `cargo test -p turbogit-ui-nsh`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.7-xhigh-fast` at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Run the same six-commit rebase on both. Save `n5-regression.png`. Pass when the resulting SHA sequence and messages match.
- [ ] Lane 2. Reorder and squash a rebase plan with the mouse. Save `n5-rebase-drag.png`. Pass when the plan matches the drop order after a scroll.
- [ ] Lane 3. Drop, fixup, and reword, then run the plan. Save `n5-rebase-run.png`. Pass when the log matches the plan.
- [ ] Lane 4. Trigger a conflict and resolve it in all panes. Save `n5-conflict.png`. Pass when the file is saved and the rebase continues.
- [ ] Lane 5. Open the merge editor and resolve a conflicted merge. Save `n5-merge-editor.png`. Pass when the merge completes.
- [ ] Lane 6. Cherry-pick a commit into another root. Save `n5-cherry.png`. Pass when only the target root changes.
- [ ] Lane 7. Get files from another branch. Save `n5-get-from-branch.png`. Pass when the file rows and the checkout match trunk.
- [ ] Lane 8. Create, apply, import, and malformed-apply a patch, then unshelve. Save `n5-patch.png`. Pass when each outcome is reported.
- [ ] Lane 9. Run a bulk operation across three roots and undo it. Save `n5-bulk.png`. Pass when every root returns to its prior state.
- [ ] Lane 10. Repeat lane 4 on Windows. Save `n5-portability.png`. Pass when the conflict save and continue match macOS.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. p95 frame build time for a 1k commit rebase plan and a 1k hunk conflict file.
- [ ] Probe. Build both fixtures, then run `nsh run --fps` at the same window and display as trunk, interleaved.
- [ ] Baseline. Record the egui trunk p95 first for open, reorder, drag, and save.
- [ ] Rule. Head p95 is at most 2 ms over trunk and under 16.7 ms. Fail above either number.

**Review gate.** The operator reviews the rewrite and conflict redesign.

- [ ] Copy lane 2, lane 4, lane 6, and lane 9 screenshots into `media/nsh/N5-review` variants.
- [ ] Record a 30 to 60 second video of a reorder, a conflict resolution, a cherry-pick, and an undo. Save it as `media/nsh/N5-review.mp4`.
- [ ] Post the screenshots and the video in chat. Stop at stack-ready. Wait for the operator's click.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] The operator lands N5 after the redesign review.

## Finish secondary surfaces and audit parity (N6)

**Depends on.** N5.

**Files.**

- [ ] Create `crates/turbogit-ui-nsh/src/screens/blame.rs`, replacing `blame_view.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/settings.rs`, replacing `settings_modal.rs` and `widgets/date_picker.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/shortcuts.rs` and `palette.rs`, replacing `shortcuts_window.rs` and the command path in `popups.rs`.
- [ ] Create `crates/turbogit-ui-nsh/src/screens/custom_command.rs`, replacing the custom command path in `popups.rs`.
- [ ] Create `crates/turbogit-ui-nsh/tests/ui/blame.test.tsx`, `settings.test.tsx`, and `palette.test.tsx`.
- [ ] Port the behavior assertions from `blame_view`, `settings_modal`, `custom_command`, and `r4_polish`.
- [ ] Edit `DESIGN.md` to close every motion inventory line against the running app.
- [ ] Edit `docs/design-system-roles.md` to name the nsh owner for every role.

**Build.**

- [ ] Build blame with `virtual_list()`, range colors, and the reveal path.
- [ ] Build settings with `dialog()`, `tabs()`, `switch()`, `select()`, and `date_picker()`, covering general, git, updates, and the version badge.
- [ ] Build the shortcuts window with `dialog()` and `data_table()`, driven from the same keymap as `on_key`.
- [ ] Build the command palette with `command()`, including the actions that open other surfaces.
- [ ] Build custom command with `dialog()` and `input()`.
- [ ] Audit every screen for keyboard reachability, focus return, role and name, and reduced motion. Encode each gap as a test, not a note.
- [ ] Audit every screen for empty, loading, error, and 200 percent font scale states.
- [ ] Delete every leftover egui-flavored helper that has no nsh caller.

**You see.**

- [ ] Every surface in the seven coverage tables has a route and a ladder of states.
- [ ] VoiceOver names every control and announces every toast.
- [ ] Reduced motion removes motion without removing the state change.
- [ ] The role document points only at nsh owners.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] The remaining behavior suites pass in the nsh crate.
- [ ] The accessibility audit test asserts a role and a name for every interactive control.
- [ ] The token audit test asserts no literal color or radius outside the token layer.
- [ ] Run `cargo test -p turbogit-ui-nsh`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.7-xhigh-fast` at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Walk every secondary surface on both. Save `n6-regression.png`. Pass when every surface exists and every load-bearing value matches.
- [ ] Lane 2. Open blame for a 2k line file and jump to a commit. Save `n6-blame.png`. Pass when the range colors and the log jump agree.
- [ ] Lane 3. Change every setting and restart. Save `n6-settings.png`. Pass when each value survives the restart.
- [ ] Lane 4. Open the shortcuts window and run three listed shortcuts. Save `n6-shortcuts.png`. Pass when the listed keys match the working keys.
- [ ] Lane 5. Use the command palette for a branch, a commit, and a setting. Save `n6-palette.png`. Pass when each result lands on the right surface.
- [ ] Lane 6. Run a custom command and read its output. Save `n6-custom-command.png`. Pass when the result is reported.
- [ ] Lane 7. Run the accessibility pass with VoiceOver. Save `n6-voiceover.png`. Pass when every control has a name and a role and focus is announced.
- [ ] Lane 8. Turn on reduced motion and set a 200 percent font scale. Save `n6-motion-density.png`. Pass when nothing animates and no text clips.
- [ ] Lane 9. Show an empty repository, a detached HEAD, and a failing operation. Save `n6-empty-error.png`. Pass when each state explains itself.
- [ ] Lane 10. Repeat lane 7 on Windows with Narrator. Save `n6-narrator.png`. Pass when names and roles match macOS.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Cold start, idle CPU, and p95 log scroll after every surface is mounted.
- [ ] Probe. Launch the release app five times and read `nsh run --fps`, the process monitor, and the trace, interleaved with trunk.
- [ ] Baseline. Record the egui trunk values first at the same window, display, and fixture.
- [ ] Rule. Fail at more than 1.5x the trunk cold start, more than 1 percent idle CPU over five minutes, or p95 2 ms over trunk.

**Review gate.** The operator reviews the finished app.

- [ ] Copy lane 2, lane 3, lane 7, and lane 9 screenshots into `media/nsh/N6-review` variants.
- [ ] Record a 60 second video of the whole shell. Save it as `media/nsh/N6-review.mp4`.
- [ ] Post the screenshots and the video in chat. Stop at stack-ready. Wait for the operator's click.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] The operator lands N6 after the redesign review.

## Cut over and delete egui (N7)

**Depends on.** N6.

**Files.**

- [ ] Delete `crates/turbogit-ui/**` and its tests and snapshots.
- [ ] Delete `kittest.toml`.
- [ ] Edit the root `Cargo.toml` to remove `egui`, `egui_extras`, `egui_kittest`, `dify`, and the old crate.
- [ ] Edit `src/main.rs` and `src/app.rs` to launch the nsh root, and delete the old `TurbogitApp`.
- [ ] Edit `crates/test-support/**` to remove the egui harness feature.
- [ ] Edit `.github/workflows/release.yml` to produce `nsh package` bundles for macOS, Windows, and Linux, with the Linux shape N0 chose.
- [ ] Rename `crates/turbogit-ui-nsh` to `crates/turbogit-ui` and update every path.
- [ ] Edit `README.md`, `AGENTS.md`, `CONTEXT.md`, `docs/architecture.md`, and `docs/design-system-roles.md`.
- [ ] Create `docs/adr/0036-egui-removal.md` recording the cutover.

**Build.**

- [ ] Make the root binary launch the nsh root and nothing else, preserving the 1280x820 initial size and the 1000x680 minimum.
- [ ] Remove every egui dependency from the workspace lock and prove it with `cargo tree -i egui`.
- [ ] Keep the old UI on the annotated tag `egui-final` so a regression can be bisected.
- [ ] Produce release bundles for all three desktops. macOS gets a signed and notarized `.app`. Windows gets a zipped bundle. Linux gets the AppImage or Flatpak from N0, plus a tarball for Ubuntu 24.04 or later.
- [ ] Update the quality gates in `AGENTS.md` for the nsh loop and the platform prerequisites.

**You see.**

- [ ] `cargo run` opens the nsh shell with no egui crate in the dependency tree.
- [ ] `cargo tree -i egui` fails because the crate is gone.
- [ ] Each release bundle installs, launches, and reads the recents file written by the egui build.
- [ ] The docs describe one UI, one harness, and one release path.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] The whole workspace passes `cargo test --workspace --all-targets --no-fail-fast`.
- [ ] The egui harness tests and snapshot baselines are gone with their crate.
- [ ] `cargo fmt -- --check`, `cargo check --workspace --all-targets`, and clippy pass.
- [ ] Run the four local gates from `AGENTS.md`.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `grok-4.7-xhigh-fast` at the PR head, per the boot recipe.

- [ ] Lane 1. Regression lane against trunk. Open the same project on the egui tag and the nsh build. Save `n7-regression.png`. Pass when every surface and saved setting matches.
- [ ] Lane 2. Install the macOS bundle on a clean user account. Save `n7-macos-install.png`. Pass when it launches and reads recents.
- [ ] Lane 3. Install the Windows bundle on a clean Windows 10 1809 virtual machine. Save `n7-windows-install.png`. Pass when it launches and reads recents.
- [ ] Lane 4. Install the Linux bundle on Ubuntu 24.04 and on one older distribution through the chosen bundle. Save `n7-linux-install.png`. Pass when both launch.
- [ ] Lane 5. Upgrade from the egui build with an open project. Save `n7-upgrade.png`. Pass when recents, settings, and the last project survive.
- [ ] Lane 6. Uninstall and verify no user data is destroyed. Save `n7-uninstall.png`. Pass when only the app bundle is removed.
- [ ] Lane 7. Launch with no display, over SSH, and on a second display. Save `n7-display.png`. Pass when each path fails or opens as documented.
- [ ] Lane 8. Run the full keyboard path from the shortcuts window. Save `n7-keyboard.png`. Pass when every shortcut lands.
- [ ] Lane 9. Walk the app with a reader on macOS and Windows. Save `n7-reader.png`. Pass when every control is named and reachable.
- [ ] Lane 10. Run the 100k log and 20k diff on the release bundle. Save `n7-release-perf.png`. Pass when the p95 matches the N2 and N3 numbers.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Release cold start, idle CPU, peak memory, and p95 log and diff frames for the shipped bundle.
- [ ] Probe. Launch each release bundle five times and read `nsh run --fps`, the process monitor, and the trace, interleaved with the egui tag.
- [ ] Baseline. Record the egui release values first at the same window, display, and fixture.
- [ ] Rule. Fail at more than 1.5x the egui cold start, more than 1 percent idle CPU, more than 1.5x peak memory, or p95 2 ms over egui.

**Review gate.** The operator signs off the release artifacts.

- [ ] Copy lane 2, lane 3, lane 4, and lane 9 screenshots into `media/nsh/N7-review` variants.
- [ ] Record a 60 second video of the installed bundle on each desktop. Save it as `media/nsh/N7-review.mp4`.
- [ ] Post the screenshots, the video, and the bundle links in chat. Stop at merge-ready. Wait for the operator's click.

**Merge.**

- [ ] Root's clean verdict at the exact head SHA.
- [ ] Bugbot triage done.
- [ ] The operator lands the final link and tags the release.

## Close the program

- [ ] Every box above is checked with its evidence.
- [ ] Every row in the seven coverage tables is ticked with a route, behavior evidence, and a lane.
- [ ] Every one of the 84 old suites is deleted with its screen, and its behavior lives in the nsh crate.
- [ ] Reply to the operator with the bundle links, the p95 table, and the list of shipped surfaces.
- [ ] Archive `docs/nsh-spike-report.md` and the decision trail, and note any lane that never ran.

## Appendix A. Prototype evidence

No prototype has run yet. N0 is the prototype, and its go/no-go table is the gate.

The questions that stay unproven until N0 records a number.

- Does `virtual_list` plus a per-row `canvas` hold 60 fps at 100k rows with aligned lanes.
- Does `diff_view` accept a staged-line gutter, or does the fallback `virtual_list` plus `canvas` win.
- Do `tree` and `data_table` cover the grouped file list, the branch tree, and the worktree tables without a custom widget.
- Does `editor` cover the conflict and patch surfaces, including folding, search, and large files.
- Does `input` in multiline mode handle IME, CJK, emoji, undo, and paste.
- Do the overlays, focus return, and `on_key` cover the whole keyboard surface.
- Does the `Rc<RefCell<AppState>>` bridge avoid re-entrant borrows under a 10k row render.
- Does `Headless` at 1280x820 reproduce the flows the kittest harness covers today.
- Does a TSX screenshot test on `--target macos` give the visual proof the old PNG baselines gave.
- Does a packaged bundle launch on macOS 14, Windows 10 1809, and Linux with GTK 4.14 and glibc 2.39.

## Appendix B. Alternatives rejected

- Stay on egui and restyle. Rejected because the goal is natively rendered controls and native accessibility on three desktops.
- Use GPUI directly. Rejected as the primary target because nsh already wraps it, and `nsh run --target gpui` stays available as a fallback backend.
- Use Slint, Qt, Dioxus, or Tauri. Rejected after the nsh goals review. Each changes the language or the runtime more than nsh does.
- Write the UI in TSX. Rejected by the operator. Rust keeps typed access to `AppState` and one toolchain.
- Build a custom diff pane. Rejected because `nsh_base::diff_view()` exists and the spike may prove it covers the read-only case.
- Build a custom code editor. Rejected because `nsh_base::editor()` exists and covers the conflict and patch surfaces.
- Build a custom tree and table. Rejected because `nsh_base::tree()` and `nsh_base::data_table()` exist and cover the branch, file, worktree, and shortcuts surfaces.
- Big-bang rewrite on one branch. Rejected because no phase would have a verification boundary.
- Keep both UIs long term. Rejected per the migrate-callers-then-delete rule. egui is frozen and deleted in N7.
- Pixel parity with the egui snapshots. Rejected by the operator. The migration includes a redesign, so behavior parity is the gate and the snapshots retire with egui.

## Appendix C. Risks

- nsh 0.5.0 API churn. Lands in every PR. The owner reads `nsh-compat` before each upgrade, pins the exact version, and vendors a fork only if a release breaks a must-have.
- The Linux floor. Lands in N0 and N7. nsh needs glibc 2.39 and GTK 4.14, so Ubuntu 24.04 or later. The owner proves an AppImage or Flatpak with a pinned runtime for older distributions.
- Text input and IME maturity. Lands in N0 and N3. The owner gates the spike on CJK, emoji, undo, and paste.
- `diff_view` gutter coverage. Lands in N0 and N3. The owner proves the staging gutter or takes the fallback in the spike.
- `editor` coverage for conflicts. Lands in N0 and N5. The owner proves a 1k hunk conflict file before N5 starts.
- Bridge re-render storms. Lands in N1 to N3. The owner starts with one revision signal, measures the p95 gate, and adds per-slice signals only where a gate fails.
- Harness regression. Lands in every PR. Each phase ports behavior assertions, not files, and the old suite stays green until its screen is deleted.
- Headless does not prove visuals. Lands in every PR. Every visual claim comes from a `--target macos` screenshot lane, never from `cargo test`.
- Redesign scope creep. Lands in N1 to N6. `DESIGN.md` and the operator's review gate bound each screen.
- Accessibility parity. Lands in N6. The audit turns every gap into a test, not a note.
- Native control skill. This harness ships `control-ui` only for browser surfaces. Native lanes use `nsh test --target` and the computer-use agent, and N0 records how repeatable each is.
- Font and bundle size. Lands in N1. The owner measures the bundled JetBrains Mono and Noto Sans SC fonts and the release bundle against the memory and start gates.
- Feature work during the migration. Lands in every phase. New UI features are deferred unless the operator names them release-blocking.

## Appendix D. Links and reading list

Read before editing.

- The nsh repository at `https://github.com/longbridge/nsh`.
- The nsh docs index at `https://nsh.dev/llms.txt`.
- The nsh Rust guide at `https://nsh.dev/docs/rust.md`.
- The nsh element index at `https://nsh.dev/elements.md`.
- The nsh list guide at `https://nsh.dev/elements/list.md`.
- The nsh canvas guide at `https://nsh.dev/elements/canvas.md`.
- The nsh diff view guide at `https://nsh.dev/elements/diff-view.md`.
- The nsh editor guide at `https://nsh.dev/elements/editor.md`.
- The nsh tree guide at `https://nsh.dev/elements/tree.md`.
- The nsh data table guide at `https://nsh.dev/elements/data-table.md`.
- The nsh command guide at `https://nsh.dev/elements/command.md`.
- The nsh testing guide at `https://nsh.dev/docs/testing-and-trace.md`.
- The nsh performance guide at `https://nsh.dev/docs/performance.md`.
- The nsh compatibility guide at `https://nsh.dev/docs/compatibility.md`.
- The nsh macOS, Windows, and Linux guides at `https://nsh.dev/docs/macos.md`, `https://nsh.dev/docs/windows.md`, and `https://nsh.dev/docs/linux.md`.
- The nsh design and coding guides, installed with `npx skills add longbridge/nsh`.
- `docs/architecture.md`, `docs/design-system-roles.md`, and `docs/adr/0026-log-virtualizes-over-show-rows-and-scoped-listings-batch.md`.
- `docs/adr/0027-design-system-v2-owns-one-accent.md` and `CONTEXT.md`.

Which PRs get the heavier design skills.

- N0 and N1 get `how` and `interrogate`, because the bridge and the token layer are one-way doors.
- N2 to N5 get `architect` only when a screen needs a new state shape.
- N7 gets `interrogate` on the release and rollback path.

Decision trail path.

- `.scratch/nsh-migration/decision-log.tsv`, one row per landed unit and one row per rejected hypothesis, per the show-me-your-work skill.
