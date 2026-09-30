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
deny-warnings via `[lints] workspace = true`.

### One sanctioned impurity

`AppState` calls `turbogit_engine::build_executor` (in `launch_in` and
`rebuild_executor`), so `turbogit-app` depends on `turbogit-engine`, not only
on the port. The textbook fix is injecting `Arc<dyn GitExecutor>` from the
composition root, but that would change constructor signatures used by ~20 test
call sites for no current benefit. Treat this edge as the single composition
touchpoint; revisit it only if the app must run against the fake executor.

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
  later run fail. `interactive_rebase_editor.rs` fails ~3 of 5 parallel runs from a
  temp-filename race — not your change.

All four gates must pass; a failure in any one blocks the change.

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

Use concise, lowercase summaries, often prefixed as `docs:` or another
Conventional Commits category; keep subjects imperative and under about 72
characters.

## Agent skills

### Issue tracker

Issues and specs live as local markdown under `.scratch/<feature-slug>/` (the GitHub remote exists but the `gh` CLI is not assumed). Each feature directory holds an `execution-plan.md` plus one `issues/NN-<slug>.md` per ticket.

### Triage labels

The five canonical triage roles use their default label strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`), recorded as a `Status:` line in each issue file.

### Domain docs

Single-context: `CONTEXT.md` at the repo root plus `docs/adr/`.
