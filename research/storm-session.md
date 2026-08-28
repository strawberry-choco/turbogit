# Storm Research Session — Diff Viewing & Editing Capabilities

> **Method.** This artifact was produced by adapting the **STORM** research method
> (Stanford OVAL, NAACL 2024) to the kickoff described in
> `docs/diff-capabilities-research-plan.md §2.5`. STORM compresses broad research by
> simulating expert perspectives, mapping contradictions, synthesizing, and self-critiquing.
> Every structured test task (§6 of the plan) and every feature-matrix column traces back
> to at least one item below, per the plan's "no orphan" rule.
>
> **Environment note.** This research was executed with *secondary sources only*
> (official docs, changelogs, GitHub issues/PRs, HN/Reddit, G2/Slant reviews, vendor
> pricing pages) plus the maintainer's domain knowledge. Hands-on GUI installation/launch
> of proprietary tools (Tower, Beyond Compare, Kaleidoscope, GitKraken, Fork, etc.) is not
> possible in this sandbox; where hands-on data would be ideal, the gap is flagged
> explicitly in `perf-notes.md` and `gap-analysis.md`.

---

## A. Multi-Perspective Scan

### 1. Practitioner (daily heavy git user)
**Position.** In real work the diff tool is judged by *flow*, not features: how fast can I
stage one line, jump to the next conflict, and get back to coding. Discoverability of
partial staging matters more than a long capability list.
**Strongest evidence.** Magit and lazygit win loyalty almost entirely on keyboard-driven
hunk/line staging flows; Sublime Merge and Fork win on native feel + speed.
**Only they would say.** "If I have to reach for the mouse to stage a line, the tool is
already slower than `git add -p` in a terminal."

### 2. Academic (HCI / diff-algorithm researcher)
**Position.** Word/char-level and *structural* (AST) diffing measurably reduce review
errors versus line diffs, but most mainstream GUIs still ship only line-level highlighting.
**Strongest evidence.** Difftastic (structural, tree-sitter, Dijkstra tree-matching) and
git-delta (word-level via patdiff) demonstrate that syntax-aware diffing is feasible and
fast enough for everyday use; VS Code's inline char-diff is the only mainstream GUI nod.
**Only they would say.** "Unified vs side-by-side is a presentation choice with weak
evidence either way; the higher-value axis is *granularity of change detection*, which the
market under-serves."

### 3. Skeptic (thinks GUIs are overrated)
**Position.** Every "must-have" diff GUI capability already exists in `git` + a terminal
pager; GUIs mostly re-skin `git diff` and add latency (Electron memory, slow large-repo
rendering).
**Strongest evidence.** git-delta + lazygit/gitui (Rust TUI) deliver equal or better
diffing at a fraction of the RAM; VS Code/GitKraken are routinely cited as "slow on very
large repos" in reviews.
**Only they would say.** "TurboGit should not try to out-GUI the GUIs — it should out-*flow*
them with a native, virtualized, keyboard-first Rust/egui surface."

### 4. Economist (follows the money)
**Position.** The paid-GUI market (Tower $69/yr, Fork $59 one-time, Sublime Merge $99,
SmartGit $99/yr commercial) survives on *polish + safety* (undo, conflict wizards), not on
diff-rendering novelty, which is commoditized.
**Strongest evidence.** 2026 pricing roundups show Fork and Tower retaining loyal
Mac/Win bases on "feel"; GitKraken leans on team/integration lock-in; free tools
(GitHub Desktop, lazygit, difftastic, delta) cover raw diffing.
**Only they would say.** "The monetizable wedge is *conflict resolution + undo/safety + AI
assist*, not the diff view itself — VS Code 1.105 just added AI merge-conflict resolution,
showing where vendors expect to add value."

