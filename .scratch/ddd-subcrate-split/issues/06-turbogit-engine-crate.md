# 06 — Extract turbogit-engine crate

**What to build:** The engine adapters become their own crate: the CLI executor, the libgit2 executor, and the backend-selection factory move into `turbogit-engine`, alongside the port and domain crates. The fake executor used by tests ships behind a `test-util` cargo feature so consumers opt in via dev-dependencies. The root engine module keeps re-export shims so existing imports resolve untouched. The engine-level test suites (golden output, push dry-run, partial-stage CLI) move into the new crate's tests.

**Blocked by:** 05 — Extract turbogit-engine-api crate.

**Status:** ready-for-agent

- [ ] CLI executor, git2 executor, and `build_executor` live in `turbogit-engine`; fake executor is gated behind the `test-util` feature
- [ ] Root engine module shims keep existing import paths resolving
- [ ] Engine test suites run from the new crate and pass
- [ ] All four quality gates pass
