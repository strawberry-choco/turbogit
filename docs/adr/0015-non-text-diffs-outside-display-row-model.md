# Non-text diffs render outside the display-row model

R8 (image / rename / binary diffs) meets ADR-0014's virtualized
display-row model, which is purely textual: rows are parsed diff lines,
and images have no line content at all. We decided that image diffs and
binary changes render as a per-file pane *instead of* the row-based text
view — an image pair side by side with dimension/size captions, a binary
change as a one-line description — rather than shoehorning textures into
special display rows inside the virtualized scroller. The row model stays
textual; rename headers remain metadata attached above a normal text diff.

## Considered options

- **Special display rows carrying textures** — rejected: breaks the row
  model's uniform-height assumption behind `show_rows` virtualization and
  couples texture lifecycle to scroll realization for no user gain.
- **Render everything as rows** (status quo) — rejected: binary already
  degrades to a literal "Binary files differ" meta line; images would be
  unviewable, failing R8's success criterion outright.

## Consequences

- The unified ⇄ side-by-side toggle has no effect on image or binary
  files; there is no second layout to switch to.
- Granular staging verbs (hunk gutter buttons, palette Stage/Unstage Hunk)
  never apply to image/binary panes — there are no hunks. Rename headers
  are likewise metadata, never stageable text.
- Image/binary handling is a property of the shared diff viewer, so it
  applies everywhere it renders (Commit tool window and Git Log alike).