### 5. Historian (seen diff tools evolve)
**Position.** Diff UX has cycled: command-line → side-by-side graphical → inline/3-way
merge editors → AI-assisted. Each leap reduced *cognitive load* of reconciling two states.
**Strongest evidence.** From `diff`/`vimdiff` → Meld/P4Merge 3-way → VS Code merge editor
(1.70+) → VS Code 1.105 AI resolution; the trajectory is toward *guided* conflict
resolution, not just prettier rendering.
**Only they would say.** "TurboGit enters at the AI-guided-resolution inflection point;
ignoring that trend means building a 2018-era tool."

---

## B. Contradiction Map

1. **GUI richness vs. terminal speed.** Practitioner + Skeptic say speed/flow beats
   features; Academic + Economist say users pay for polish/safety. *Direct conflict on what
   "good" means.*
2. **Line diff vs. structural diff.** Academic says structural is clearly better; Skeptic
   says line diff in a fast pager is "good enough" and structural has memory cliffs.
3. **Strongest evidence: Practitioner + Historian** (flow + guided-resolution trajectory)
   — both rest on broad, repeated user behavior. **Weakest: Economist's "AI is the wedge"**
   — newest, least proven, vendor-driven.
4. **Resolving question:** *Does native speed + keyboard-first flow beat feature-rich GUI
   polish for the target user?* If answered "yes", TurboGit should prioritize virtualized
   rendering + staging ergonomics over matching every GUI bell.
5. **Universal agreement:** Partial-staging granularity (file/hunk/line) and conflict
   resolution are the two highest-leverage diff capabilities. Everyone converges here.
6. **Blind spot (none addressed):** *Accessibility.* No perspective raised screen-reader /
   contrast / font-scaling diff UX — yet the rubric §5.7 lists it. This is the most
   under-served dimension across incumbents and a cheap differentiator for TurboGit.

---

## C. Synthesis Briefing

**One-paragraph summary.** The diff-tool market is split between heavy GUI clients that win
on polish, safety (undo/conflict wizards), and integration, and lightweight terminal/Rust
tools that win on speed and keyboard flow. The genuinely under-served, high-value axes are
(1) *granularity* of change detection — word/char and structural diffing is proven but
rarely shipped in mainstream GUIs — and (2) *accessibility*. The strategic inflection is
AI-guided conflict resolution (VS Code 1.105), which TurboGit can adopt cheaply.

**5 Key Findings (ranked by reliability)**
1. **Partial staging (file/hunk/line) + conflict resolution are the top capabilities.** All
   perspectives agree. *(Reliability 10/10.)*
2. **Structural/word-level diffing is feasible and better, but absent from most GUIs.**
   Backed by difftastic 0.67 + git-delta + VS Code char-diff. *(Reliability 8/10.)*
3. **Native speed/virtualization beats Electron polish for flow.** Practitioner + Skeptic +
   multiple 2026 reviews. *(Reliability 7/10.)*
4. **Monetizable wedge is safety/AI-assist, not the diff view.** Economist + VS Code 1.105.
   *(Reliability 5/10 — newest, vendor-driven.)*
5. **Accessibility is an unclaimed differentiator.** Rubric + blind-spot analysis.
   *(Reliability 7/10 — logical, lightly evidenced.)*

**Hidden connection.** The same engineering investment that gives TurboGit *virtualized
rendering* (needed for 5k-line diffs) also unlocks *accessibility* (consistent,
programmatically-themed widgets) — the two under-served axes share one root cause
(non-native/Electron bloat in incumbents).

**Actionable insight (for the TurboGit maintainer).** Do **not** try to match every GUI
feature. Build a native, virtualized, keyboard-first diff surface with (a) file/hunk/line
staging, (b) a 3-way conflict modal, (c) word/structural diff toggle, and (d) first-class
dark/light + font-scaling + contrast theming. That covers the universally-agreed top
capabilities *and* the two cheap differentiators, with less engineering than cloning a GUI.

**Frontier question.** *Can a Rust/egui structural diff (difftastic-style AST matching)
stay memory-bounded on 5k-line refactors so it can be the default, not an opt-in?*

---

## D. Peer Review

- **Confidence scores.** F1 10/10 (universal agreement). F2 8/10 (strong tool evidence,
  some GUI-absence inference). F3 7/10 (review-supported). F4 5/10 (newest claim).
  F5 7/10 (logical + rubric).
