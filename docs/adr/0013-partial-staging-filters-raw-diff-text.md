# Partial staging composes patches by filtering raw diff text

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

- **Rebuild from the parsed row model** (`Row` structs in `src/ui/diff.rs`) —
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
