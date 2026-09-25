# The GitExecutor interface is the engine seam; no VcsManager façade

**Superseded by `ADR-0022` (2026-09-23) as to the shape of the seam.** The
decision here — one trait, no pass-through façade — stands and is still the
reason there is no `VcsManager`. Its *description* of the seam was wrong on two
counts, and this file's own face is where that gets recorded, because a future
reader trusts the ADR title more than the file that replaced it:

- "Two real adapters now exist at the seam" — there are **four** implementations
  (CLI, composed libgit2-over-CLI, the fake, and a hand-written forwarding
  adapter in `test-support`), and the composed one delegates to the CLI for 39 of
  its 68 methods.
- "Engine additions touch the trait and adapters only — locality in one place" —
  true of the number of files, false of the cost: an addition crossed four
  implementations.

What it got right, and what still binds: the interface is `GitExecutor`,
services receive settings explicitly, and canonical `VcsSettings` lives on
`AppState`.

TurboGit's engine layer exposes `Arc<dyn GitExecutor>` directly to core services, state, and UI dispatch. We deleted the `VcsManager` pass-through façade: its interface mirrored the executor's 53 methods with one-line bodies — it failed the deletion test, concentrated nothing, and made every engine addition ripple through four files. Root discovery and `Root` snapshots live in `core::multi_root` (the Root scanner), not on a manager object.

## Considered options

- **Keep `VcsManager` as a thin façade** — rejected: a shallow module invites regrowth of pass-throughs (that is how it reached 53 methods).
- **Lazy settings indirection inside the executor** — rejected: no behavior gain; services that need settings receive them explicitly (`sync_service::push` precedent).

## Consequences

- Engine additions touch the trait and adapters only — locality in one place. *(Corrected by `ADR-0022`: the file count held, the cost did not.)*
- Two real adapters now exist at the seam: `CliExecutor` in production, `engine::fake::FakeExecutor` in tests; tests drive core logic without spawning git. *(Corrected by `ADR-0022`: four implementations, and the fake could answer only 5 of the port's 22 typed reads, so most suites drove a real repository.)*
- Canonical `VcsSettings` lives on `AppState`; the CLI adapter keeps a snapshot for argv assembly and is rebuilt on save.
- Amends `execution-plan.md` §4.2, which previously prescribed `VcsManager` as a core service trait.
