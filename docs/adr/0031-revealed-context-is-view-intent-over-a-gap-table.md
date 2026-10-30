# Revealed context is view intent over a gap table, never patch material

A diff pane shows git's three lines of context and nothing else, so the unchanged runs between
hunks are invisible and cannot be read. We decided to let the user reveal them without asking git
anything new per click, and to place the revealed lines in `PaintPlan`'s row space as `Slot`s that
are **not** entries in `DiffModel::display`. The reason is that a reveal must never become patch
material: `ADR-0013` as restated by `ADR-0022` composes staged patches from the parsed `Patch`, and
`ADR-0014` measures its frame budget on a per-frame walk of `display`, so rows that materialize
grow the thing both of those rules are written about.

## Considered options

- **Re-diff with a wider context per click**, rejected twice over. Width is a whole-patch option,
  so it cannot name one hunk's edge, and widening **fuses** hunks whose contexts touch, which
  moves the hunk count that `HunkTarget::Whole` indexes positionally
  (`crates/turbogit-app/src/granular.rs`).
- **Materialized rows in the display model**, with reveal as a filter the way hunk collapse is,
  rejected: a materialized row space needs a ceiling, and a ceiling keeps a strip on screen over a
  gap that can reveal nothing more, which is the exact affordance the feature forbids. It also
  re-runs `build_model` inside the click's own frame, because the click is handled in the paint
  arm.
- **Eager file text carried on the diff value**, rejected on coverage:
  `crates/turbogit-services/src/diff_engine.rs` answers in-process only for a single-path,
  no-stat, no-commit comparison, so a whole-commit pane and every CLI-sourced diff would get
  nothing, and "eager where we happen to hold the text, lazy otherwise" is two mechanisms for one
  verb.
- **Reusing `Kind::Pane` to carry the text**, rejected: its key omits the `FileMeta`, its one
  in-flight slot is shared with image panes, its cache is FIFO with cap 4, and `PaneEntry` has no
  field for lines.
- **A windowed `GitExecutor` read**, rejected: it is a `git show` either way, so it buys no
  cheaper call, and it still cannot size a run that ends at the file's end.

## Consequences

- The unexpanded row space stays proportional to the patch, and the expanded one to what the user
  asked to see. `Slot` replaces `plan index == display index` as the row identity, so
  `total_rows`, `hunk_slots`, `staging`, `caret` and `file_rows` keep their contracts while the
  two `model.display[di]` dereferences learn the new variants.
- A revealed line is a `&str` borrowed from one file's text, resolved from `DiffTarget::pane_sides`
  on the **new** side when it exists. Reading one side answers both panes because a gap is by
  definition unchanged, and that holds only while nothing sets `DiffOpts.ignore_whitespace`.
- The **trailing** run is the one thing the patch cannot answer, because a file's length is not in
  it. Its strip may appear and then disappear after one tap proved the tail empty. Every other gap
  knows its length from the headers and hides its strip exactly when exhausted.
- Git spells a context-free insertion `@@ -5,0 +6 @@`, where the anchor line is itself unprinted, so
  the leading run is `start` lines, not `start - 1`. Header subtraction without that case is off by
  one, and deriving geometry from the printed row numbers is worse: `rows_from` resets its
  line-number counters per hunk but not per file, so on a multi-file patch header rows carry the
  previous file's numbers.
- Each expansion affordance is **one button whose kind names its direction** — expand next lines,
  expand previous lines, or, for a run within one reveal step of closing, one expand-in-between button
  that finishes it in that tap. A two-button strip over a single gap was built, reviewed and rejected:
  it puts opposite directions in one row and forces an anchor argument for neither. The collapse
  threshold is one step, so a collapsed gap is always a single click.
- Reveal counts are keyed by the diff read's own key and survive a refresh of the same comparison,
  which is a deliberate break from how `diff_collapsed` is wiped at the fetch site: staging a line
  refreshes, and a user who revealed context and then staged should not have every region closed
  under them.
