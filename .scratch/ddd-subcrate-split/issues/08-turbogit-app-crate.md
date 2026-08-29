# 08 — Extract turbogit-app crate

**What to build:** The application layer becomes its own crate, egui-free. Into `turbogit-app` move: the app state container, the root caches, persistence, recents, and a new events module that receives `AppEvent`, `DecodedImage`, and `FetchedBlob` out of the engine module (the engine trait never references them — their producers are worker threads and the diff viewer). The granular stage/unstage service relocates here from the core service layer and is documented as an application service (orchestration over state), not a domain service. The root keeps shims for the state/caches/persistence/recents paths and repoints the granular path. The granular-ops and partial-dispatch suites move into the new crate's tests. One sanctioned edge is documented: the app crate may call the engine's backend-selection factory as the single composition touchpoint.

**Blocked by:** 03 — Decouple domain from RON; 04 — Invert the state→UI type leak; 07 — Extract turbogit-services crate.

**Status:** done

- [x] State, root caches, persistence, recents, and event types live in `turbogit-app`; the engine module no longer defines or re-exports them
- [x] Granular service lives in the app crate, documented as an application service; the core service layer is now entirely pure
- [x] App crate manifest contains no egui dependency
- [x] Granular-ops and partial-dispatch suites run from the new crate and pass
- [x] All four quality gates pass