- **Weakest link.** F4 (AI-assist as the wedge) — verify by checking adoption of VS Code
  1.105's AI merge feature and competitor roadmap (GitKraken/Fork AI plans) in 2026.
- **Bias check.** Practitioner + Skeptic (terminal/keyboard bias) were overrepresented
  because the maintainer is a Rust/egui builder. Mitigate by weighting GUI-user reviews
  (G2/Slant) in `feature-matrix.csv` and `ux-patterns.md`.
- **Missing perspective.** *Enterprise/compliance buyer* (SmartGit's audience) — relevant
  if TurboGit targets teams; add if scope expands.
- **Overall grade.** B+. Strong, actionable convergence; F4 needs evidence; accessibility
  blind spot converted into a recommendation.

---

## E. Storm Deliverables (required by plan §2.5)

### E.1 Ranked Research-Question Backlog
1. Which tools make partial line-staging *discoverable without docs*? (Magit/lazygit vs GUI)
2. How do incumbents render 5k-line diffs — virtualized or一次性? (perf gap)
3. Is side-by-side or unified preferred by real users, or does it depend on task? (assumption test)
4. How do tools handle 3-way conflict resolution — wizard, split-pane, or delegate to editor?
5. Where does structural/word-level diffing appear in mainstream GUIs, and at what cost?
6. What accessibility (screen-reader/contrast/scaling) do diff tools actually offer?
7. Can a Rust/egui surface match Sublime Merge's render speed?

### E.2 Prioritized Hypotheses (predicted winner per capability)
| # | Hypothesis | Predicted winner |
| --- | --- | --- |
| H1 | Fastest large-diff render | Sublime Merge (custom engine) |
| H2 | Best partial-staging discoverability | Magit / lazygit (transient + keyboard) |
| H3 | Best 3-way conflict UX | VS Code merge editor / Tower wizard |
| H4 | Best structural diff | difftastic (AST) |
| H5 | Best keyboard-first flow | Neovim+neogit / Magit / Sublime Merge |
| H6 | Best accessibility | VS Code (built-in a11y + extensions) |
| H7 | Best cross-platform parity | GitKraken / SmartGit (true 3-OS) |

### E.3 Assumptions to Validate or Kill
- **A1 "Users prefer side-by-side over unified."** → *Kill/qualify*: evidence shows task-dependent; offer both, default unified with toggle.
- **A2 "GUIs are necessary for good diff UX."** → *Qualify*: terminal/Rust tools match on flow; GUIs win on safety/polish only.
- **A3 "Syntax highlighting requires an IDE."** → *Kill*: bat/git-delta + difftastic prove standalone syntax-aware diff.
- **A4 "Conflict resolution needs a GUI."** → *Qualify*: 3-way works in TUI (smerge/lazygit); GUI adds guided ergonomics.

### E.4 "Good" Definitions per Rubric Dimension (plan §5)
1. **Rendering fidelity** — word/char changes are highlighted *within* a line, syntax is colored, and side-by-side panes scroll in lockstep.
2. **Conflict-resolution UX** — a 3-way view shows base/ours/theirs, lets you accept per-hunk, and recovers gracefully from a bad edit (undo).
3. **Partial-staging granularity** — you can stage/unstage at file, hunk, *and* arbitrary line level, and the action is discoverable without docs.
4. **Navigation & search** — you can jump next/prev change and inline-search the diff with the keyboard.
5. **Performance & scalability** — a 5k-line change renders without freezing and scrolls smoothly; memory stays bounded.
6. **Keyboard-first / command palette** — every staging/conflict action has a keybinding; power users never need the mouse.
7. **Theming & accessibility** — dark/light, user font scaling, sufficient contrast, and screen-reader-legible diff output.
8. **Cross-platform consistency** — identical behavior and keybindings on Windows/macOS/Linux.
9. **Extensibility** — a plugin/API hook to supply custom diff drivers or semantic highlighters.
