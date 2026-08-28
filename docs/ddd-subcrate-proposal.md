# TurboGit DDD Subcrate Split — Architectural Proposal

> Single-crate app → DDD-aligned Cargo workspace. Grounded in inspection of the current source layout.

**Note:** the layout below reflects the actual file organization in the repo.

---

## 0. What the code actually looks like today

The good news: this codebase is **already layered**, just not *enforced*.

| Observation | Evidence |
|---|---|
| One clean seam exists | `GitExecutor` trait (`engine/mod.rs:114`) — ~60 methods, depends only on `model` types + `TgResult`. Two adapters (`CliExecutor`, `Git2Executor`) + factory `build_executor` (`engine/mod.rs:345`) |
| Core services are nearly pure | 10 of 11 `core/*` modules import only `engine::GitExecutor`, `error`, `model` |
| Exactly one service violates purity | `core/granular.rs` takes `&mut AppState` (`granular.rs:91,180,199,269,300`) |
| State reaches *up* into UI | `state.rs` references `crate::ui::diff::{PaneCache, PaneEntry, PaneSide}` and `crate::ui::hunk_nav::Dir` — an inverted dependency |
| One clean seam exists | `GitExecutor` trait — methods depend only on `model` types + `TgResult`. Two adapters plus a factory |
| Core services are nearly pure | Most `core/*` modules import only `engine::GitExecutor`, `error`, `model` |
| Exactly one service violates purity | `core/granular.rs` takes `&mut AppState` |
| State reaches up into UI | `state.rs` references types from `ui/diff` and `ui/hunk_nav` — an inverted dependency |
| Event bus is misplaced | `AppEvent` and related types live inside the engine module even though the engine trait never mentions them |
| Error type drags RON everywhere | `TgError::Serde(#[from] ron::Error)` pulls RON into the domain layer |
---

## 1. Proposed workspace layout

```
turbogit/
├── Cargo.toml                  # [workspace] + root package "turbogit" (binary, composition root)
├── src/
│   ├── main.rs                 # unchanged (32 lines)
│   ├── app.rs                  # TurbogitApp: eframe wiring, rfd picker injection
│   └── lib.rs                  # thin: pub use turbogit_app::…; pub use turbogit_ui::…
├── tests/                      # CROSS-LAYER suites only (diff_parity)
└── crates/
    ├── turbogit-domain/
    ├── turbogit-engine-api/
    ├── turbogit-engine/
    ├── turbogit-services/
    ├── turbogit-app/
    ├── turbogit-ui/
    └── test-support/
```

Root `Cargo.toml` becomes both `[package] turbogit` and `[workspace] members = ["crates/*"]` — the root package stays put so nothing breaks on day one.

### Member table (dependency order, leaves first)

| Crate | Bounded context / responsibility | Modules moving in | Depends on |
|---|---|---|---|
| **turbogit-domain** | Domain model + shared vocabulary. Pure data + invariants | `model.rs`, `error.rs` | `serde`, `chrono`, `thiserror` *(see §2 on `ron`)* |
| **turbogit-engine-api** | The **port**: the only contract between services and git execution | `GitExecutor` trait, `ApplyDirection` (from `engine/mod.rs`) | `turbogit-domain` |
| **turbogit-engine** | **Adapters**: CLI process spawning, libgit2, backend selection | `engine/cli.rs`, `engine/git2_exec.rs`, `build_executor`; `fake.rs` behind `#[cfg(feature = "test-util")]` | `turbogit-engine-api`, `turbogit-domain`, `git2` |
| **turbogit-services** | Application/domain services: use-cases over the port, no state ownership | `core/{branch_service, changes, conflict, diff_engine, history_editor, history_service, integrate_service, multi_root, partial, shelve_stash, sync_service}` — **not** `granular` | `turbogit-engine-api`, `turbogit-domain`; dev: `turbogit-engine` (features `test-util`) |
| **turbogit-app** | Application layer: state container, event pump, caches, app-scoped persistence | `state.rs`, `granular.rs` (moved out of core), `root_caches.rs`, `persistence.rs`, `recents.rs`, plus `events` module receiving `AppEvent`/`DecodedImage`/`FetchedBlob` out of `engine/mod.rs` | `turbogit-domain`, `turbogit-engine-api`, `turbogit-services`, `crossbeam-channel`, `ron`, `dirs`, `serde`; **one flagged edge**: `turbogit-engine` for `build_executor` (§3) |
| **turbogit-ui** | Presentation/shell: egui rendering, design tokens, dialogs, diff viewer | `theme.rs`, all of `ui/*` | `egui`, `egui-dock`, `egui-extras`, `image`, `nucleo-matcher`, `crossbeam-channel`, `turbogit-app`, `turbogit-services`, `turbogit-engine-api`, `turbogit-domain` |
| **test-support** | Headless harness extracted from `tests/common/mod.rs`: kittest helpers + `RecordingExecutor` | — | `egui`, `egui-kittest`, `tempfile`, `turbogit-app`, `turbogit-ui`, `turbogit-engine-api` |
| **turbogit** (root) | Composition root: binary + cross-layer integration tests | `main.rs`, `app.rs` | everything above + `eframe`, `rfd` |

