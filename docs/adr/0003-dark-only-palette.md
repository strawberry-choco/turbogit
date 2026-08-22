# Dark-only palette; Light and HighContrast modes deleted

The HTML mockups define exactly one palette (Darcula-derived dark). We decided
to delete `ThemeMode::{Light, HighContrast}` and ship dark-only: `palette()`
returns the single token set, widgets never branch on mode, and the settings
dialog loses the theme row.

The obvious alternative — keeping the old light/high-contrast visuals "until a
light palette is designed" — was rejected because maintaining three palettes
doubles every widget's color-mapping work for modes no mockup validates.
Consequence: there is genuinely no light mode until someone designs one; the
`ThemeMode` enum and per-mode `configure_style` branches are gone rather than
stubbed. If a light palette is ever designed, it re-enters through the
`Palette` struct as a new token set, not by resurrecting deleted code paths.
