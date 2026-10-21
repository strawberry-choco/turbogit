# The working-tree preview reads HEAD, and a click applies the index-facing patch

The diff preview made the user pick which of git's three working-tree diffs they
were looking at. It now shows exactly one comparison, `HEAD ↔ worktree`, and a row
click stages or unstages that line out of a *different* patch than the one the pane
drew. Both halves are decisions, and the second is the one a later reader will be
tempted to undo. This record states them with the measurements behind them.

## Decision

1. **The working-tree preview is one fixed comparison, `HEAD ↔ worktree`.** It is
   not a user control, and no app site overrides it.
2. **The value a user reads and the patch a click applies are different values, and
   that is deliberate.** ADR-0013's surviving rule, restated by ADR-0022 as "a
   selection is appliable because it was composed from the same value the user
   selected from", is read here as *the value the operation is composed from*, not
   the value the pane drew.
3. **Staged-ness is a derived per-line property.** `crates/turbogit-app/src/staged_lines.rs`
   answers it from the two whole-root patches the app already reads. It is never
   stored in the displayed diff value.
4. **An unstage moves one change**, not one row and not one hunk, and the patch it
   composes describes the index rather than HEAD.

## Three sites wrote the selector, and two of them overrode the user

A `Repo | Staged | Local` chip group selected the pair. Three sites wrote that
value, and two of them were the app overriding the user: `settle_preview_on_unstaged`
forced `Local` after every granular stage, and `follow_the_staging_decision` forced
`Staged` on a file tick and `Repo` on an un-tick. Both are readable at `3ebffc4`, the
commit this program started from. One control the application overrode twice on the
ordinary path is not a control, so the axis and its state (`DiffComparison`,
`diff_comparison`) are deleted rather than defaulted.

The replacement is a constructor, not a default. `DiffTarget::working_tree`
(`crates/turbogit-app/src/keyed_read.rs:328`) names `left = Some("HEAD")` and
`right = None`, and which constructor a call site asks for is how a reader tells the
two kinds of target apart. `ui/diff/view.rs:215` routes its no-revision arm there,
`ui/diff/actions.rs:49` asks it directly, and `granular.rs`'s untracked route asks it
through a `peek`. A commit-to-commit target still goes through `DiffTarget::new` and
keeps the revisions its caller named, because a history-window diff must not claim
`HEAD` as its left side. `sides()` stayed the identity map on purpose: a mapping
there would have rewritten commit diffs too.

A comparison that names `HEAD` has no answer in a repository with no commits,
and it is not given a fabricated one. The read is refused there, and the section
below records the decision and its reason.

## A read and an operation draw on two different patch values

The pane paints `git diff HEAD`. A click composes from one of the two index-facing
patches, chosen by direction (`crates/turbogit-app/src/granular.rs:30` to `:33`):

- a **stage** forward-applies the `index ↔ worktree` section (`git diff`), whose
  pre-image is the index;
- an **unstage** reverse-applies the `HEAD ↔ index` section (`git diff --cached`),
  whose post-image is the index.

The split buys the property that makes an immediate click tolerable. Staging a line
cannot change what the worktree differs from HEAD by, so a pinned combined view is
invariant under the only action it offers: rows do not move, vanish or renumber when
one is clicked, and the pane needs no scroll or caret restoration. That stability is
why the pending set could be deleted rather than replaced, and it is pinned by a test
and not by prose, in
`the_displayed_bytes_do_not_move_while_the_answer_for_that_row_flips`
(`crates/turbogit-app/tests/combined_preview_toggle.rs:350`), which asserts the
displayed patch bytes identical across a click while the oracle's answer for that row
flips.

The rule is not weakened to make the code sound shorter. It keeps its force and names
its value: the hunk indices and ordinals a selection uses must be positions in the same
patch the selection is composed from. What changed is *which* value that is, for the
read and for the operation. One case collapses the two roles into one value, and it is
not an exception to the rule. An untracked path is in neither index-facing patch, so the
pane's synthesized creation diff is simultaneously what the reader selected from and the
only text git will take for it; `plan_untracked` (`granular.rs:323`) composes from the
displayed value and routes through intent-to-add as before.

## Staged-ness is derived per line and lives outside the displayed value