Note what did **not** become a crate: `theme` (presentation, lives in `turbogit-ui`), `root_caches`/`persistence`/`recents` (small, app-owned; separate crates would be ceremony without a second consumer — YAGNI).

---

## 2. Shared types, errors, and the engine seam

### Shared types → `turbogit-domain`

All of `model.rs` moves verbatim: git entities (`RootId`, `Root`, `Branch`, `Commit`, `Change`, `Stash`, `Worktree`, `BlameLine`…), op vocab (`DiffOpts`, `LogOpts`, `MergeOpts`, `RebaseOpts`, `RebasePlanEntry`), and settings (`VcsSettings`, `GitBackend`, `ProjectState`, `DirMapping`, `Vcs`). It already has exactly the right shape: `serde` + `chrono`, zero behavioral deps. Keep the internal module names (`pub mod model; pub mod error;`) so compatibility shims can preserve existing paths (§4).

### Errors → `turbogit-domain`

`TgError`/`TgResult` travel with the domain. One wrinkle: `Serde(#[from] ron::Error)` pulls `ron` into the domain crate. Since `persistence.rs` already converts RON failures manually, verify whether the `#[from]` variant has any constructor sites; if not (or trivially few), change it to `Serde(String)` and keep `ron` confined to `turbogit-app`. Micro-task, do it during Phase 2.

### Engine abstraction split

Three-way split along the existing seams:

1. **Port** (`turbogit-engine-api`): the `GitExecutor` trait + `ApplyDirection`. Keep the trait **whole** — do not slice it into capability traits now. The default-method pattern already used for `ref_decorations`/`commit_files`/`is_repo` is what keeps the 60-method surface survivable.
2. **Adapters** (`turbogit-engine`): `CliExecutor`, `Git2Executor` (which internally delegates fallbacks to a held `CliExecutor` — hence one crate, not two), `build_executor` factory, and `fake.rs` behind a `test-util` feature consumed via dev-dependencies.
3. **Messaging** (`turbogit-app::events`): `AppEvent`, `DecodedImage`, `FetchedBlob` move **out of the engine**. The trait never references them; their producers are the app's worker threads and `ui/diff.rs`. This also fixes the current oddity where `engine/mod.rs:22` re-exports `root_caches::Affected` — after the split, `Affected` lives in `turbogit-app` beside its consumer, and the engine-api crate gains no dependency on app (which would be a cycle).

One deliberate consequence: image decoding (`image::load_from_memory`, `ui/diff.rs:759`) stays in the UI crate; the engine only ships raw bytes (`show_file_bytes`). The `image` dependency never enters engine or domain.

---

## 3. Dependency rules and how they're enforced

The enforcement mechanism is **Cargo itself**: a rule is real when the dependent crate doesn't declare the dependency, making the violation a compile error.

| # | Rule | Enforcement |
|---|---|---|
| D1 | `turbogit-domain` imports nothing but `serde`/`chrono`/`thiserror` | Its `Cargo.toml` has no other deps — egui/git2 imports impossible |
| D2 | `turbogit-engine-api` depends only on domain; knows nothing of processes, git2, channels, or events | Dep list |
| D3 | Only `turbogit-engine` and the root binary may name `CliExecutor`/`Git2Executor`/`build_executor` | `turbogit-ui` and `turbogit-services` don't declare `turbogit-engine` (except services' *dev*-dep for the fake). Belt-and-braces: `clippy::disallowed_types` entries in ui/app `clippy.toml` |
| D4 | Services depend on the **port**, never adapters, never egui, never `AppState` | Dep list; the `granular` exception is resolved by relocating it (§5) |
| D5 | `turbogit-app` contains no egui types | Dep list (plain-data pane/hunk types move *down* into app first — see Phase 3). Keeps headless tests cheap |
| D6 | UI never spawns git processes and never persists directly; it mutates through `AppState` methods and service functions over `&dyn GitExecutor` | D3 + review |
| D7 | Dependencies flow strictly downward: domain ← engine-api ← {services, engine} ← app ← ui ← binary | The member table above *is* the DAG |

