# Worktree lifecycle owns worktree-list freshness policy

The app crate's `WorktreeLifecycle` owns worktree-list admission, worker dispatch, mutation epochs, and settlement (worktree-lifecycle ticket 02). It admits one fetch per root, stamps that admission with the epoch current at dispatch, and dispatches with the current executor and event-sender clones supplied by `AppState`. Settlement then answers two independent questions: it releases the admission slot *only* when the settling request still owns that slot, and separately accepts the result *only* when its stamp is still current. Its mutation outcome requires targeted list invalidation alongside the epoch bump. `RootCaches` retains cached list storage and the invalidation interface; `AppState` routes accepted lists and errors to the existing cache and last-error surfaces. Dirty probes remain in `AppState` until ticket 03.

## Considered options

- **Keep freshness policy in `AppState`** — rejected: admission, mutation invalidation, and settlement ordering would remain scattered among dispatch and event-routing code.
- **Extract the event pump** — rejected: routing existing events is not the policy boundary and moving the pump would broaden this change without centralizing worktree freshness.
- **Unify cache writes in a generalized cache-request framework** — rejected: storage already belongs to `RootCaches`; a generalized framework would obscure the worktree-specific mutation epoch and its sanctioned root-refresh exception.
- **Store an executor in the lifecycle or inject executors into app construction** — rejected: passing current clones at dispatch follows the existing settings-driven executor rebuild lifecycle without constructor or adapter changes.

## Consequences

- `AppState::fetch_worktrees` remains the shell's unchanged fetch-on-miss entry point; dispatch and freshness decisions live together in the lifecycle module.
- An admission is stamped with the epoch current at dispatch, so "one fetch per root in flight" holds through resets as well as mutations: a zombie settlement from before a reset is rejected *and* cannot free the slot its replacement holds, which would otherwise let the per-frame fetch-on-miss dispatch a duplicate worker. Rejection and slot release are deliberately independent — a stale settlement still frees its own slot, or the refetch its staleness made necessary could never be admitted.
- Mutation completion bumps only the affected root's epoch and requires its cached list to be dropped. Stale successes and errors are silently discarded; current fetch errors retain normal last-error feedback.
- Ordinary `Affected::Root` refresh retains the worktree list through the existing `RootCaches` exception. Sibling roots stay untouched by worktree mutations.
- Project switches, workspace attachment, close-all, all-root refresh, and rescan clear lifecycle state alongside cached lists. Rescan uses a worktree-only all-roots invalidation interface, including roots no longer discovered, without clearing unrelated caches.
- Existing event vocabulary, engine seams, shell visibility gating, submodules, and dirty-probe dispatch and settlement remain unchanged. No executor or cache is retained in the lifecycle module.
