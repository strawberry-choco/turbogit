# 03 — Decouple domain from RON

**What to build:** The domain error type stops dragging the RON serializer into the domain layer. The serialization-failure variant of `TgError` carries a plain `String` instead of a `ron::Error` (verified: the `#[from]` conversion has no constructor sites today — all RON failures are already converted manually by the persistence layer). After this, `ron` is a dependency only of the app layer that actually reads and writes `.turbogit/` state.

**Blocked by:** 02 — Extract turbogit-domain crate.

**Status:** ready-for-agent

- [ ] The serde error variant holds a `String`; `ron` is absent from the domain crate's manifest
- [ ] Every place that previously relied on the implicit conversion now maps the RON error explicitly
- [ ] Error display output for serialization failures is unchanged for users
- [ ] All four quality gates pass
