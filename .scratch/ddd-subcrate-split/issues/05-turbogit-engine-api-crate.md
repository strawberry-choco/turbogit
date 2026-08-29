# 05 — Extract turbogit-engine-api crate

**What to build:** The engine port becomes its own crate: the `GitExecutor` trait moves whole (not sliced into capability traits) together with `ApplyDirection`, depending only on `turbogit-domain`. The engine module in the root shrinks to a re-export of the port plus the items that stay behind. The odd re-export of the cache `Affected` type from the engine module is dropped, and its importers repoint to where `Affected` actually lives. No test file is edited.

**Blocked by:** 02 — Extract turbogit-domain crate.

**Status:** done

- [x] `turbogit-engine-api` crate contains the whole `GitExecutor` trait and `ApplyDirection`, depending only on the domain crate
- [x] Engine module re-exports the port; the `Affected` re-export is gone and all its importers are fixed
- [x] All four quality gates pass