`staged_lines::derive_line_sets(staged, local)` is called inside
`ensure_hunk_stats` (`crates/turbogit-app/src/root_caches.rs:948`) on the two whole-root
patch values that fill already holds. It takes both by move and keeps each file's
section beside the numbers counted out of it, in the same fill that reduces the three
patches to hunk spans, so the appliable text and its line sets share one lifetime. Per
file it keeps the line sets and the two appliable per-file sections, and
`staged_lines::line_is_staged` answers the row question while
`staged_lines::line_source` answers the composition question. Nothing writes
staged-ness into `DiffValue`, into a `Row`, or into the pane cache. `staged_lines` is
the only module in the app allowed to relate a line of one patch to a line of
another, so a second cross-patch pairing in `granular.rs` or `partial.rs` is refused
here as well as in the ticket.

The answer's lifetime, precisely: `RootCaches::invalidate` drops the entry for the
root on every completed operation at `self.hunk_stats.remove(root)` in the
`Affected::Root` arm (`crates/turbogit-app/src/root_caches.rs:1103`), and that call is
the first statement of `AppState::refresh`
(`crates/turbogit-app/src/state.rs:2615`), which runs before settlement. So an answer
is at most one operation old, while the displayed bytes do not move at all across that
same operation. The two lifetimes differing is the point, not an accident.

Retention is the price, and it is now the dominant part of that entry. Measured
against this working tree's uncommitted diff at 30 files, 139 hunks and 3,474 body
lines, and while the tree held nothing staged, so only the `local` sections were in
memory, the retained sections cost about 265 KB, against about 11.6 KB for the line
runs and 11.8 KB for the hunk spans the entry already carried. That is roughly 71
bytes per body line counting the text and the line structs, so a 200,000-line diff
costs about 14 MB per patch and a tree with both a staged and an unstaged half pays it
twice. It is bounded by the working diff's own text and dropped as a unit by every
invalidation. The alternative, re-reading the source patch per click, is a git spawn
per click at the 14.5 ms recorded below.

## Cutting one line out of a `git diff HEAD` patch is refused

`git apply --cached` matches a hunk's context lines against the **index**. Taking one
line's change out of a `git diff HEAD` patch converts its neighbouring changes into
context lines, and those context lines describe a file state the index does not hold.
Reproduced in a throwaway repository with git 2.54.0: a file with one modified, one
added and one deleted line, all three staged, then the modified line's change
extracted from `git diff HEAD` and reverse-applied. git refused with
`error: patch failed: f.txt:1`. A forward stage of the added line, with the modified
line already staged, failed the same way.

That experiment only ever passed because git's context requirement was lowered far
enough, and `-C1` made it pass. Rejected: one combined patch plus fuzz or reduced
context. Relying on git quietly searching less text is not a correctness story worth
buying, and a selection whose acceptance depends on how much context git is allowed
to check is not appliable in any sense this app can promise the user.

## An unstage moves one change

`staged_lines::unstage_ords` returns the ordinals that must move together, which is a
`Removed` immediately followed by an `Added`, matched greedily. A modification is
therefore one replacement and its two rows move as one unit, because one row alone is
not a change git can undo: reverse-applying `+TWO` by itself deletes a line instead of
restoring its old text, and reverse-applying `-two` by itself leaves both texts in the
index. A same-kind run is not coupled. Two identical adjacent additions reverse one
line at a time, pinned by
`two_identical_adjacent_additions_stage_and_unstage_apart`,
`unstaging_one_staged_row_of_a_hunk_leaves_the_other_staged_change_staged` pins the
same answer for two changes sharing one hunk, and
`two_staged_modifications_unstage_the_clicked_one_only` pins it for two changes in
separate hunks of one file.

One shape genuinely refuses sub-hunk granularity. A file git has never had a version
of is a creation section, and `git apply --cached --reverse` refuses a partial reverse
there with `error: new file f.txt depends on old contents`.
`staged_lines::section_hunk_is_creation` detects it, and only there does an unstage
keep `HunkSelection::Whole`. That is why clicking a row of a newly added file
unstages the file. The gutter `−` still means the whole hunk, because it is a
whole-hunk button.

This granularity was measured twice, and the first measurement was wrong about what
it measured. That run handed git 52 single-ordinal trials over nine hunk shapes: all
26 forward ones accepted, all 26 reverse ones refused. It had composed every
selection with the *forward* filter and then asked for `--reverse`, so it had tested
its own composer rather than git. The refused patches claimed HEAD's text for a staged
line the index had already replaced, and dropped staged lines from the block git was
searching for. Re-measured with the reverse arm in place, single staged changes
reverse. Unstage granularity is per change, and a later reader must not re-widen it to
the hunk.

