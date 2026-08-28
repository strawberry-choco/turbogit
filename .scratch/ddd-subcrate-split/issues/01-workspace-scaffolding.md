# 01 — Workspace scaffolding

**What to build:** The repo becomes a Cargo workspace without any code moving. The root manifest gains a `[workspace]` with `crates/*` members, a `[workspace.dependencies]` table that is the single source of truth for every shared dependency version, and `[workspace.lints]` carrying the clippy deny-warnings configuration that member crates inherit via `[lints] workspace = true`. Empty crate directories are created under `crates/`. The root package itself is untouched — the app builds and behaves exactly as before.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] Root manifest declares `[workspace]`, `[workspace.dependencies]`, and `[workspace.lints]`
- [ ] Existing root package dependencies are repointed at the workspace table (no duplicated version strings)
- [ ] All four quality gates pass: `cargo fmt -- --check`, `cargo check --all-targets`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all-targets`
