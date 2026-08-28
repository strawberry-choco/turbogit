# Diff Viewing & Editing — Product Research Execution Plan

> **Purpose.** Build an evidence base that informs TurboGit's diff viewing and editing
> feature design by benchmarking how leading IDEs, editors, and standalone git tools
> handle diffs. This plan describes *how to run the research*, not the research itself.

---

## 1. Goal & Scope

**Objective.** Produce a defensible, side-by-side understanding of how the market
renders and lets users act on diffs, then translate findings into a prioritized
TurboGit diff feature spec.

**In scope**
- *Diff viewing*: unified vs side-by-side rendering, word/char-level highlighting,
  syntax highlighting, image/binary diffs, rename detection display, large-file and
  large-diff handling, whitespace/ignore options, inline blame/annotation, navigation
  and search within a diff.
- *Diff editing*: inline edit of the working tree, partial staging (file / hunk / line),
  merge-conflict resolution UI, split-pane editing of conflicting versions, undo/redo.

**Out of scope.** Merge algorithm internals, hosting-provider CI, non-git VCS
(Mercurial/SVN), server-side code review workflows (kept only as a reference point).

---

## 2. Tool Landscape (research subjects)

Group into comparable cohorts so scoring stays apples-to-apples:

| Cohort | Tools |
| --- | --- |
| **IDEs** | VS Code, JetBrains family (IntelliJ/PyCharm/WebStorm/Rider), Visual Studio, Xcode, Eclipse, Zed, Fleet |
| **Modal / terminal editors** | Neovim (+ `diffview.nvim`, `neogit`), Emacs Magit, Helix, Sublime Text |
| **Standalone Git GUIs** | GitKraken, Sourcetree, Fork, Tower, SmartGit, Sublime Merge, GitHub Desktop, GitUp, `lazygit`, `tig` |
| **Diff specialists** | difftastic, `git-delta`, `icdiff`, Kaleidoscope, Beyond Compare, Meld, P4Merge, WinMerge |

> Freeze this cohort list at P0. Adding tools later is scope creep unless justified.

---

## 2.5 Storm Research Method (kickoff)

Before any hands-on evaluation, run a **storm research session** — a time-boxed,
collaborative kickoff (60–90 min) whose goal is to converge on *what we need to learn*
and *what we assume*, so the structured work in §3–§6 measures the right things instead
of drifting into feature tourism.

**Format**
- Assemble 2–4 stakeholders (product, design, engineering — for TurboGit: the
  maintainer + a heavy git-user proxy).
- Use a shared board (Miro / FigJam / whiteboard) with three columns:
  **Questions**, **Hypotheses**, **Assumptions**.
- Time-box: 15 min silent solo capture → 20 min round-robin share & clustering →
  15 min dot-vote prioritization → 10 min define "good" per rubric dimension.

