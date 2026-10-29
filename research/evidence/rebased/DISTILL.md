# DISTILL, the agent pass: turn harvested evidence into the parity document

> This file is the hand-off. It assumes nothing about prior conversation.
> Read it, then execute it against this repository.

## Goal

Produce `docs/spec/rebased-parity.md`: a complete, evidence-backed specification
of how **Rebased** (a stripped-down IntelliJ IDEA Git client, installed at
`/Applications/Rebased.app`) behaves, so TurboGit can be brought to parity across
**UI**, **UX** and **user flow**. TurboGit is a Rust/egui git client in the same
repository. The point of this pass is to *specify*, not to implement, **do not
touch anything under `crates/` or `src/`.**

## What has already been harvested (read, do not redo)

Run to refresh if anything below is missing. Nothing requires launching Rebased.

```
research/evidence/rebased/
  harvest.sh              one command: bundle extraction + docs + skeletons
  harvest.py              verifier/fetcher/skeleton generator
  surfaces.tsv            ONE ROW PER SURFACE — the manifest, edit to add surfaces
  extract.sh              bundle harvester (actions, strings, keymap, settings labels)
  MANIFEST.txt            which Rebased build everything below came from
  actions.txt             1821 action ids declared by the bundled plugins
  actions-labeled.tsv     id | label | description | key  (Git/Vcs/Diff subset)
  keymap-bindings.txt     every default keystroke
  strings/                GitBundle (1772), VcsBundle (1259), VcsLogBundle (309)
  settings-labels/        the Settings tree, verbatim
  SOURCES.md              which online sources work and which are dead ends
  docs-text/              each verified doc page, converted to plain text
  parity/NN-<surface>.md  ONE SKELETON PER SURFACE — you fill these in
  HARVEST-REPORT.md       what resolved, what did not
```

## Refresh command

```
research/evidence/rebased/harvest.sh        # ~30 s, needs network for docs only
```

It reports how many pages resolved. A row reading `0 / N` docs means that surface
has **no documentation**, say so explicitly in its spec and list what runtime
capture would be needed, rather than inventing behaviour from general knowledge.

## The governing rule, parity of structure and flow, not of finish

| Copy from Rebased | Do NOT copy |
| --- | --- |
| Panel topology; every item a surface offers | Darcula colours, insets, radii, row heights |
| Menu and context-menu ordering and grouping | Their icon set |
| Default keystrokes (`⌘⌥N`, `⌥M`, `⌃⌥Z`, `⌥0`…) | macOS-native chrome egui does not own |
| Dialog field order, defaults, validation timing | IDEA inheritance: editor, terminal, debugger |
| Terminology: Changelist, Shelve, Amend, Smart Checkout | Their literal string files and images |
| Empty, progress and error states | Their accessibility gaps (TurboGit's own R4 exists because a11y is weak everywhere) |

Where egui cannot match mechanism (no real `NSMenu`, no drag-reorderable
changelists), **match intent**: same items, same order, same accelerators, our
chrome. Write that in the divergence table rather than silently dropping it.

## Source precedence

1. **Bundle artifacts win on names, ids and keystrokes.** They come from the
   exact build installed; the docs are for a moving version.
2. **Documentation wins on UX and flow.** Those exist nowhere else.
3. **Docs do not render Mac modifiers correctly** in raw HTML, `Ctrl0K` means
   `⌘K`, `Alt09` means `⌥9`. Always convert.
4. **If neither source says it, write `UNVERIFIED`** and add it to that surface's
   "needs runtime capture" list. Never infer behaviour plausibly and present it
   as fact, a confident wrong line costs more than a flagged gap.

## Procedure, per surface

For each `parity/NN-<surface>.md` in numeric order:

1. Read the skeleton and its `Sources` block.
2. Read every `docs-text/<slug>.md` listed there in full. These are the whole
   evidence base for that surface.
3. Read the shipped action table; cross-check it against `actions-labeled.tsv`
   for anything the prefixes missed.
4. Fill in, in place:

   - **UI**, topology, per-region contents, and the state matrix
     (default / hover / pressed / disabled / focus / selected / progress).
     Note which facts came from docs vs bundle vs runtime.
   - **UX**, the five questions already stubbed in the skeleton
     (reachability and hidden affordances; destructive-verb confirmation and
     preflight; undo reach and lifetime; defaults and memory between uses;
     feedback, progress, notifications, focus after action).
     Concrete examples of the standard to hit: a dirty checkout offering
     **Force Checkout vs Smart Checkout**; **Smart Checkout** shelving →
     switching → unshelving and being able to conflict itself; **Delete** being
     immediate even when unmerged but offering restoration plus a link to the
     lost commits; the **Recent** node capped at five; branch grouping off `/`
     prefixes; the **Push** dialog defaulting to only the current repository in
     multi-repo setups and surfacing `Define remote` inline.
   - **Flow**, entry points, internal sequence including dialog chaining,
     the next step on success, the next step on failure and how the user
     recovers, escape/dismiss semantics. Draw the sequence: `A → B (dialog) → C`.
   - **Divergences**, one justified row per intentional departure. An empty
     table means nobody considered the question, not that answers agree.

5. Cite every non-obvious claim: quote the sentence and point at the file
   (`docs-text/resolve-conflicts.md`, or `actions-labeled.tsv` for ids).

## Deliverables

1. `research/evidence/rebased/parity/*.md`, every surface filled, no TODO left.
2. `docs/spec/rebased-parity.md`, the assembled document: background, method,
   the copy/don't-copy table, one section per surface summarising UI/UX/Flow and
   linking its evidence file, then four closing tables:
   - **Gap list**. Rebased surface vs TurboGit equivalent vs verdict
     (has / partial / missing / intentionally absent). TurboGit baseline:
     14 `Operation::*` variants and 20 `Dialog::*` variants in
     `crates/turbogit-app/src/operation.rs` and `state.rs`.
   - **Reverse gap list**, what TurboGit has that Rebased does not; those are
     the product's differentiators and parity must not delete them
     (multi-root bulk operations, smart grouping, worktrees, submodules).
   - **Unresolved**, everything marked `UNVERIFIED` plus every page that 404'd,
     each with what would close it.
   - **Divergences**, every intentional departure with its reason and its
     dimension (UI / UX / Flow).
3. Tickets at `.scratch/rebased-parity/issues/NN-<slug>.md`, one per implementable
   unit, each naming: the surface, the spec section, the three verification
   assertions (structural kittest style`painted_text`, `galley_origin`,
   `filled_rects`), and anything still unresolved.

## Definition of done

- Every surface file has all four sections filled, or an explicit
  `UNVERIFIED` plus a capture note. No bare TODOs.
- Every paraphrased behaviour is traceable to a file in `research/`.
- `docs/spec/rebased-parity.md` exists, links each surface, and carries all four
  closing tables.
- No file under `crates/` or `src/` changed. Verify with `git status --short`.
- Nothing was copied verbatim from the harvested string bundles; terminology
  matches, wording is ours.

## Suggested order

Start with `05-conflicts` and `02-branches-popup`, they have the richest
documentation and the sharpest decision points. Let the shape you settle on
there set the template for the rest.
