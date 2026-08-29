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
- `crates/turbogit-app` — `AppState` (worker dispatch, `run_git`, the event
  pump), `events`, `root_caches`, `persistence`, `recents`, the diff plain-data
  types, and the `granular` staging orchestrator.
- `crates/turbogit-ui` — `theme` and every presentation module under `ui/`.
- `crates/test-support` — the headless harness and kittest helpers, consumed as
  a dev-dependency.
- `src/` — composition root only: `main.rs` plus `lib.rs` and `app.rs`
  (`TurbogitApp` eframe wiring).
- `tests/` — the root's single cross-layer suite (`diff_parity`).
- `docs/` — product spec, ADRs, architecture notes.
- `research/` — competitive research and UX findings.
- `turbogit-screens/` — visual assets/screenshots.

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
- `cargo clippy --workspace --all-targets -- -D warnings` — lint with warnings as errors.
- `cargo test --workspace --all-targets` — run all unit and integration tests.

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
