# Recommendations — Prioritized TurboGit Diff Feature Spec

> Translates the evidence (`storm-session.md`, `feature-matrix.csv`, `ux-patterns.md`,
> `perf-notes.md`, `gap-analysis.md`) into a build plan grounded in TurboGit's **Rust +
> egui** reality (`docs/diff-capabilities-research-plan.md §9`). Each recommendation cites
> the evidence and the existing project primitive it reuses.

## Design Principles (from research)
1. **Don't out-GUI the GUIs — out-flow them.** Native, virtualized, keyboard-first.
2. **Dual-mode everything.** Discoverable (mouse/gutter) AND fast (keyboard/palette).
3. **Accessibility is a feature, not a retrofit.** Build theming + scaling + contrast in.
4. **Bounded semantic diff.** Offer word/structural diff, but never let it freeze the UI.

---

## R1 — Virtualized Diff View (P0, highest priority)
- **Evidence:** Universal agreement that large-diff smoothness matters (perf-notes: native >
  Electron); `egui_extras::Table` virtualization already used elsewhere in TurboGit.
- **Spec:** Render diff as a virtualized row model (one row per diff line). Reuse
  `egui_extras::Table`. Adopt the **trailing-spacer guard** (`ui.add_space(0.37)`) documented
  in project MEMORY to avoid the egui 0.35 `ScrollArea` `remap_clamp` panic on exact-fit
  content (already applied in `diff.rs`, `commit_window.rs`, `log_window.rs`).
- **Success:** 5k-line file scrolls smoothly, memory bounded.

## R2 — Dual-Mode Partial Staging (P0)
- **Evidence:** gap #3 — no tool is great at both discoverability and power; top capability
  per universal agreement.
- **Spec:**
  - *Discoverable mode:* clickable gutter `+`/`−` per line and per-hunk stage/unstage
    buttons (GitHub Desktop model).
  - *Power mode:* keyboard verbs (`s` stage / `u` unstage) with context (file→hunk→line
    selection) + a command palette entry "Stage Selected Lines" (Magit/lazygit model).
  - Engine: route through `GitExecutor` (CLI `git add -p`-style line staging via
    `git apply` of selected hunks) — the project's primary, zero-C-dep path.
- **Success:** stage a hunk then unstage a single line, both by mouse and by key.

## R3 — Native 3-Way Conflict Modal (P0/P1)
- **Evidence:** gap #4; VS Code merge editor + Tower wizard are the bar; egui `ScrollArea`
  + trailing-spacer guard already documented for this exact use.
- **Spec:** 3-pane modal — **Ours | Base | Theirs** — with an editable Result pane. Each
  conflict independently actionable (Accept Ours/Theirs/Both) with a "N conflicts remaining"
  jump counter (`F7`/`Shift+F7`). Use egui `ScrollArea` + `add_space(0.37)` guard per MEMORY.
- **Success:** resolve a 3-way conflict take-ours / take-theirs / manual edit without
  leaving TurboGit.

## R4 — Theming, Font-Scaling & Contrast (P0)
- **Evidence:** gap #1 — accessibility is the unclaimed differentiator; egui primitives are
  programmatically themable (`ctx.all_styles_mut`, `Visuals`, `CornerRadius` per MEMORY).
- **Spec:** dark/light toggle, user font-scale slider, guaranteed contrast tokens. Hook into
  the existing `theme.rs` restyle path. Ensure diff color tokens (add/remove/conflict) meet
  WCAG AA at all scales.
- **Success:** passes contrast check at 100%–200% scaling in both themes.

## R5 — Unified ⇄ Side-by-Side Toggle (P1)
- **Evidence:** storm A1/A3 — presentation is task-dependent, not a fixed preference; both
  should exist with a one-key toggle and per-file memory.
- **Spec:** toggle key (e.g. `Ctrl/Cmd+\`), persist choice per file path.
- **Success:** user flips presentation without losing scroll/selection state.

## R6 — Word / Structural Diff Toggle (P2, bounded)
- **Evidence:** gap #2; difftastic 0.67 (AST) + git-delta (word) prove value but have a
  memory cliff (perf-notes).
- **Spec:** integrate **git-delta** (word-level, cheap, pager) as the default "refine"
  highlighter; expose **difftastic** (structural) as an *opt-in* driver for recognized
  languages, with a hard line/change-count ceiling (e.g. skip structural if file > 2k
  changed lines) to avoid the memory cliff. Reuse `GitExecutor` to shell out; do not vendor
  the parsers into the app core.
- **Success:** word-level highlight on by default; structural available without freezing UI.

## R7 — Navigation & Search (P1)
- **Evidence:** ux-patterns §3 — dual convention (`F7` IDE + `/` terminal).
- **Spec:** `F7`/`Shift+F7` jump next/prev change; `/` inline filter over diff; both routed
  through the command palette.
- **Success:** jump between 10 changes and filter by substring, all keyboard.

## R8 — Image / Rename / Binary Diff (P1)
- **Evidence:** feature-matrix — most GUIs support image diff; rename display is near-universal.
- **Spec:** reuse git's rename detection for display; render image diffs side-by-side with
  slider/overlay; show binary change description (difftastic-style "binary file changed").
- **Success:** view an image diff and a renamed-file diff natively.

## R9 — (Optional, Phase 3) AI-Guided Conflict Resolution (P3)
- **Evidence:** gap #6 — VS Code 1.105 added agentic merge resolution; nascent.
- **Spec:** defer; design the conflict modal (R3) so an optional LLM backend can be plugged
  in later without restructuring. Do **not** block P0/P1 on this.

---

## Build Sequence (maps to plan §8 phases)
| Phase | Recommendations |
| --- | --- |
| P0 (now) | R1 Virtualized view, R2 Dual-mode staging, R3 Conflict modal, R4 Theming/a11y |
| P1 | R5 Toggle, R7 Nav/search, R8 Image/rename/binary |
| P2 | R6 Word/structural diff (bounded) |
| P3 (optional) | R9 AI-assisted conflicts |

## Success Criteria (plan §11, restated for TurboGit)
- ≥3 concrete features justified by direct evidence: **R1, R2, R3** all trace to universal-
  agreement findings + existing project primitives.
- Accessibility (R4) claims a white space no incumbent owns.
- Semantic diff (R6) is delivered *bounded*, directly addressing the difftastic memory cliff
  risk identified in perf-notes.

## Open Items / Follow-ups
- **Hands-on perf pass** (real 5k-line/10k-commit samples) — biggest evidence gap.
- **Verify 2026 pricing/versions** before build kickoff (pinned 2026-08-02).
- **Enterprise/compliance perspective** — add if TurboGit targets teams.