## A reverse patch describes the index, not HEAD

This is the rule a later reader will otherwise "fix" back, so it is stated with its
reason. When composing a patch to reverse-apply, in `filter_lines`
(`crates/turbogit-services/src/partial.rs:328`):

- an untouched neighbouring change is written as **context carrying the index's own
  text**, which is its post-image text;
- an unselected addition is **kept**, not dropped, so the block git searches stays
  whole;
- an unselected deletion is **dropped**, not demoted, because the index no longer
  holds HEAD's line.

The reason is that `git apply --cached --reverse` never consults HEAD. It matches the
patch's new side against the index and rewrites the file to the patch's old side, so
each of those three statements is true of the index instead of a lie about it.

The forward direction keeps exactly the behaviour ADR-0013 and ADR-0022 describe: an
unselected `-` is demoted to context carrying the old text and an unselected `+` is
dropped. That is proven by a stored byte comparison, not argued.
`forward_composition_is_byte_for_byte_what_it_was_before_the_reverse_rule`
(`crates/turbogit-services/src/partial.rs:777`) compares the forward composed bytes
over seven fixtures against literals captured from `compose`'s forward output *before*
`filter_lines` learned about direction. It is the guard on `stage_untracked_selection`,
the whole-file checkbox and the palette verbs. `compose`, `composed_or_noop` and
`filter_lines` take `turbogit_engine_api::ApplyDirection`, and only the reverse arm
changed.

## A repository with no commits refuses the `HEAD` read

`HEAD ↔ worktree` in a repository nobody has committed to has no baseline, and
the answer is a refusal rather than an invented one.
`CliExecutor::guard_unborn_head` (`crates/turbogit-engine/src/cli.rs`) uses the
existing `has_head` probe and returns `TgError::NoCommitsYet` when a read names
the literal `HEAD` on a root with no commit. The sentence on screen is:

> this repository has no commits yet: there is no first commit to compare your
> changes against

`Git2Executor::diff_patch` (`crates/turbogit-engine/src/git2_exec.rs:1399`)
delegates to the CLI adapter, so both backends refuse in the same words. A read
naming a branch or a sha on such a root still gets git's own error, because that
is a different missing revision, and a surface that offered "make a first commit"
for a ref the user typed themselves would be answering a question nobody asked.
The refusal is a variant and not `TgError::Other` holding a string because a
consumer must recognise the condition without re-reading prose: the keyed read's
per-key failure store renders the text today, and the name is what lets a later
surface turn it into a recovery action instead of a dead sentence.

The pane's route for a failed verdict already exists, so nothing in the UI was
built for this. `AppState::read` answers `Read::Failed(message)` from that store
(`crates/turbogit-app/src/keyed_read.rs`) and `ui/diff/view.rs` paints it as an
inline error under the file header, which is the whole of what a user sees. The
refusal is exactly for reads that name `HEAD`. One surface keeps answering on such
a root: a per-file read of an unversioned path never asks git for a `HEAD` side,
because the synthetic untracked route builds the section from the file's own
bytes, so an untracked file keeps its rows and its stage control, and the
whole-file checkbox in a repository with no commits still works, because staging
goes through intent-to-add and `unstage` has its own HEAD-free command. The
whole-root read behind the root's hunk statistics is a different case and stays
open: `root_caches.rs` folds
its three `diff_patch` calls with `unwrap_or_default()`, so on such a root the
`HEAD` half of that entry is silently empty while the two index-facing halves fill
normally.

The refusal is not a sticky verdict, and that is the property that makes it safe to
show at all. `AppState::refresh` calls `Reads::invalidate`, which drops each kind's
failure together with the values it governed, precisely so a failure cannot keep
reporting itself over a refetch that would now succeed. So the moment a first
commit exists, the same key answers rows.
`an_unborn_head_refuses_the_combined_read_and_shows_the_sentence`
(`crates/turbogit-app/tests/staged_lines_oracle.rs`) commits mid-test and reads
again through the same target to hold that.

The rejected design, and it was built and then removed, was to put git's empty
tree in HEAD's place and answer the read against it, by deriving the OID with
`git hash-object -t tree --stdin` so a SHA-256 repository got its own value. The
hazard is the one a pane cannot show: that answer reports every line of every
staged file as an addition relative to something, and the something is invented to
make a view render, on exactly the repository where the reader knows nothing has
been committed. git's own text at the same command, `fatal: bad revision 'HEAD'`,
is the other wrong answer. A third option, reporting the unborn repository as an
empty diff, only hides the question.

