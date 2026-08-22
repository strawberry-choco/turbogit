# The GitExecutor interface is the engine seam; no VcsManager façade

TurboGit's engine layer exposes `Arc<dyn GitExecutor>` directly to core services, state, and UI dispatch. We deleted the `VcsManager` pass-through façade: its interface mirrored the executor's 53 methods with one-line bodies — it failed the deletion test, concentrated nothing, and made every engine addition ripple through four files. Root discovery and `Root` snapshots live in `core::multi_root` (the Root scanner), not on a manager object.

## Considered options

- **Keep `VcsManager` as a thin façade** — rejected: a shallow module invites regrowth of pass-throughs (that is how it reached 53 methods).
- **Lazy settings indirection inside the executor** — rejected: no behavior gain; services that need settings receive them explicitly (`sync_service::push` precedent).

## Consequences

- Engine additions touch the trait and adapters only — locality in one place.
- Two real adapters now exist at the seam: `CliExecutor` in production, `engine::fake::FakeExecutor` in tests; tests drive core logic without spawning git.
- Canonical `VcsSettings` lives on `AppState`; the CLI adapter keeps a snapshot for argv assembly and is rebuilt on save.
- Amends `execution-plan.md` §4.2, which previously prescribed `VcsManager` as a core service trait.
