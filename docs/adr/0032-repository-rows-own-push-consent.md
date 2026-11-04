# Repository rows own push consent, and the commit list is a preview selector

The Push dialog had three surfaces that each looked like a selection and only one of which decided
anything: a `PUSH SCOPE` segmented control, a per-commit checkbox grid, and a repository row whose
highlight was a preview filter. `ADR-0006` had made the third deliberately irrelevant. We decided
that **a repository is a row, the row's checkbox is the only thing that decides whether that
repository is pushed, and the row prints the target that push will actually use**, with consent,
expansion and preview focus as three separate click targets and a single resolver feeding both the
displayed target and the executed one.

`ADR-0006` rejected per-root selective push on the ground that "the mockup gives root nodes no
checkbox semantics". The reference product gives every repository row a checkbox and selects only
the current repository by default, so the ground is gone rather than the decision being wrong for
its time. Its remaining worry — that selection-driven scope makes the primary action ambiguous — is
answered by the primary action stopping being a scope: the button says `Push`, and each repository
owns its own yes or no.

## Considered options

- **Keep `PushScope` and add a summary band that names what will be pushed**, rejected. It is a
  presentation fix for a model defect: `push_scope` and `push_scope_excluded` still reconcile at
  `crates/turbogit-app/src/state.rs`, and four named scopes cannot say "all of these except one",
  which is the request the segmented control was built to answer and never could.
- **A tri-state row box derived from the commit ticks**, rejected on both grammar and arithmetic. It
  costs two clicks to go from partial to whole, and the ticks it derives from cannot filter a push.
- **A per-row `Push all up to here…` cutoff**, rejected three times over. It is the deleted checkbox
  wearing a new name, still a click target on a commit row. It competes with `ADR-0024`, which makes
  the log's commit row the single place a commit action lives. And it is not honestly buildable:
  `build_executor` returns the `git2` adapter as the front engine
  (`crates/turbogit-engine/src/lib.rs`), and `Git2Executor::outgoing_commits` reads a `revwalk`
  whose sorting mode is the libgit2 default rather than the `TOPOLOGICAL | TIME` that
  `Git2Executor::log` sets, so list position is not ancestry and "everything above this line comes
  with it" mis-promises on a merge history.
- **One `PushArgs` struct on the port so `push` and `push_dry_run` cannot diverge**, rejected as
  scope. It existed only to carry a cutoff to the dry-run. With the cutoff gone the port change
  inverts into a deletion: `selected_oldest` leaves `GitExecutor::push` along with the refspec
  branch that consumed it.
- **A full sum type per root** — `RootPush::{Whole, Excluded, Unavailable}` — rejected because it
  makes "consented and unpushable" unrepresentable, and that state is both legal and worth
  painting: a protected row is consented and refuses only force, and a first-push row is consented
  and cannot list its commits. Consent is a stored boolean and the obstacle is a separate fact.

## Consequences

- **The commit checkbox grid is deleted, and it filtered nothing.** `subset_push_state`'s binding
  named `oldest_idx` is `outgoing.iter().position(…)` over a newest-first list, so it holds the
  *newest* selected commit: a selection containing the tip sets the refspec to the tip, and a
  selection excluding it falls to `OlderUnchecked`, which maps to no refspec and so to the tip
  again. A probe over the verbatim function enumerated every reachable tick pattern of a
  three-commit listing and **none of them reduced what git received**. The capability is not lost
  because it never existed. Its `…pushes_only_the_selected_newest_commit` test passed throughout,
  because it asserts the recorded argument and never where the remote ref landed.
- The range cut becomes what the reference makes it: a `Push All up to Here…` item on the log's
  commit context menu, in its own change, unblocked by giving `outgoing_commits` a sorting mode.
- `PushTargets::Single` and `Scope` collapse to one batch of resolved targets. That is what lets the
  branch-menu verb survive: `ui/branches.rs` seeds a branch that is not `current_branch`, and
  `push_roots` used `root.current_branch`, so the merged batch path would otherwise silently push
  HEAD instead of what was clicked. The request also stops cloning whole `Root` snapshots.
- **The dialog no longer opens with every registered repository consented.** `PushScope::default()`
  is `All`, so four of five paths that opened it had been consenting everything, and
  `docs/spec/rebased-parity.md` §5.6 recorded a ThisRepo default that did not exist. A developer who
  always pushes all seven repositories now ticks seven boxes. That is the reference's rule and the
  direction the complaint pointed, but it is a real ergonomic loss on the push-everything path, and
  the reference's own mitigation is an advanced setting this app does not have.
- A first-push repository lists **zero** commits, because `outgoing_per_root` answers empty when
  there is no upstream to be ahead of. In a row-based model that would silently drop precisely the
  repository most in need of pushing, so the row says `first push — its commits cannot be listed`
  and stays consentable. Listing them needs a new engine read.
- Force push stays an options checkbox for now; the reference's item on a `Push ▾` split primary is
  **deferred, not rejected**. What did change is where protection is enforced: it used to disable
  the whole primary button, and it now refuses force per repository at the engine while plain
  `Push` stays live. `docs/spec/rebased-parity.md` §4.6 recorded the reference as making force
  "absent" on a protected branch; the shipped product keeps it **visible and disabled** with a
  tooltip naming the setting, and the spec is corrected rather than the code following the claim.
- **Push disables on an empty payload, not an empty consent set.** A consented row whose obstacle
  blocks the push never reaches `PushPlan::targets`, so the first rule would still have let the
  dialog dispatch a batch of zero and report it a success — the original defect, one layer down.
- **The reference's inline target grammar is deferred, and the dialog is weaker for it.** Targets
  are printed on every row but are not yet click-to-edit, so `Edit all targets` is absent,
  `Define remote` is not offered inline, and `Set upstream` is derived per row by `build_row`
  rather than chosen. A repository with no remote is still addable, just through
  `Dialog::ManageRemotes` rather than in place. `ADR-0007`'s reasons are met by *printing* the
  resolved target; the editing affordance is the part that did not land.
- The dry-run and the push now describe one payload on the axis that mattered: `push_dry_run_roots`
  had been discarding the subset outright. **The residual divergence is deliberate and named**:
  `push_dry_run` on the port carries `force` and `tags` and nothing else, because `--no-verify` and
  `--set-upstream` cannot change which refs move, which is all a dry run reports.
- **The retired model is still in the tree.** `PushScope`, `roots_in_scope`, `push_roots`,
  `push_dry_run_roots`, `subset_push_state`, `SubsetPushState`, `selected_oldest` on the port and the
  seventeen `dlg.push_*` fields are no longer read by the dialog, but this change did not delete them.
  They are inert rather than harmful, and the sentence above that says `PushScope` retires is the
  design decision, not the current file state. Deleting them is the follow-up, and it is what lets
  `services/tests/push_scope.rs` be renamed rather than left naming a concept that is gone.

- The session lives in `crates/turbogit-app/src/push_session.rs`, egui-free and git-free, following
  the `new_branch.rs` precedent, so "which repositories are in the batch" is a data fact with a
  harness-free oracle rather than something only a painted widget can prove.
