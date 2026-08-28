# 11 — Documentation cutover

**What to build:** The project documentation matches the new workspace reality. The repository guidelines' project-structure and test-layout descriptions (already stale) are rewritten for the crate-per-layer layout, the progress tracker reflects the completed split, and any spec or architecture references to old module paths are updated to the new crate paths. A short note records the single sanctioned impurity (app crate calling the engine factory) and the deferred options (executor injection, capability-trait split, UI-state decomposition) so future work knows they are choices, not oversights.

**Blocked by:** 10 — Composition root cutover and shim removal.

**Status:** ready-for-agent

- [ ] Repository guidelines describe the workspace layout, crate responsibilities, and where tests live
- [ ] Progress tracker and spec/architecture docs reference current crate paths, not old module paths
- [ ] The sanctioned app→engine edge and the deferred options are documented
