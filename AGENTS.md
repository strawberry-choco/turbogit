# Repository Guidelines

## Project Structure & Module Organization

TurboGit is a Rust workspace-free binary/library crate built with `eframe`/`egui`.

- `src/lib.rs` — public module root shared by the binary and integration tests.
- `src/core/` — repository services and domain logic (branches, history, sync, conflicts, multi-root).
- `src/engine/` — Git execution layer; mutating operations use the Git CLI, with optional read-only acceleration via the `gix-reader` feature.
- `src/ui/` — egui windows, dialogs, diffs, and dockable panels.
- `tests/` — phase-based integration tests (`phase0.rs` … `phase4.rs`).
- `docs/` — product spec, execution plan, progress tracker, and UI backlog.
- `research/` — competitive research and UX findings.
- `turbogit-screens/` — visual assets/screenshots.

## Build, Test, and Development Commands

- `cargo check` — fast type/borrow checking.
- `cargo build` — compile the application.
- `cargo run` — launch the desktop app locally.
- `cargo test` — run all unit and integration tests.
- `cargo test --test phase2` — run one phase suite.
- `cargo fmt` — format all Rust code.
- `cargo clippy --all-targets -- -D warnings` — lint with warnings as errors.
- `cargo test --features gix-reader` — exercise tests with the optional Gix read path enabled.

## Coding Style & Naming Conventions

Use idiomatic Rust and rustfmt defaults (4-space indentation). Name modules and functions with `snake_case`, types with `UpperCamelCase`, constants with `SCREAMING_SNAKE_CASE`, and files after their primary type or module. Keep Git mutations in the engine layer rather than calling CLI commands directly from UI code. Preserve existing error handling patterns in `src/error.rs`.

## Testing Guidelines

Prefer headless integration tests that create temporary repositories with `tempfile` and require `git` on `PATH`. Follow the existing `<area>_<behavior>` naming style, for example `phase0_single_root_status_scan`. Add new suites to `tests/` by phase or cohesive area, and update the relevant checklist in `docs/progress-tracker.md`.

## Commit & Pull Request Guidelines

Recent history uses concise, lowercase summaries, often prefixed as `docs:` or another Conventional Commits category; keep subjects imperative and under about 72 characters.

Pull requests should include:

- A short description of behavior changes and rationale.
- Linked issue or spec section when applicable.
- Tests or manual verification steps.
- Screenshots/GIFs for visible UI changes.
