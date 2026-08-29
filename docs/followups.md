# Follow-ups

Open items after the DDD subcrate split (issues 01-11, all done; last commit
`932a75b`). Everything in this file is optional work, not a broken build:
`fmt`, `check`, `clippy`, and `test` all pass as of the entry below.

## ADR numbering is broken

`docs/adr/` has **two files numbered 0001**:

- `0001-executor-is-the-seam.md` — `GitExecutor` is the engine seam; no VcsManager façade
- `0001-mockups-single-source-of-truth.md` — mockups are the visual truth

The highest real number is **0015**. Three ADRs — `0008-commit-subtabs-placeholder-panes`,
`0010-advanced-options-inert`, `0012-branch-actions-inert-until-flows` — cite
"ADR-0001" meaning *mockups win; behavior gaps are made explicit*, while
`docs/architecture.md` cites "ADR-0001" meaning the *engine seam*. The same
reference resolves to two different decisions depending on the file.

**Fix:** the mockups ADR belongs at a fresh number (0016), since it was added
late; renumber `0001-mockups-single-source-of-truth.md` → `0016-…` and repoint
those three citations. Don't renumber the engine-seam ADR — it's the original
0001 and the one `architecture.md`'s decision table already points at.

## `architecture.md`'s ADR table is wrong

My docs-cutover rewrite of the decision table introduced two defects:

1. It cites **ADR-0016**, which does not exist.
2. It omits 8 real ADRs — 0002 (embedded JetBrains Mono), 0007, 0008, 0010,
   0011, 0012, 0014, 0015.

That row should be either the real crate-split decision (written up as a new
ADR) or deleted in favour of the pointer to `docs/ddd-subcrate-proposal.md`
already present two lines below it.

## `diff.rs` is the biggest live maintenance issue

`crates/turbogit-ui/src/ui/diff.rs` is **2,606 lines** — up from 2,490 when the
proposal measured it, because image/binary diff landed after. One file holds
four unrelated concerns:

| Concern | Entry points |
|---|---|
| Row-model parsing | `parse`, `hunk_starts`, `Row`, `RowKind` |
| Public diff API | `RowSummary`, `parsed_rows` |
| Pane assembly + async loading | `DiffModel`, `build_model`, `ensure_diff`, `fetch_side`, `decode_image` |
| Virtualized painting | `render_diff`, `hunk_needs_scroll`, `paint_selection_bar`, `image_cell` |

Split it into submodules *inside* `turbogit-ui` (`diff/{model,actions,panes,view}.rs`).
Cheap, no Cargo churn. Keep `ui::diff::parsed_rows` resolvable — the root
cross-layer suite `tests/diff_parity.rs` imports it directly and compares engine
diff text against it, so that path is load-bearing. Do **not** extract a
standalone diff-model crate; `diff_parity` is the only second consumer and it
lives in the same workspace.

## Historical docs need a status header

`docs/ddd-subcrate-proposal.md` and `docs/MIGRATION_PLAN.md` describe the
pre-split single-crate codebase. I left their `src/core` / `src/engine` paths
alone deliberately — those old paths *are* the subject of the documents — but a
reader who lands on the proposal sees a plan that already executed. A one-line
"superseded; completed <date>" header on each removes the confusion.

## Workspace root hygiene

Untracked clutter sitting next to `Cargo.toml`: `build.log`, `check.log`,
`test.log`, `fmt.log`, `acceptance_rerun.log`, plus stray dirs `tmpwtdebuglinked`,
`tmpwtdebugrepo`, `%TEMP%`. All are already gitignored, so this is a `rm`, not a
tracked-files change.

## Deferred by design

Recorded in `docs/architecture.md` and `AGENTS.md` as choices, not oversights:

- Executor injection into `AppState` — the one sanctioned impurity;
  `turbogit-app → turbogit-engine` via `build_executor` at
  `crates/turbogit-app/src/state.rs:402,466,517`
- Splitting `GitExecutor` into capability traits
- Decomposing UI state out of `AppState`

None is a prerequisite for anything else.

---

**Verified 2026-08-29:** `cargo fmt -- --check`, `cargo check --workspace
--all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace --all-targets` — 267 tests, 0 failures.
