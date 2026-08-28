# 02 — Extract turbogit-domain crate

**What to build:** The domain model and error types become the leaf crate `turbogit-domain`, depending on nothing but `serde`, `chrono`, and `thiserror`. The `model` module (git entities, op vocabulary, settings types) and the `error` module move into it verbatim, keeping their internal module names. The root library re-exports them so every existing `turbogit::model::X` and `turbogit::error::X` path keeps resolving — no downstream file is edited in this ticket.

**Blocked by:** 01 — Workspace scaffolding.

**Status:** ready-for-agent

- [ ] `turbogit-domain` crate exists with `model` and `error` modules moved verbatim
- [ ] Domain crate manifest declares only `serde`, `chrono`, `thiserror`
- [ ] Root library shims preserve all existing import paths; zero edits outside the new crate and root manifests
- [ ] All four quality gates pass
