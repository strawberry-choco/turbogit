# The recents store keeps every project, and Welcome windows its card to three

The store truncated to ten (`MAX_RECENTS`) and the Welcome card painted every row it was handed, so
one constant in `turbogit-app` decided both what a user kept and what a user could see. We decided
that **the store records every project and truncates nothing, and the Welcome card paints three of
them**: the window is a display constant (`RECENTS_WINDOW`, `crates/turbogit-ui/src/ui/welcome.rs`)
the store never learns about, whether it is open is one non-persisted bit
(`UiState::welcome_recents_expanded`), and the card's inert `Show all projects` label is now a real
disclosure button that appears only while records remain hidden.

A window carries one obligation: a stored project must still be reachable from the screen that shows
the store. It cannot route to the workspace switcher, because `workspace_picker` closes itself
whenever Welcome is showing and `render` paints Welcome before it runs the popups, so a Welcome click
cannot open that picker in the same frame. The card reveals its own records in place. The badge keeps
the store length rather than the row count: `12` above three rows plus a button that resolves the
difference is honest, and `3` would hide nine projects.

Two costs the cap was silently paying for, both now owned by the surfaces that incur them.

- **Per-frame branch reads.** Every painted row reads its current branch through
  `AppState::current_branch_of`, one uncached `git2::Repository::open` per row, memoized five seconds
  per path. The composed executor answers from libgit2 (`crates/turbogit-engine/src/git2_exec.rs`),
  so no git process spawns. Measured on one machine, debug profile, 40 freshly seeded single-commit
  repositories: 175-270 µs per read, 7-11 ms for all 40. Collapsed, the card now reads three rows
  where the cap let it read ten; expanded, it reads every record, and only on a frame whose memo has
  gone stale.
- **The switcher's unbounded menu.** `workspace_menu` laid out `1 + N` rows in a bare `egui::Area`
  with no height bound, so an uncapped store pushed its rows and its own footer items past the
  viewport. Its recent half now scrolls under a fixed height, with the `Current` and `Recent`
  captions outside the scroller so a scrolled list never shows rows under no name.
- **The card's per-frame copy.** `recents_column` cloned the whole store each frame to release its
  borrow before the row body mutates `AppState`, so painting three rows copied every record. It now
  copies the window it paints.

## Considered options

- **A display cap in the store**, rejected on layering. `recents.rs` would then own a fact about one
  screen, and the switcher, which needs the whole list, would have to work around it.
- **A centered `All projects` overlay with a filter field**, rejected on cost. A long flat list is
  exactly what a search box is for, and the overlay is the cheapest place to hang one, but it costs a
  second row renderer to keep in sync with the first, Escape and focus machinery over the screen the
  user is reading, and rows that show less than the top three. The measured read cost is what
  removed the only argument that genuinely needed a virtualized surface.
- **Virtualizing the expanded card over `ScrollArea::show_rows`** (`ADR-0014`, `ADR-0026`), rejected
  on the same measurement: tens of milliseconds per stale frame is not a cliff, and virtualizing
  would make which rows exist a function of the viewport, which the painted-text assertions this
  screen is tested with cannot express.

## Consequences

- Rows for deleted directories never leave. The cap used to evict them eventually, and `open_recent`
  only toasts. An uncapped store makes a moved folder a permanent row.
- `load` maps any parse failure to an empty list and every write is a whole-file overwrite with the
  error discarded. Uncapped, a corrupt `recents.ron` now loses the entire history instead of the last
  ten. `ADR-0005` chose the forgiving read deliberately, so the blast radius is a decision to revisit
  rather than a defect this change fixed.
- `welcome_branch_cache` grows by one entry per path ever painted. It is in-memory, disposable by
  `ADR-0005`, and bounded by the store.
