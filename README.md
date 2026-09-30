# TurboGit

Desktop Git client built in Rust with `eframe`/`egui`. Modeled after IntelliJ IDEA's Git integration, with first-class multi-repository (multi-root) support.

In-process libgit2 (`git2`) as the primary backend with the system `git` CLI as fallback for sync/credential operations.

## Build and run

```sh
cargo run
```

Four gates must pass before any commit or PR (full rules in `AGENTS.md`):

```sh
cargo fmt -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets --no-fail-fast
```

## Crate map

One crate per layer, dependencies flowing strictly downward:

```
domain ← engine-api ← {engine, services} ← app ← ui
```

with a thin composition root on top (`src/`: `main.rs`, `lib.rs`, `app.rs`).
The UI layer never calls git directly — all git work crosses the `GitExecutor`
seam. Per-crate responsibilities are listed in `AGENTS.md`.

## Docs

- `docs/architecture.md` — the layer boundaries and the `GitExecutor` seam.
- `docs/adr/` — 27 architecture decision records.
- `docs/design-system-roles.md` — which design-system name owns which role.
