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