This reverses what this record first carried, and it also corrects the citation
used to justify it. The spec row
`an_unborn_repository_reads_as_nothing_committed_rather_than_as_an_error`
(`docs/spec/rebased-parity.md:872`) is a commit-listing row: it
governs how the log reads an unborn HEAD, which `cli.rs` and `git2_exec.rs` answer
as no history, and it never governed the diff read. Nothing in the spec asked the
preview to answer on an unborn branch.

Pinned by `crates/turbogit-engine/tests/unborn_combined_diff.rs`, which holds the
refusal, the exact sentence, the agreement between both adapters, the committed
case byte-for-byte unchanged against `git diff HEAD`, and the contrast that naming
the empty tree outright still answers every staged line as an addition. On the app
side, `an_unborn_head_refuses_the_combined_read_and_shows_the_sentence`
(`crates/turbogit-app/tests/staged_lines_oracle.rs`) and
`a_repository_with_no_commits_offers_no_rows_to_click`
(`crates/turbogit-app/tests/combined_preview_toggle.rs`) hold the verdict the pane
receives and the absence of rows to click.

## What this costs

- **A repository with no commits shows a sentence where the preview would be.**
  Someone who has just run `git init` gets the refusal named above instead of a
  diff, and line-level staging has nothing to click there until a first commit
  exists. Untracked files keep their own pane, and the whole-file checkbox still
  works, because neither route asks for `HEAD`.
- **No batch staging.** Six line clicks are six `git apply --cached` round trips.
  Ticket 01 measured the median of 30 reps in a temp repository, one file, six `-U3`
  hunks, the index reset between reps, real git 2.54.0: 87.1 ms against 12.8 ms
  batched, a 6.8x ratio, about 14.5 ms per operation, and 95.7 ms with a `git status`
  churner competing for the index lock. Subprocess spawn dominates, so the cost is
  linear in operation count, and `retry_on_transient_lock`
  (`crates/turbogit-services/src/partial.rs:201`) absorbs the lock case. A hunk verb
  is one operation, not two, because each gutter button names one direction and each
  direction has exactly one appliable source.
- **No pre-mutation preview of a selection.** What the user sees before the click is
  the state of the repository, not the state the click will produce.
- **No free Esc undo.** A click writes the index immediately, so undoing means
  reverse-applying what was applied, which is a second operation and a second round
  trip.
- **Retained patch text.** The figures are the 265 KB and 14 MB numbers above, in the
  root caches, for as long as the root's statistics are cached.
- **A row click on half of a replacement pair leaves git reporting `MM`.** Staging
  the `+` of a modification keeps HEAD's line in the index and adds the replacement,
  so the file differs from HEAD and the worktree differs from the index. That is the
  meaning that ships, pinned by
  `staging_one_half_of_a_replacement_pair_leaves_the_file_mm`, because a row click that
  quietly took the pair's other half along would do what the reader did not ask.

## Considered options

- **Answer a `HEAD` read on an unborn branch with the empty tree.** Rejected, and
  it had been built: deriving the repository's own empty-tree OID and diffing
  against it makes the pane answer, and the answer is that every line of every
  staged file is an addition relative to a baseline the repository never had. An
  invented baseline is not a preview of the user's state. See the section above.
- **Keep the comparison selector, fixed to a sane default.** Rejected: the defect was
  three writers, two of them the app. A default changes what the control starts on and
  removes no override.
- **Pin the preview to `index ↔ worktree` instead.** Rejected: `git diff` excludes
  what the user already staged, so a partly staged file shows the reader less than
  half the story and a staged row cannot be unstaged from the pane at all.
- **One combined patch, applied with fuzz or reduced context.** Rejected for the
  measured reason above; `-C1` made the experiment pass.
- **Compose the click from the displayed patch.** Rejected: a subset of a
  `git diff HEAD` patch is the `error: patch failed` case above, and a view whose rows
  moved under the click is the behaviour the program exists to remove.
- **Store staged-ness on the displayed diff value or on the row.** Rejected: the
  `HEAD ↔ worktree` patch carries no fact about the index, so the flag would have no
  source of truth, and a stored answer goes stale the moment git's index moves
  somewhere the app did not read.
- **Fetch the appliable patch per click.** Rejected on the measured cost, one git
  spawn per click at about 14.5 ms, which is the latency the whole feature is judged
  by.