**The one sanctioned impurity:** `AppState::launch_in` calls `engine::build_executor` (`state.rs:394,458`), so `turbogit-app → turbogit-engine` is a real edge. The textbook fix is injecting `Arc<dyn GitExecutor>` from the composition root, but that changes constructor signatures used by ~20 test call sites for zero current benefit. Recommendation: accept the edge now, document it as the single composition touchpoint, and revisit only if you ever need to run the app against the fake.

---

## 4. Phased migration plan (green gates throughout)

Every phase ends with all four gates passing: `cargo fmt -- --check`, `cargo check --all-targets`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all-targets`.

Technique throughout: **extract crate → leave `pub use` shims in the old paths → keep tests compiling untouched → repoint imports in a later mechanical pass → delete shims last.**

### Phase 0 — Scaffolding (no code moves)
Root `Cargo.toml` gains `[workspace]`, `[workspace.dependencies]` (single source for `egui = "0.36.1"` etc.), and `[workspace.lints]` (clippy deny-warnings, inherited via `[lints] workspace = true`). Create empty `crates/` dirs. Gates green trivially.

### Phase 1 — `turbogit-domain`
Move `model.rs`, `error.rs` into `crates/turbogit-domain/src/{model,error}.rs`. Root `lib.rs`: `pub use turbogit_domain::{error, model};` — every `turbogit::model::X` path in ~20 test files keeps resolving. Zero downstream edits.

### Phase 2 — Invert the state→UI leak (intra-crate prep)
Before any crate boundary touches it: move `PaneCache`/`PaneEntry`/`PaneSide`/`from_blob` out of `ui/diff.rs` and `Dir`/`EDGE_WINDOW` out of `ui/hunk_nav.rs` into a plain-data module next to `state.rs`. `ui/diff.rs` imports them back up. Behavior-neutral, guarded by `redesign_diff`/`diff_parity`. This is what makes Phase 6 (egui-free app crate) possible. Also do the `TgError::Serde(String)` micro-task here.

### Phase 3 — `turbogit-engine-api`
Extract trait + `ApplyDirection`. `src/engine/mod.rs` shrinks to: `pub use turbogit_engine_api::{GitExecutor, ApplyDirection};` plus remaining items. Drop the `Affected` re-export; fix its importers. Tests untouched.

### Phase 4 — `turbogit-engine`
Move `cli.rs`, `git2_exec.rs`, `build_executor`; expose `fake` behind `#[cfg(feature = "test-util")]`. Shims: `pub use turbogit_engine::{cli, git2_exec};` under `src/engine/`. Move `engine_golden`, `push_dry_run`, `partial_stage_cli` to `crates/turbogit-engine/tests/`.

### Phase 5 — `turbogit-services`
Move the 11 pure core modules (all except `granular`). Their unit tests come along; dev-dep `turbogit-engine = { features = ["test-util"] }` satisfies `FakeExecutor`. Shim in root: `pub use turbogit_services as core;`. Move `sync_outgoing` → `turbogit-services/tests/`.

### Phase 6 — `turbogit-app`
Move `state.rs`, `granular.rs` (from core — see §5), `root_caches.rs`, `persistence.rs`, `recents.rs`, and the event types from `engine/mod.rs` into `turbogit-app::{state, granular, root_caches, persistence, recents, events}`. Root shims preserve `turbogit::{state, root_caches, persistence, recents}` paths; `core::granular` shim repoints to `turbogit_app::granular`. Move `granular_ops`, `partial_dispatch` → `turbogit-app/tests/`.

### Phase 7 — `turbogit-ui` + `test-support`
Big move: `theme.rs` + all `ui/*`. Extract `tests/common/mod.rs` into `crates/test-support`. Move the 17 `redesign_*` suites to `turbogit-ui/tests/`.

**Gotchas:** (a) `redesign_acceptance.rs` writes PNGs into `turbogit-screens/redesign/` via a manifest-relative path — the `../..` depth changes, update the constant and regenerate; (b) `egui-kittest` (wgpu/snapshot features) becomes a dependency of only `test-support` + `turbogit-ui` dev-deps — a real compile-time win for everyone else.

Optionally precede this phase with the internal `ui/diff.rs` module split (§5) while it's still one crate.

