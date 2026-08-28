# Gap & Opportunity Analysis

> Where incumbents fall short, derived from `feature-matrix.csv`, `ux-patterns.md`, and
> `perf-notes.md`. Each gap is scored for **impact** (how much users care) and **TurboGit
> feasibility** (given Rust/egui, from `docs/diff-capabilities-research-plan.md §9`). White
> space = gaps with high impact + high feasibility = build targets.

## Gap 1 — Accessibility is universally weak
- **Evidence.** No perspective in the storm session raised a11y; feature-matrix shows most
  tools mark `Accessibility` Partial/No. Terminal tools (Magit/Neovim/lazygit/tig/difftastic)
  are effectively screen-reader-blind for diff content; GUIs vary; only VS Code has a
  first-class a11y story.
- **Impact:** High (legal + ethical + broadens market). **Feasibility:** High (egui widgets
  are programmatically themable; contrast/font-scaling come for free if built in).
- **White space:** ✅ Strong differentiator, cheap to claim.

## Gap 2 — Structural / word-level diff absent from mainstream GUIs
- **Evidence.** difftastic (AST) and git-delta (word) prove feasibility; no IDE/GUI ships
  AST diff as default. VS Code only does inline char-level.
- **Impact:** Medium-High (reduces review errors per Academic perspective). **Feasibility:**
  Medium (difftastic memory cliff on large diffs; needs bounding/opt-in).
- **White space:** ✅ Differentiator if bounded correctly.

## Gap 3 — Partial-staging discoverability split
- **Evidence.** Magit/lazygit maximize keyboard flow but hide it; GitHub Desktop maximizes
  discoverability (checkboxes) but lacks power. No tool is great at *both*.
- **Impact:** High (top capability per universal agreement). **Feasibility:** High (egui can
  do gutter stage buttons + command palette verbs).
- **White space:** ✅ Build both modes.

## Gap 4 — Conflict resolution: guided vs delegated
- **Evidence.** IDEs/GUIs give guided 3-way (VS Code merge editor, Tower wizard); terminal
  tools largely delegate to an external editor (lazygit). Specialists (Beyond Compare/Meld)
  own high-fidelity 3-way but aren't Git clients.
- **Impact:** High. **Feasibility:** High (egui 3-pane + trailing-spacer ScrollArea guard
  already documented in project MEMORY).
- **White space:** ✅ Native 3-way modal is squarely buildable.

## Gap 5 — Linux parity gap among paid GUIs
- **Evidence.** Fork, Tower, GitHub Desktop, Kaleidoscope, GitUp are Windows/macOS-only.
- **Impact:** Medium (Linux devs underserved by polished GUIs). **Feasibility:** High
  (Rust/egui is 3-OS by construction).
- **White space:** ✅ Positioning advantage, not a feature to build.

## Gap 6 — AI-guided conflict resolution is nascent
- **Evidence.** VS Code 1.105 (Sept 2025) added agentic merge resolution; competitors have
  not broadly followed.
- **Impact:** Emerging/Medium. **Feasibility:** Medium (depends on an LLM integration the
  project may not want to own).
- **White space:** ⚠ Optional future wedge; not core.

## Gap 7 — Large-diff smoothness in Electron/JVM clients
- **Evidence.** GitKraken/VS Code/GitHub Desktop (Electron) and SmartGit/JetBrains (JVM)
  cited as heavy on large repos.
- **Impact:** Medium. **Feasibility:** High (Rust/egui native + existing virtualization).
- **White space:** ✅ Flow advantage if virtualization is done right.

## Prioritized Opportunity Shortlist
| Rank | Opportunity | Impact | Feasibility | Build? |
| --- | --- | --- | --- | --- |
| 1 | Accessible, themable diff (dark/light + scaling + contrast) | High | High | ✅ Now |
| 2 | Native 3-way conflict modal (keyboard + mouse) | High | High | ✅ Now |
| 3 | Dual-mode partial staging (gutter buttons + palette verbs) | High | High | ✅ Now |
| 4 | Virtualized rendering for 5k-line diffs | High | High | ✅ Now |
| 5 | Word/structural diff toggle (bounded) | Med-High | Med | ◐ Phase 2 |
| 6 | AI-assisted conflict resolution | Med | Med | ⚠ Phase 3 (optional) |

## Risks / Threats to Validity
- **Evidence bias:** terminal/keyboard bias from maintainer (see storm peer review).
- **Hands-on gap:** perf numbers are documented, not measured (see `perf-notes.md`).
- **Version drift:** pinned 2026-08-02; re-verify before build starts.
