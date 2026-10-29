# Paste-ready prompt. Rebased parity research agent

Everything below the divider is the prompt. It assumes the agent has this
repository checked out and can run shell commands. Nothing else is needed.

---

You are doing UI/UX research, not implementation. Do not modify any code.

## Goal

Produce a complete, evidence-backed parity specification so that TurboGit, a
Rust/egui Git client living in this repository, can be brought in line with
**Rebased**, a stripped-down IntelliJ IDEA Git client installed at
`/Applications/Rebased.app`. Parity spans three dimensions, and all three are
in scope: **UI** (what exists, what it is called, where it sits, its interaction
states), **UX** (how it is discovered, what it confirms, what it forgives), and
**user flow** (how surfaces chain into journeys).

First, read `research/evidence/rebased/DISTILL.md`. It is the full brief,
goal, inputs, rules, deliverables, definition of done. Everything below is the
short version so nothing depends on that file being read successfully.

## Step 0, refresh the evidence

```
research/evidence/rebased/harvest.sh
```

This re-harvests the installed Rebased bundle (action ids, labels, every default
keystroke, the settings tree) and fetches the verified JetBrains documentation
pages into plain text. It takes about 30 seconds and needs network only for the
docs. Run it even if the files already exist, it regenerates the surface
skeletons from the manifest `surfaces.tsv`.

If it fails, stop and report the error rather than proceeding on stale evidence.

## What you are reading

- `research/evidence/rebased/HARVEST-REPORT.md`, every surface, how many
  actions it has, and which documentation pages actually resolved. A surface
  showing `0 / N` docs has no documentation covering it.
- `research/evidence/rebased/parity/NN-<surface>.md`, one skeleton per surface,
  already preloaded with the real action inventory (id, label, keystroke) and the
  sources that resolved. **These are your working files: fill them in place.**
- `research/evidence/rebased/docs-text/*.md`, the documentation, converted to
  plain text. This is your evidence base. Read the relevant ones in full.
- `research/evidence/rebased/actions-labeled.tsv`, the bundle's action
  inventory; cross-check anything the per-surface table missed.
- `research/evidence/rebased/SOURCES.md`, which online sources work, and which
  are dead ends (do not try to read git4idea source; it is no longer public).

## Rules

1. **Cite every non-obvious claim.** Quote the sentence and point at the file
   (`docs-text/resolve-conflicts.md`, or `actions-labeled.tsv` for ids).
2. **`UNVERIFIED` is a valid answer.** If neither the bundle nor the docs
   establish a behaviour, write `UNVERIFIED` and add it to that surface's "needs
   runtime capture" list. Never fill a gap with plausible-sounding IntelliJ
   behaviour, a confidently wrong line costs far more than a flagged gap.
3. **Bundle wins on names, ids and keystrokes**, it comes from the exact
   installed build. **Documentation wins on UX and flow.**
4. **Docs mangle Mac shortcuts**: `Ctrl0K` means `⌘K`, `Alt09` means `⌥9`.
5. **Parity of structure and flow, not of finish.** Copy topology, item
   inventory, menu order, default keystrokes, dialog field order and defaults,
   terminology (Changelist, Shelve, Amend, Smart Checkout), and empty/error/
   progress states. Do NOT copy Darcula colours, insets, their icons, or their
   accessibility gaps. Ignore everything the docs say about the editor,
   terminal and debugger, those are IDEA inheritance, not a Git client.
6. **Never copy sentences from the harvested string bundles.** Terminology may
   match; wording must be ours.
7. Where egui cannot match mechanism, no real `NSMenu`, no drag-reorderable
   changelists, **match intent**: same items, same order, same accelerators,
   our own chrome. Record it in the divergence table, do not silently drop it.

## Per surface, fill in place

Work in this order, richest evidence first:
`05-conflicts`, `02-branches-popup`, `01-commit-tool-window`,
`06-history-rewrite`, `07-shelve-stash`, `08-remotes-sync`,
`03-log-tool-window`, `04-integrate`, `10-patches`, `09-onboarding`,
`11-diff-viewer`.

For each, complete four sections:

- **UI**, topology (regions, panels), per-region contents, and the state matrix:
  default / hover / pressed / disabled / focus / selected / progress. Note the
  source of each fact.
- **UX**, answer the five questions already stubbed in the skeleton: how the
  surface is reached and what hides behind a secondary affordance; which verbs
  are destructive and what they confirm; what can be undone, from where, and for
  how long; which options are on by default and what is remembered between uses;
  what feedback appears (progress, optimistic paint, notification wording) and
  where focus lands afterwards.
  Standard to hit, a dirty checkout offering **Force Checkout vs Smart
  Checkout**, where Smart Checkout shelves, switches, then unshelves and can
  itself conflict; **Delete** running immediately even with unmerged commits but
  offering restoration plus a link to the lost commits; the **Recent** node
  capped at five branches; the multi-repo **Push** dialog defaulting to the
  current repository only and surfacing `Define remote` inline.
- **Flow**, entry points, internal sequence *including dialog chaining* drawn as
  `A → B (dialog) → C`, next step on success, next step on failure and how the
  user recovers, escape/dismiss semantics.
- **Divergences**, one justified row per intentional departure, tagged with its
  dimension. An empty table means nobody considered it, not that they agree.

## Deliverables

1. `research/evidence/rebased/parity/*.md`, every surface filled, zero bare
   TODOs left (an `UNVERIFIED` plus a capture note is a legitimate finish).
2. `docs/spec/rebased-parity.md`, the assembled document: background, method,
   the copy/don't-copy table, one section per surface summarising UI/UX/Flow and
   linking its evidence file, then four closing tables: **gap list** (Rebased
   surface vs TurboGit equivalent vs verdict, has / partial / missing /
   intentionally absent), **reverse gap list** (what TurboGit has that Rebased
   does not: multi-root bulk operations, smart grouping, worktrees, submodules,
   parity must not delete these), **unresolved** (every `UNVERIFIED` and every
   404, with what would close each), and **divergences**.
   TurboGit baseline for the comparison: 14 `Operation::*` variants in
   `crates/turbogit-app/src/operation.rs` and 20 `Dialog::*` variants in
   `crates/turbogit-app/src/state.rs`.
3. `.scratch/rebased-parity/issues/NN-<slug>.md`, one ticket per implementable
   unit, each naming the surface, the spec section it implements, three
   verification assertions in the existing structural kittest style
   (`painted_text`, `galley_origin`, `filled_rects`), and anything unresolved.

## Definition of done, check before you stop

- All four sections filled per surface, or explicitly `UNVERIFIED` with a note.
- Every paraphrased behaviour traceable to a file under `research/`.
- `docs/spec/rebased-parity.md` exists with all four closing tables.
- Tickets exist for the implementable units.
- **`git status --short` shows changes ONLY under `research/`, `docs/spec/`, and
  `.scratch/`.** Nothing under `crates/` or `src/` changed. Confirm this and
  report it, if any code file changed, say so explicitly.

## Report back

Finish with: how many surfaces were fully specified vs partially, no more than
five examples of the most valuable behaviours discovered, and a list of what
still needs runtime capture (launching Rebased) because neither source covered
it. Be concrete and cite files.

If the evidence volume proves too large for one pass, do surfaces one at a time
in the order above and report after each, partial delivered work with clean
provenance beats a speculative whole.
