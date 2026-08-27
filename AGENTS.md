# Repository Guidelines

## Project Structure & Module Organization

TurboGit is a Rust binary/library crate built with `eframe`/`egui`.

- `src/lib.rs` — public module root shared by the binary and integration tests.
- `src/core/` — repository services and domain logic (branches, history, sync, conflicts, multi-root).
- `src/engine/` — Git execution layer; in-process operations use `git2` (libgit2) with the git CLI as fallback for sync/credential ops and other cases libgit2 can't handle.
- `src/ui/` — egui windows, dialogs, diffs, and dockable panels.
- `tests/` — integration tests organized by area/phase.
- `docs/` — product spec, ADRs, and architecture notes.
- `research/` — competitive research and UX findings.
- `turbogit-screens/` — visual assets/screenshots.

## Build, Test, and Development Commands

### Quality Gates (must pass before any commit or PR)

- `cargo fmt -- --check` — verify formatting without modifying files.
- `cargo check --all-targets` — type/borrow checking across all targets.
- `cargo clippy --all-targets -- -D warnings` — lint with warnings as errors.
- `cargo test --all-targets` — run all unit and integration tests.

All four gates must pass; a failure in any one blocks the change.

### Other Commands

- `cargo build` — compile the application.
- `cargo run` — launch the desktop app locally.

## Coding Style & Naming Conventions

Use idiomatic Rust and rustfmt defaults (4-space indentation). Name modules and functions with `snake_case`, types with `UpperCamelCase`, constants with `SCREAMING_SNAKE_CASE`, and files after their primary type or module. Keep Git mutations in the engine layer rather than calling CLI commands directly from UI code. Preserve existing error handling patterns in `src/error.rs`.

## Testing Guidelines

Prefer headless integration tests that create temporary repositories with `tempfile` and require `git` on `PATH`. Follow the existing `<area>_<behavior>` naming style for test functions. Add new suites to `tests/` by area or phase.

## Commit & Pull Request Guidelines

Use concise, lowercase summaries, often prefixed as `docs:` or another Conventional Commits category; keep subjects imperative and under about 72 characters.