**Outputs (committed to `research/storm-session.md`)**
- A ranked **research-question backlog** (e.g. *"Which tools make partial line-staging
  discoverable without docs?"*).
- A prioritized **hypothesis list** with a predicted winner per diff capability — later
  confirmed or disconfirmed in the feature matrix.
- An explicit list of **assumptions to validate or kill** (e.g. *"users prefer
  side-by-side over unified"*).
- A one-line definition of **"good"** for each §5 rubric dimension, so scoring stays
  consistent across tools and reviewers.

**Rule.** The storm session feeds the workstreams — every structured test task (§6) and
every feature-matrix column should trace back to at least one storm question or
hypothesis. Orphan items with no storm backing are candidates for cut.

---

## 3. Workstreams

- **A. Landscape mapping** — categorize each tool: license, platform, price, target user,
  git-engine model (libgit2 / CLI / built-in).
- **B. Feature matrix** — enumerate every diff capability per tool (binary yes/no matrix).
- **C. UX pattern study** — capture interaction flows for staging, conflict resolution,
  navigation; note keyboard-first vs mouse-driven design.
- **D. Performance probing** — time-to-render on large diffs, memory behavior, scroll
  smoothness on a 5k-line change and a 10k-commit history.
- **E. Gap & opportunity analysis** — where do incumbents fall short (partial-staging UX,
  conflict-resolution friction, monorepo scale, accessibility)?
- **F. Synthesis** — convert evidence into prioritized TurboGit recommendations.

---

## 4. Methodology

- **Hands-on testing.** Install/launch each tool and run the scripted task list (§6).
- **Evidence capture.** Screenshots + short screen captures saved to `research/evidence/<tool>/`.
- **Secondary sources.** Official docs, changelogs, GitHub issues/PRs, HN & Reddit
  (r/git, r/neovim, r/emacs), G2/Slant reviews for sentiment.
- **Version pinning.** Record the exact version tested (and date) to guard against drift.

---

## 5. Evaluation Rubric (score 1–5 per dimension)

1. **Rendering fidelity** — word-diff accuracy, syntax highlighting, synced side-by-side scroll.
2. **Conflict-resolution UX** — clarity of 3-way view, speed, error recovery.
3. **Partial-staging granularity** — file / hunk / line, and how discoverable it is.
4. **Navigation & search** — jump between changes, filter, inline search.
5. **Performance & scalability** — render time and smoothness on large diffs.
6. **Keyboard-first / command palette** — can power users avoid the mouse?
7. **Theming & accessibility** — dark/light, contrast, font scaling, screen-reader friendliness.
8. **Cross-platform consistency** — behavior parity across Win/macOS/Linux.
9. **Extensibility** — API / plugin hooks for custom diff semantics.

---

## 6. Scripted Test Tasks (applied to *every* tool)

1. Open a **5,000-line** changed file → measure render time and scroll smoothness.
2. **Stage a single hunk**, then **unstage a single line**.
3. Resolve a **3-way merge conflict** (take ours / take theirs / manual edit).
4. View an **image diff** and a **renamed file** diff.
5. **Search within** a diff; jump between changes.
6. Toggle **ignore-whitespace** and confirm behavior.

Record results per tool in `research/results/<tool>.md`.

---

## 7. Deliverables

- `research/storm-session.md` — storm research outputs (questions, hypotheses, assumptions, "good" definitions).
- `research/landscape.md` — cohort map and positioning.
- `research/feature-matrix.csv` — capability matrix (one row per tool, one column per feature).
- `research/ux-patterns.md` — annotated screenshots + flow notes.
- `research/perf-notes.md` — benchmark table.
- `research/gap-analysis.md` — opportunities and white space.
- `research/recommendations.md` — prioritized TurboGit diff feature spec.

---

## 8. Phases & Timeline (part-time estimates)

| Phase | Work | Est. |
| --- | --- | --- |
| **P0** | Setup: environment, acquire tools (incl. trials), **storm research session**, finalize task script | 1 d |
| **P1** | Landscape + feature matrix (A, B) | 2–3 d |
| **P2** | UX deep-dives (C) | 3–4 d |
| **P3** | Performance probing (D) | 1–2 d |
| **P4** | Synthesis & recommendations (E, F) | 2 d |

**Total ≈ 2–3 weeks part-time.**

---

## 9. TurboGit-Specific Implications

Translate findings into the `egui` + Rust reality of the project:
- **Virtualized rendering** — diff view must page/virtualize large files (mirror the
  `egui_extras::Table` approach already used elsewhere in TurboGit).
- **Keyboard-driven staging** — map the best partial-staging shortcuts onto egui accelerators.
- **Conflict-resolution modal flow** — design a 3-way split using egui `ScrollArea` +
  the trailing-spacer guard already documented for wrapped scroll areas.
- **Word-level diff reuse** — evaluate patience/myers + a difftastic-style semantic
  highlighter rather than hand-rolling.
- **Large-diff paging** — adopt the "load N hunks, lazy-load rest" pattern seen in
  GitKraken/Fork.

---

## 10. Risks & Mitigations

- **Paid tools** (Tower, Beyond Compare, Kaleidoscope) → use time-limited trials and
  lean on documented behavior + reviews where hands-on isn't possible.
- **Version drift** → pin versions, date-stamp every artifact.
- **Reviewer bias** → enforce the rubric; test with ≥2 independent sample repos.
- **Scope creep** → freeze cohort list at P0; defer new tools to a v2 pass.

---

## 11. Success Criteria

- Every in-scope tool assessed on the §5 rubric with captured evidence.
- Feature matrix complete and cross-checked against secondary sources.
- ≥3 concrete TurboGit diff features justified by direct evidence from the research.
