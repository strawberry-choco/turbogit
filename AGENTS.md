# Repository Guidelines

## Project Structure & Module Organization

TurboGit is a Cargo workspace built with `eframe`/`egui`: one crate per layer,
plus a thin root composition root.

- `crates/turbogit-domain` — `model` and `error`. The leaf; depends only on
  `serde`, `chrono`, `thiserror`.
- `crates/turbogit-engine-api` — the `GitExecutor` trait (the only git
  boundary) and `ApplyDirection`. Depends on domain only.
- `crates/turbogit-engine` — the two adapters (`cli`, `git2_exec`), the
  backend-selecting `build_executor` factory, and `fake` behind the `test-util`
  feature.
- `crates/turbogit-services` — pure domain services over the port: branch,
  sync, history/editor, integrate, changes, partial, diff engine, conflict,
  shelve/stash, multi-root.
- `crates/turbogit-app` — `AppState` (the dispatch seam, the event pump),
  `operation` (the `Operation` dispatch unit), `keyed_read` (the one rule by
  which a surface reaches a cached git value: `read` answers and admits, `peek`
  answers only), `shell_reads` (the named git reads that are not cached values),
  `events`, `root_caches`, `persistence`, `recents`, `diff_data` (plain
  diff-pane types), `diff_model` (the egui-free display model the diff read
  answers with), `diff_load` (the pane's byte sourcing and decode limits),
  `log_display` (the held log-window display model and its lane walk), and the
  `granular` staging orchestrator.
- `crates/turbogit-ui` — `theme` and every presentation module under `ui/`.
- `crates/test-support` — the headless harness and kittest helpers, consumed as
  a dev-dependency.
- `src/` — composition root only: `main.rs` plus `lib.rs` and `app.rs`
  (`TurbogitApp` eframe wiring).
- `tests/` — the root's single cross-layer suite (`diff_parity`).
- `docs/` — product spec, ADRs, architecture notes.
- `research/` — competitive research and UX findings.
- `crates/turbogit-ui/tests/snapshots/` — the per-page PNG renders written by
  `acceptance_matrix.rs` (egui_kittest's default snapshot directory).

Dependencies flow strictly downward: domain ← engine-api ← {engine, services}
← app ← ui, with the root composition root above ui. Every shared version lives
in `[workspace.dependencies]` in the root manifest; members inherit clippy
deny-warnings via `[lints] workspace = true`. `crates/commit-lint` is tooling,
not a layer: the `crates/*` glob pulls it into the workspace, and nothing in
the layer stack may depend on it.

### One sanctioned impurity

The impurity is `turbogit-app` reaching `turbogit-engine` instead of only the
port, and it has two edges. `AppState` constructs its own executor —
`turbogit_engine::build_executor` in `state.rs`, from `with_flat_state` (behind
`launch_in` and `for_roots`) and from `rebuild_executor`. And
`turbogit_engine::resolve_git_version` is reached at three sites: re-exported
from the app's `lib.rs`, read by that same `with_flat_state`, read by
`git_env_check`. Patch parsing is **not** one of them any more — the unified-diff
reader lives in `turbogit-engine-api` (ADR-0022), so those calls are the legal
`app → port` edge. The textbook fix is injecting `Arc<dyn GitExecutor>` from the
composition root, but that would change constructor signatures used by ~20 test
call sites for no current benefit; a test that wants another executor swaps one
in after construction through `AppState::with_executor`. Treat these two edges as
the whole impurity — the only places an app-side call may reach the adapters.

### Deferred options (do not schedule)

- Executor injection into `AppState`
- Splitting `GitExecutor` into capability traits
- Decomposing UI state out of `AppState`

All are choices to make once the layer boundaries exist; none is a prerequisite.

## Build, Test, and Development Commands

### Quality Gates (must pass before any commit or PR)

- `cargo fmt -- --check` — verify formatting without modifying files.
- `cargo check --workspace --all-targets` — type/borrow checking across all targets.
- `cargo clippy --workspace --all-targets -- -D warnings` — lint with warnings as
  errors. Add `--keep-going` to count violations; it aborts a crate on its first
  error batch, so a partial run undercounts.
- `cargo test --workspace --all-targets --no-fail-fast` — run all unit and
  integration tests. Redirect output to a file; never pipe it into `head`/`tail`,
  because SIGPIPE kills git mid-write and leaves a `config.lock` that makes every
  later run fail. The UI's `interactive_rebase_editor.rs` fails when the machine is
  busy, because `wait_until` polls a spawned git worker against a wall-clock
  deadline — not from a shared temp name, since every test gets its own
  `tempfile::tempdir()`. Re-run the failing target alone before reading a red there
  as a defect, and never write a failure off as known flakiness without that re-run.
- The `commits` CI job — the fifth gate, CI-side rather than local: it lints the
  PR's commit range (and pushes to `main` as a backstop) with the repo's
  `cog.toml`, through the same cocogitto parser the commit-msg hook runs
  (ADR-0028). Nothing to run by hand; the hook is its local counterpart.

All five gates must pass; a failure in any one blocks the change.

### Other Commands

- `cargo build` — compile the application.
- `cargo run` — launch the desktop app locally.

## Coding Style & Naming Conventions

Use idiomatic Rust and rustfmt defaults (4-space indentation). Name modules and
functions with `snake_case`, types with `UpperCamelCase`, constants with
`SCREAMING_SNAKE_CASE`, and files after their primary type or module. Services
call git only through the `GitExecutor` trait; UI code never calls the CLI. Keep
git mutations in the engine layer. Preserve the `TgError` / `TgResult` error
patterns defined in `crates/turbogit-domain/src/error.rs`.

### Comments

Production code is ~24% comment lines, so the three genres are policed rather
than tolerated:

- **Write in full** — a hazard the code cannot show: a behavioural difference
  between two call sites, a field order that compiles and passes either way, a
  library default that bites.
- **Compress to one line** — "why I didn't extract this" / "why I didn't merge
  these". The load-bearing fact is one sentence.
- **Delete** — narration of the refactor itself ("the rows are literals", "the
  six tests reduced to this"), changelogs enumerating which suite now owns which
  dropped assertion, and ticket references. Git has the history.

## Design system

The design system is `crates/turbogit-ui/src/theme.rs`: every shared presentation
token (surfaces, ink, severity, radius, spacing, type scale, fonts) is defined,
consumed and changed there, not at call sites. The shared widgets that paint
those tokens are `crates/turbogit-ui/src/ui/components.rs` and `ui/widgets.rs`.
`docs/design-system-roles.md` records which name owns which role, and
`crates/turbogit-ui/tests/` (`design_tokens.rs`, `widget_library.rs`,
`branch_component_kit.rs`) pins these contracts.

## Testing Guidelines

Prefer headless integration tests that create temporary repositories with
`tempfile` and require `git` on `PATH`, drive the real `AppState` plus a fake or
CLI engine, and pump events through the production path. Snapshot and kittest
helpers live in `crates/test-support`; enable its `harness` feature from
dev-dependencies. Suites live in the crate they exercise — `crates/<crate>/tests/` —
named `<area>_<behavior>`. Only suites that must span every crate belong in the
root `tests/`.

## Commit & Pull Request Guidelines

Every commit subject follows `type(scope)!: subject`: one type verb, an
optional free-form scope, a subject. Types and scopes are conventions, not an
enforced enumeration, and both breaking notations are legal — `!` after
type/scope, or a `BREAKING CHANGE:` footer. The commit-msg hook and the CI
`commits` job both enforce it through cocogitto's parser — one parser, per
ADR-0028. Subjects stay imperative and lowercase at about 72 characters; that
is house style, not something the parser polices.

PRs merge by rebase, so what lands on `main` is what was written and linted —
every main commit is a lint target. The merge commits already in history are
exempt; the tools skip them by default.

## Agent skills

### Issue tracker

Issues and specs live as local markdown under `.scratch/<feature-slug>/` (the GitHub remote exists but the `gh` CLI is not assumed). Each feature directory holds an `execution-plan.md` plus one `issues/NN-<slug>.md` per ticket.

### Triage labels

The five canonical triage roles use their default label strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`), recorded as a `Status:` line in each issue file.

### Domain docs

Single-context: `CONTEXT.md` at the repo root plus `docs/adr/`.