### Phase 8 — Composition root + shim removal
Root crate retains `main.rs`, `app.rs`, and `tests/diff_parity.rs` (the one true cross-layer suite: engine text vs `ui::diff::parsed_rows` — it needs deps on everything, which only the root has). Now do the mechanical import repoint (`turbogit::model::X` → `turbogit_domain::model::X`, etc.) across remaining stragglers, **delete every shim**, and confirm the gates. Update `AGENTS.md` (its test-layout description is already stale), `docs/progress-tracker.md`, and spec references to module paths.

**Deferred (do not schedule):** executor injection into `AppState` (§3), capability-trait split, decomposing `UiState` out of `AppState`. All are options *after* the boundaries exist, none are prerequisites.

---

## 5. Codebase-specific risks and trade-offs

### The 95KB `ui/diff.rs` (2,490 lines)

It conflates four concerns: a pure row-model parser (`parsed_rows`/`RowSummary` — independently tested by `diff_parity`), patch composition for gutter staging, async loading/cache keying, and virtualized painting. Risk assessment: the *move* is mechanical and safe; the *size* is a maintainability problem independent of crate structure. Recommendation: split it into submodules (`diff/{model,actions,panes,view}.rs`) **within** the ui crate before or shortly after Phase 7 — cheap, no Cargo churn, preserves the public `ui::diff::*` paths that `state.rs` and two test suites rely on. Do **not** extract a standalone diff-model crate; there's no second consumer (YAGNI).

### The 39KB `state.rs` (887 lines) — where does it live?

Unequivocally the application layer: it owns the executor handle, the event channel, cache invalidation policy, and op dispatch — and `drain_events` writes into `UiState`, so it is inseparable from UI ephemeral state today. It belongs in `turbogit-app`, whole. Resist the temptation to shard `AppState` fields across crates: every UI function takes `&mut AppState`, so field-level sharding means a borrow-checker war across crate boundaries for no architectural gain. Optional later step: split the *file* into `app/{state,events,ops}.rs` modules. The inverted `ui::diff`/`ui::hunk_nav` type references are handled by Phase 2's downward move.

### The state↔services circular-ish need

Concretely: `granular.rs` alone. It reads selection/comparison state, composes patches via `partial`, dispatches through `run_git`, and owns settlement hooks called *from* `drain_events` (`state.rs:602,843,848`). Options considered: (a) parameterize it into a pure resolver returning an op descriptor — cleanest, but touches the dispatch/settle protocol and its three tests; (b) define a narrow `GranularContext` trait implemented by `AppState` — indirection without payoff at this size. **Recommended: (c) relocate it to `turbogit-app`** and document it as an *application service* (orchestration), not a domain service. The other ten services stay pure. Revisit (a) only if more services grow state appetites.

### Is a `turbogit-kernel` facade warranted?

**No — as a permanent crate.** The binary is already 32 lines; thinness comes from the dependency DAG, not a re-export hub. A facade would (1) hide exactly the boundaries this restructuring exists to enforce, (2) let violations creep back through the convenience path, (3) add a crate every change must touch. What you *do* want is the **temporary shim strategy** from §4 — same ergonomic benefit during migration, explicitly deleted at the end. If IDE ergonomics ever demand a single import surface, a `prelude` module inside `turbogit-ui`/`turbogit-app` is the harmless version of the idea.

### Other risks

- **Trait-width tax:** adding one `GitExecutor` method today edits `cli.rs`, `git2_exec.rs`, `fake.rs`, and `RecordingExecutor` (the 600-line `common/mod.rs` is the receipt). The split doesn't fix this; default methods do. Rule of thumb: new optional capabilities land as defaulted trait methods.
- **Snapshot/output fragility:** only `redesign_acceptance` writes images, but its relative path breaks on the move (Phase 7 gotcha).
- **Feature unification:** `test-util` on `turbogit-engine` activated via dev-deps won't leak into release builds — verify with `cargo check --all-targets` vs plain `cargo check` in the gates.
- **Compile-time expectations:** the split is *not* primarily a build-speed play — dependents still rebuild. Real wins: parallel crate compilation, incremental caching when editing leaves (domain/services changes no longer relink the kittest suites), and isolating `egui-kittest`/wgpu and `git2`/libgit2-sys behind fewer dependents. Sell it as **enforced architecture + team scalability**, with modest build benefits as a bonus.
- **Cost:** ~8 manifests, one-time import churn (~25 test files), and the discipline to keep `[workspace.dependencies]` authoritative. For a codebase already exhibiting textbook layer discipline, that cost is low.

### Bottom line

This restructuring is unusually low-risk because the seams already exist and are respected in practice. The work is almost entirely *making the compiler enforce what the code review currently enforces*, plus three genuine untanglings: `AppEvent` out of the engine, UI data types out of `AppState`, and `granular` out of the domain-service layer.
