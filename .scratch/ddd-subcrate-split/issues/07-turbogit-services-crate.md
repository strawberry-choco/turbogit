# 07 — Extract turbogit-services crate

**What to build:** The pure domain/application services become their own crate: the ten core service modules (branch, changes, conflict, diff engine, history editor, history service, integrate, multi-root, partial, shelve/stash, sync — everything except the stateful granular service, which stays behind for a later ticket) move into `turbogit-services`, depending only on the engine port and the domain. Their unit tests come along; the fake executor is consumed through a dev-dependency on the engine crate's `test-util` feature. The outgoing-commits sync suite moves into the new crate's tests. The root keeps a shim so `turbogit::core` paths still resolve.

**Blocked by:** 05 — Extract turbogit-engine-api crate; 06 — Extract turbogit-engine crate.

**Status:** ready-for-agent

- [ ] All pure core services and their unit tests live in `turbogit-services`; the granular service does not move
- [ ] Services crate depends on the port, never on adapters, egui, or app state (fake executor only as dev-dependency)
- [ ] Sync outgoing suite runs from the new crate and passes
- [ ] All four quality gates pass
