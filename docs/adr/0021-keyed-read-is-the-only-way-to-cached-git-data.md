# The keyed read is the diff surface's only way to reach cached git data

Reading a cached git value today means rebuilding the cache key at the call site and
comparing it by hand: the rule is written **eleven** times across the app and UI
crates, five of those copies in presentation code, and `ADR-0020` recorded it as
written three times. We decided to consolidate them into one **Keyed read** —
`read(target)` answers *and* admits, `peek(target)` answers only — placed in
`crates/turbogit-app/src/keyed_read.rs`, holding the rule and no storage: the values
stay in the `UiState` fields they live in now. Callers pass a *target*, the app
derives the key, and no caller ever formats or compares a key string again.

## Status

Accepted. Stage (i) — the module and the diff kind — landed as
`.scratch/keyed-read/issues/01-slot-and-diff-read.md`; blame and the pane bytes
follow in tickets 02 and 03, and the last field readers go in 04.
The glossary entry was in `CONTEXT.md` already, because the concept was settled
before the module was.

Two consequences the tickets wrote differently than the code needed:

- **A `Failed` verdict does not re-admit.** Today a failed diff is re-dispatched
  every frame the surface paints, because the admit test only asks whether the
  cached key is stale — so a permanently failing comparison hammered the engine
  once a frame, and `diff_error` was cleared by the very call that preceded
  reading it. A failure now belongs to its key until that key changes or a
  refresh retires it, which is what "an error belongs to the key that failed"
  has to mean to be worth anything.
- **The pane's key and its two side specs are derived from the diff target**,
  not handed to `ensure_pane_bytes` by the painter. Ticket 01 removes the
  caller's ability to name a diff key at all, which the `#bin`/`#img` spellings
  depended on, so ticket 03's two bullets about them landed here.

## Considered options

- **Own the storage as well as the rule** — rejected: it forces the
  owned-handle-per-visible-row question that ADR-0014's frame budget governs into a
  refactor that is otherwise pure subtraction. Values stay where they are.
- **A generality first: generation stamps, interned structural keys, an admission
  policy per kind, absorbing `RootCaches`' invalidation and `WorktreeLifecycle`'s
  mutation epoch** — rejected for this branch, not for ever. It would carry ~10 read
  kinds without the implementation changing, and it is a strictly better second
  branch once four real reads have crossed one interface. Folding ADR-0019's epoch
  into a framework also hides the policy that ADR-0019 exists to protect.
- **One `Target` enum for input rather than a sealed trait per read kind** —
  rejected: the verdict payload loses its per-kind type, so every painter matches a
  three-way handle and can take the pane entry where the patch text was asked.
- **`read` only, no `peek`** — rejected: the granular preview and the palette verbs
  are readers that must not admit, and a read that always fetches turns today's
  deliberate silent no-op into a spend of git work.
- **Collapse the three completion events into one `ReadReady`** — rejected:
  ADR-0020 requires the event stay cheap, copyable and free of a live closure. Only
  the settlement body collapses.

## Consequences

- **ADR-0020's deliberately-violated constraint is closed, not inherited.** The read
  hands back an owned `Rc`/`Arc` handle: one refcount bump per frame, none per row,
  so `&mut AppState` stays free while a painter writes the hunk cursor and collapse
  set. ADR-0020 deferred this on the belief that fixing it meant owning per visible
  row under ADR-0014's budget; the code already disproves that — `diff_model()`
  returns `Rc<DiffModel>` and its callers mutate state while holding rows.
- **The per-frame parse memo moves inside the read.** `read_diff` answers a value
  that already carries its display model, which deletes the thread-local memo and
  the full-text compare it paid every frame. The key derivation is one `format!`
  per ask instead of the two the caller used to build to answer the same
  question; removing the last one is what generation stamps or interned keys
  would buy, and that stays the rejected second branch. This makes the owned
  handle a gain rather than a wash, and it puts ADR-0014's display-row model
  behind the read's interface instead of beside it — which meant *moving* that
  model: it is `crates/turbogit-app/src/diff_model.rs` now, egui-free, imported
  back up by the painters (the same inversion `diff_data` already did for the
  pane types).
- **Five fields leave `UiState`:** `diff_loading`, `diff_error`, `blame_loading`,
  `blame_error`, `pane_bytes_loading`. The verdict is the whole paint contract, so
  a caller cannot forget to look at a flag — and `shell.rs`'s busy gate asks the
  read instead of one of the fields.
- **An error belongs to the key that failed.** Today `diff_error` is one field any
  in-flight failure can set, so a late failure can blank a current value; the read
  attributes `Failed` to the target it came from.
- **Two store policies survive in one interface, and that is deliberate.** A
  single-entry read drops a value whose key mismatches; the keyed pane-bytes map
  keeps it, because the entry is keyed and will be right when that pane comes back.
  Do not unify them further.
- **A scripted read seam is internal, and it is a hold rather than an adapter.**
  `pub(crate)` and `cfg(test)` — `pump_keyed_read`, `hold_reads`,
  `release_held_read` — never named by a caller, and no constructor signature
  moves (the reason ADR-0020 declined executor injection). It exists because
  lateness is the only window in which staleness lives, and no adapter reachable
  today can arrive late: without it, the drop-on-stale and zombie-settlement
  rules cannot be tested at all without a git binary. A held dispatch is still
  admitted — its slot is taken and the frame is told to wait — and releasing it
  posts into the same channel a worker posts to and settles nothing itself, so
  scripted and real answers drain identically. Do not promote it to the read's
  interface.
- **The thread-local texture cache at `ui/diff/panes.rs` stays in the UI
  crate on purpose.** egui textures cannot live in `AppState`. It is a keyed cache
  with a per-frame ordering rule, so it will look like a copy that was missed: it
  was not. It keys by the tag `PaneTarget::texture_tag` derives and prunes by
  asking `AppState::pane_is_cached`, which is the last thing presentation code
  knows about the pane store.
- **`RootCaches`' invalidation interface and the read are two halves of one promise
  in the glossary.** Root caches already say cached values are invalidated through
  one interface and "never poked field-by-field by callers"; the read is the
  matching half for fetching them.
