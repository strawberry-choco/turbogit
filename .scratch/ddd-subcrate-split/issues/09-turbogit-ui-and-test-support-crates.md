# 09 — Extract turbogit-ui and test-support crates

**What to build:** Presentation becomes its own crate and the headless test harness becomes a shared one. The theme module and every UI module move into `turbogit-ui`, depending on the app, services, port, and domain crates plus the egui stack. The kittest helpers and the recording executor currently duplicated out of the shared test module are extracted into a `test-support` crate. The redesign UI suites move into the UI crate's tests, and `egui-kittest` (with its wgpu/snapshot features) becomes a dependency of only `test-support` and the UI crate's dev-deps — no other crate pays its compile cost. The screenshot-acceptance suite writes PNGs via a manifest-relative path; that path constant must be updated for the new directory depth and the snapshots regenerated.

**Blocked by:** 08 — Extract turbogit-app crate.

**Status:** ready-for-agent

- [ ] Theme and all UI modules live in `turbogit-ui`; kittest helpers and recording executor live in `test-support`
- [ ] Redesign UI suites run from the UI crate and pass
- [ ] Screenshot-acceptance suite's output path is corrected for the new crate depth and snapshots regenerate cleanly
- [ ] `egui-kittest` appears only in `test-support` deps and UI dev-deps
- [ ] All four quality gates pass