- **Widen `GitExecutor::apply_patch_to_index` to take a list of patches**, so a mixed
  hunk dispatches both directions in one call. Rejected: ADR-0022 keeps one writer and
  one patch shape on purpose, and the two-operation form is already what two clicks
  cost.
- **Make an unstage a hunk verb.** Rejected by re-measurement. One dispatch inside
  this program landed it, and the next replaced it, because it moved changes the
  reader had not clicked while the pane offered one row per click.
- **Keep the pending set as an optional batch mode beside immediate clicking.**
  Rejected: it was an invisible mode. The plan records that a button labelled
  `Stage hunk N` silently changed meaning once a pending row existed, and that
  cross-hunk accumulation never worked because the selection was always one hunk.

## Consequences

- **The pane is a reading view.** It answers "what did I change in this file, and
  which of it is staged" in one stable scroll, and every write it offers is composed
  elsewhere.
- **`index ↔ worktree` survives as a name, and it names two true things.** It is the
  stage direction's source patch, and it is what a `DiffTarget` that names no revision
  still means. What no document may say is that it is the comparison the pane shows.
  That distinction is what the prose of `ui/diff/mod.rs`, `keyed_read.rs` and
  `staged_lines.rs` now keeps straight, and it is the sentence to check whenever a
  comment about the pin looks wrong.
- **Painting reads the oracle, never the displayed patch.** The hunk-header suffix and
  a row's accessibility label both come from `staged_lines`, because for an unmerged
  path `git diff HEAD` prints an ordinary two-sided diff whose added lines are the
  conflict markers, so the displayed value carries no conflict signal at all.
  Conflicted inertness is derived from the oracle's absence of an answer rather than
  special-cased, pinned by `a_row_click_on_a_conflicted_path_does_nothing_at_all`.
- **A rename does not look the same read two ways.** `git diff HEAD -- <renamed file>`
  cannot see the rename, because the pathspec hides the other half, so the pane shows
  the renamed file's lines as plain additions from `/dev/null` while the whole-root
  section is a rename. The per-line answers agree; the shapes do not.
- **Binary and pure-rename sections render no line rows,** so they offer no toggle and
  no per-line answer, and a section present with empty sets answers nothing rather
  than the "staged" an empty added set would imply by accident.
- **ADR-0022 is untouched.** Still one reader of git's diff syntax, one writer, one
  patch shape on the port. `ApplyDirection` was already the port's flag for a reverse
  apply, so the composer change added no method and no format.
- **ADR-0013's other three rules stand unchanged**: `@@` counts computed from the kept
  lines on both sides, appliability answered by the engine, and
  `\ No newline at end of file` a property of the line it qualifies. Its first rule is
  *not* withdrawn; its subject is now named precisely, and rebuilding patches from the
  Display row model stays wrong.
- **ADR-0021 still owns the route.** The pin is a constructor and a key like any other
  target, not a second way to reach a cached value.
- **The vocabulary grew two entries** in `CONTEXT.md`: the per-line staged-ness oracle
  and the appliable source patch, because code now treats both as domain facts rather
  than as module internals.

## Status

Accepted, and landed as the six-ticket `local-changes-preview-one-view` program. The
behaviour is verified: workspace `cargo fmt -- --check` and `cargo clippy
--workspace --all-targets -- -D warnings` exit 0, and the orchestrator's run of
`cargo test --workspace --all-targets --no-fail-fast` on the final tree reported 2168
passing with the one machine failure recorded in the run's STATE file, a
`test-support` identity test that depends on the developer's global git config and is
unrelated to this decision. The `acceptance_matrix` goldens needed no regeneration for the pin, and the reason is
in the fixture: `seeded_repo` leaves one unstaged modification and stages nothing, so
its `git diff HEAD` and its `git diff` print the same bytes. Exactly one PNG is dirty
for the whole program, `crates/turbogit-ui/tests/snapshots/04-diff.png`, and its delta
against `3ebffc4` belongs to ticket 06's muted row bands and to ticket 02's deleted
chip group. This dispatch changed no painting and no logic, and
`cargo check --workspace --all-targets` exits 0 on it.

Revised once after that run closed, and before anything was committed: the
unborn-branch rule first landed as an empty-tree substitution and was reversed to
the refusal recorded above. The gate figures in the paragraph before this one are
the program's, measured with the substitution in the tree, and the reversal has
its own crate-scoped runs in the ticket record.
