# 10 — Composition root cutover and shim removal

**What to build:** The migration completes: the root crate becomes a thin composition root (binary wiring plus the one true cross-layer test suite that needs every crate), and every compatibility shim is deleted. All remaining imports are mechanically repointed from the old root paths to their real crate paths across source and test files, then the re-export shims in the root library are removed entirely — no deprecated paths survive. Feature unification is verified: the engine's `test-util` feature, activated only via dev-dependencies, must not leak into release builds.

**Blocked by:** 02 — Extract turbogit-domain crate; 03 — Decouple domain from RON; 04 — Invert the state→UI type leak; 05 — Extract turbogit-engine-api crate; 06 — Extract turbogit-engine crate; 07 — Extract turbogit-services crate; 08 — Extract turbogit-app crate; 09 — Extract turbogit-ui and test-support crates.

**Status:** ready-for-agent

- [ ] No `pub use` shim remains in the root library; every import names its real crate
- [ ] Root crate contains only the binary wiring and the cross-layer parity suite
- [ ] Plain `cargo check` (release profile) does not build the `test-util` feature; `cargo check --all-targets` does
- [ ] All four quality gates pass
