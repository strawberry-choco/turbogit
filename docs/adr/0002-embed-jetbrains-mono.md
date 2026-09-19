# Embed JetBrains Mono rather than system font lookup

The redesign's fixed pane widths (320px changelist, 210px branches, 44px
gutter) and dense row metrics were measured against JetBrains Mono. We
decided to embed the Regular and Bold weights (~160KB each, OFL license
permits redistribution) as binary includes rather than doing system font
lookup with fallback chains.

System lookup would make text metrics vary per machine, breaking fixed-width
layout assumptions differently on every machine — an unreproducible layout
bug class. Deterministic rendering across machines outweighs binary size in
a desktop application. Consolas/Segoe UI remain as fallbacks only for
font-load failure, and layouts must tolerate that degradation (§3.1).

## Correction: the chains as actually registered

The decision above stands — every leading face is an embedded binary, never a
system lookup. What this summary implied about *which* face leads each family
was wrong, and `theme::font_definitions()` is the authority:

- **Monospace (data):** embedded JetBrains Mono Regular, then egui's built-in
  fallbacks, then `Consolas` — appended only when `consola.ttf` loads.
- **Proportional (chrome):** egui's embedded `Ubuntu-Light`, then the built-in
  fallbacks, then `Segoe UI` — appended only when `segoeui.ttf` loads.
  JetBrains Mono Bold is registered under the named family
  `jetbrains-mono-bold` for real bold.
- `Ubuntu-Light` ships inside egui (`epaint_default_fonts`), so leading the
  chrome chain with it keeps this ADR's rule intact while interface labels read
  in a proportional face instead of the data face.

An unloadable system face is omitted from its chain entirely rather than listed
without font data — epaint panics on a family member with no data, so the
documented degradation is by omission.
