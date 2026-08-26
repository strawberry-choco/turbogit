# Diff rendering virtualizes over `ScrollArea::show_rows`, not `egui_extras::Table`

R1 (large-diff smoothness) needs virtualization, and the research spec named
`egui_extras::Table` while the ranking doc named `ScrollArea::show_rows`. We
chose `show_rows` over a cached display-row model: it preserves the existing
custom painter-based cells and pixel output exactly, works naturally with the
two-pane side-by-side layout, and needs only a stable display-row count and
uniform row height (already true). `Table` would impose its own cell/widget
model against custom gutters and paired cells for no gain. Parsing the raw
diff text into rows happens once per diff (memoized alongside `diff_cache`),
not per frame — paint virtualization alone would leave O(n) parsing on every
frame.

## Considered options

- **`egui_extras::Table`** (research recommendation) — rejected: fights custom
  gutter painting and side-by-side cell pairing.
- **`show_rows` without parse caching** — rejected: only removes paint cost;
  per-frame `parsed_rows()` remains O(total lines).

## Consequences

- Hunk navigation must be index-based: target hunk → first display-row index →
  `scroll_to_row`. Widget-level `resp.scroll_to_me` cannot reach unrealized
  rows. R7 keyboard nav (`F7`/`Shift+F7`) reuses this same hunk→row map.
- Hover, click toggles, and accessibility info exist for visible display rows
  only; no feature may depend on interacting with off-screen rows.
- Both render modes consume one cached display-row vector; unified mode
  ignores pairing. All diff surfaces (main viewer and commit-window preview)
  virtualize through the shared render path.
- Success criteria (validated by the parallel perf pass): ≤8 ms steady-state
  frame at 5k display rows, ≤16 ms at 20k, memory flat regardless of scroll
  position.
