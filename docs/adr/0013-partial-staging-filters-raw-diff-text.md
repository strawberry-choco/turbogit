# Partial staging composes patches by filtering raw diff text

**Superseded by `ADR-0022`, as executed by deepen-git-engine ticket 11
(2026-09-24).** **Partial staging** selects, composes and applies from the
**Git engine**'s patch value, and the text the index takes is rendered from the
composed value at the moment of applying. What this record was protecting is not
withdrawn; it is restated in terms of the value:

- **A selection is appliable because it was composed from the same value the user
  selected from.** The hunks and line ordinals the viewer offers *are* the
  positions `partial::compose` filters, so a hunk that can be seen is a hunk
  that can be staged — there is no second reading of a diff that could drift
  from the first.
- **A `@@` header's counts are computed from the kept lines on both sides**, so
  header and body agree before anything is applied. `git apply --recount` was
  never the guarantee; the agreement was, and libgit2 — which has no recount
  knob — is still shipped an honest body.
- **Appliability is the engine's answer, not a property of the prose.** Where a
  count of failing hunks is wanted, each hunk is checked through the port's
  patch-applicability read; git's `error: patch failed:` locations are no longer
  matched against header spans, which only ever worked under the CLI adapter.
- **`\ No newline at end of file` is a property of the line it qualifies**, so it
  survives exactly when that line does, instead of being a backslash-prefixed
  line every reader had to special-case.

The reasoning below stands as history, and its *considered options* still bind:
rebuilding patches from the **Display row** model remains wrong, because that
model pairs and drops rows to render.

---

Granular (hunk/line) stage and unstage need a patch to feed
`GitExecutor::apply_patch_to_index`. We filter the raw unified-diff text —
keeping git's original `@@` headers and splicing out unselected lines — rather
than rebuilding patches from the parsed row model. The row model drops or
mangles fidelity-critical details (function context in hunk headers, mode
changes, rename headers); git already computed them correctly, so we preserve
its output instead of re-deriving it. Unstage reverse-applies against the
index via a direction flag on `apply_patch_to_index`
(`git apply --cached --reverse`).

## Considered options

- **Rebuild from the parsed row model** (`Row` structs in `crates/turbogit-ui/src/ui/diff.rs`) —
  cleaner data flow, but synthesizing headers and line counts re-derives what
  git produced and risks subtle mismatches.
- **`git add -p` / interactive add** — interactive-only, unusable from a GUI.

## Consequences

- The raw diff text must remain available wherever granular selection happens;
  the row model alone is not sufficient to compose patches.
- Selections are ephemeral UI state keyed by path: cleared when the file's
  diff cache key changes and after each successful granular operation on that
  file, because hunk indices shift whenever the diff changes.
- A file with active partial staging commits as-is from the index at commit
  time; its checkbox triggers no whole-file re-stage.
