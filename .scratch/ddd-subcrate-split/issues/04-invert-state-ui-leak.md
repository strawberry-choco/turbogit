# 04 — Invert the state→UI type leak

**What to build:** The application state stops reaching up into the UI layer for its data types. The plain-data diff-pane types (`PaneCache`, `PaneEntry`, `PaneSide` and the blob constructor) and the hunk-navigation edge-nudge types (`Dir`, the edge-window constant) move down out of the UI modules into a plain-data module beside the app state. The UI modules import them back up; nothing else changes. This is behavior-neutral prep that makes an egui-free application crate possible later, and it is guarded by the existing diff redesign and parity suites.

**Blocked by:** 02 — Extract turbogit-domain crate.

**Status:** ready-for-agent

- [ ] Pane and hunk-nav plain-data types live beside the app state, not in UI modules
- [ ] UI modules import the types back up; public `ui::diff` / `ui::hunk_nav` paths used by state and test suites keep resolving
- [ ] Diff redesign and diff parity suites pass unchanged
- [ ] All four quality gates pass
