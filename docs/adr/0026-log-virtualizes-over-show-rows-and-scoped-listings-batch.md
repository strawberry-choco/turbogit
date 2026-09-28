# The commit list virtualizes over `ScrollArea::show_rows` and its listings grow by batches

ADR-0014 settled how a long list stays fast in this codebase: not
`egui_extras::Table`, but `ScrollArea::show_rows` given the total row count and a
uniform row pitch, so a frame builds only the rows in its viewport. That ADR is
about the diff viewer and its display-row model, and **this one extends it
rather than replacing or contradicting it** — the same mechanism, the same
rejection of `Table`, the same uniform-height precondition, now applied to a
second surface. The diff viewer's display rows, its hunk navigation and its
cached model are untouched by this decision.

The commit list needed it for a second reason the diff viewer did not have. Its
rows come from git, and a repository's history does not stop at whatever fits on
screen: before this, the log asked for a whole listing in one request and a
scope — one file's history, one ref's history, a pickaxe search — was asked for
all of it. So the listing itself is now **batched**. A **loaded window** is the
prefix of a listing the app currently holds; a **batch** is one fetched slice of
it, and the unit every log read asks for. A cold read asks for one batch; a
later one asks for the batch after it, one row longer, so the row the window
already holds comes back as the boundary checksum. A batch that comes back short
is the end of the listing, and its length against what was asked for is what
sets the window's has-more flag. Path, ref and search scopes page on those rules,
keep three caches that are separate from the unscoped window's and from each
other, and are offered the same Load-more affordance wherever their history
continues.

One scope cannot be given a position by that rule, and how its batches are
positioned is the one thing the three scopes do not share. git's `--skip` counts
commits in the *unfiltered* traversal and applies `-S` after it, so a position
sized from the window counts traversal entries rather than matches: a search
window holding `c1, c2, c3` asks for `c4` next and is answered with a row from
the middle of the history. A search batch is therefore asked for from the front
of the filtered stream, sized to what the window already holds plus one batch,
and taken from the tail of the answer, while a path or ref batch keeps the
overlapping position. Everything around the batch — the anchor checksum, the
short batch that ends a listing, the has-more flag — is the same rule for all
three. Batching also narrowed a search for the first time: `git log -S` used to
be asked for uncapped and answered with every match, so leaving a broad search
one batch deep would have been a silent loss of results rather than a
deferral.

The batch is not called a page because the glossary reserves that word: **Tab**
rejects `page` and `view` as its synonyms, and a tab is not a batch. The process
still reads as paging — a list pages over its rows, a window pages in more
history — while the unit is a batch.

Two things about the painted list follow from `show_rows` rather than from
choice. The **lane** — the graph column a commit's node is drawn in — is assigned
by a walk over the whole loaded window, before the live filter and before search
hits are unioned in, and is held with the window rather than rebuilt per frame:
a walk over the drawn rows would change a commit's colour as it crossed the
viewport, which is not a colour anyone can learn. And scroll-to-selected became
index-based, because a row outside the viewport was never built and so has no
response to ask: the selection is found in the loaded window, its position is
computed from the index and the row pitch, and the scroll area is aimed there.
The commit details pane's parent link, the blame view's row click and the commit
context menu are the three callers, and all three go through that one path.

## Considered options

- **`egui_extras::Table` for the commit list**, as the product spec's
  list-of-tables entry originally named — rejected: ADR-0014 already rejected it
  for the diff viewer, for the same reasons that hold here. The row paints its
  cells directly, in a graph gutter and a details column that `Table` would
  impose a cell and widget model on. The spec entry was corrected rather than
  the code changed to match it.
- **Virtualize the painted rows but keep asking git for whole listings** —
  rejected: it fixes the frame cost and leaves the request cost, and a
  repository with a long history for one file would still hand the render thread
  a listing sized by the repository rather than by the screen. Batching is what
  makes the first paint independent of history depth.
- **Keep the Load-more rule that suppressed the affordance inside a scope** —
  rejected: its premise was that a scope was fetched whole. That premise is gone,
  so the rule went with it rather than being reimplemented per scope.
- **Leave the search scope out of the pager, as a scope whose batch cannot be
  positioned** — rejected: this was what the first landing of batching did, and
  it stopped being acceptable as soon as batching capped a listing that used to
  be uncapped. A search shown one batch deep with no way past it is a worse
  answer than a batch that costs a re-read, and the position rule is per-scope
  data in the plan rather than a property of the engine, so the exception is one
  branch and the pager keeps one rule for everything else.
- **A position in the engine, or a filtered `skip`** — rejected: `LogOpts::skip`
  is an offset into the traversal and the pickaxe filter is applied after it, so
  no value of it addresses a search window's match stream. A new request option
  would buy the same answer the app gets by asking for a longer prefix and
  taking its tail, while putting a capability in the engine that one scope of one
  surface needs. Path and ref scopes still take the engine's own offset, so no
  executor, adapter or request option changes.
- **A per-scope in-flight guard keyed by root** — rejected: it would let a slow
  scope block a fast one on the same root. The key is the pair, so two scopes of
  one root page at once.

## Consequences

- **The loaded window is never evicted.** It only grows, so scrolling back to
  already-loaded history reads from cache instead of re-fetching. Growth stops
  being reachable in one step, which is what this change needed; a bounded
  eviction policy is a separate decision with its own trade-off against
  scroll-back, and is deliberately not taken here.
- **A scoped batch is read off the render thread and admitted through the same
  event path the unscoped listing uses**, with one read in flight per (root,
  scope) and the guard released on success and on failure alike. The *cold* read
  of a scope is still synchronous, because the log pane paints what that call
  returns in the same frame; a cold read that became asynchronous would show an
  empty scope for a frame and break that contract at every call site.
- **A scope's window is dropped rather than emptied when its batch comes back
  torn** — the listing moved under the request, so the rows measured against it
  no longer describe anything, and a window that claimed "no more history" over
  an emptied list would be believed by the next read. The restart re-reads the
  same listing from the front.
- **An append batch's anchor row is load-bearing.** A batch that does not lead
  with the row the window was measured against means the listing moved, and the
  window restarts instead of growing a torn list.
- **A search scope positions its batch in the filtered match stream, and pays for
  it in re-reads.** `--skip` counts commits in the *unfiltered* traversal, so a
  search window's next batch is asked for from the front, sized to what the
  window already holds plus one batch, and taken from the tail of the answer.
  The anchor checksum and the has-more flag are unchanged, so a search window
  restarts on a torn batch and ends on a short one exactly as any other window
  does; only its position differs, not the rules that judge the answer. The cost
  is that the already-loaded prefix is re-read on every batch, which makes
  paging a search O(n) in what is held where a path or ref batch stays O(1) in
  the batch. That is accepted because a search scope is narrow and because the
  request stays bounded to what is already held plus one batch instead of
  growing with the history. It is also the cost of not having done nothing: while
  a search batch was mispositioned, the app read every one as torn and re-read
  the listing from the front, which under the settled-bottom trigger is a
  `git log` per frame.
- **Row count is a rendering input, not a data one.** A frame's cost is bounded
  by the viewport, so a commit scrolled well outside it paints nothing; a test
  that needs a row's state must first bring it into view, and no feature may
  depend on interacting with a row that was not built.
- **The log and the diff viewer now share one answer to "how does a long list
  stay fast".** A future long list is expected to take the same route rather than
  re-deriving one, and `egui_extras::Table` stays out of this codebase.
