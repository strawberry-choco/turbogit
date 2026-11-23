# TurboGit

Desktop Git client built in Rust with Native Shell (nsh) 0.5.x, rendering with
each desktop's own controls (AppKit, Win32/Direct2D, GTK 4). Modeled after
IntelliJ IDEA's Git integration, with first-class multi-repository (multi-root)
support.

In-process libgit2 (`git2`) as the primary backend with the system `git` CLI as
fallback for sync/credential operations.

## Build and run

```sh
cargo run
```

The nsh app and its release bundles live in `crates/turbogit-ui`:

```sh
cd crates/turbogit-ui
nsh run --target macos
nsh package --target macos   # windows: .zip, linux: native tarball
```

Six gates must pass before any commit or PR (full rules in `AGENTS.md`):

```sh
cargo fmt -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets --no-fail-fast
cd crates/turbogit-ui && nsh check
# the sixth is the CI `commits` job
```

## Crate map

One crate per layer, dependencies flowing strictly downward:

```
domain ← engine-api ← {engine, services} ← app ← ui
```

with a thin composition root on top (`src/main.rs`). `crates/turbogit-ui` is the
nsh presentation crate: `src/ui/` controls, `src/screens/` surfaces,
`theme.json` and `src/theme.rs` for tokens, and `launch` for the native
backends. The UI layer never calls git directly, all git work crosses the
`GitExecutor` seam. Per-crate responsibilities are listed in `AGENTS.md`.

## Docs

- `docs/architecture.md`, the layer boundaries and the `GitExecutor` seam.
- `docs/adr/`, the architecture decision records.
- `docs/design-system-roles.md`, which design-system name owns which role.
