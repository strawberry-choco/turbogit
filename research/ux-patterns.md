# UX Pattern Study — Diff Interaction Flows

> Secondary-source synthesis (docs, reviews, HN/Reddit, vendor guides) of how each cohort
> handles the three highest-leverage flows from `storm-session.md`: **staging**,
> **conflict resolution**, and **navigation**. Keyboard-first vs mouse-driven is noted per
> flow. See `feature-matrix.csv` for the binary capability map.

## 1. Staging Flow (file / hunk / line)

### Magit (Emacs) — *discoverability reference*
- Flow: open Magit status → `s` stage, `u` unstage; `s` on a hunk stages the hunk; with point
  in a hunk, `s` acts on the hunk and a transient prefix lets you stage a *region* (arbitrary
  lines). Everything is a keystroke; a transient menu (`C-s` staging menu) lists options.
- Keyboard-first: **fully**. Mouse: irrelevant.
- Lesson: staging is *one verb with context*, not a separate panel. Discoverability comes
  from a consistent verb + contextual scope.

### lazygit / tig (TUI) — *keyboard parity*
- lazygit: arrow to file → `s` stage file, `Enter` opens file panel → select hunk → `s`
  stage hunk, `v` stage a line range. Fuzzy menu (`/`) for commands.
- tig: `s` stages hunk/line in the stage view; `u` unstages.
- Keyboard-first: **fully**. The strength is speed; the weakness is discoverability for
  newcomers (no visible hints like Magit's transient).

### Fork / Sublime Merge / Tower / GitKraken (GUI) — *mouse + shortcuts*
- Fork: click a hunk's `+` to stage it, drag to select lines, right-click "Stage Lines"; or
  `Cmd+S`/`Cmd+U`. Image diffs shown inline.
- Sublime Merge: click line gutter to stage/unstage individual lines; keyboard mirrors it.
- GitKraken: drag file into "Staged" area; per-hunk stage via hover button; line staging
  via context menu.
- GitHub Desktop: checkboxes per file/hunk/line (the most *discoverable* GUI approach — no
  hidden gestures), but no side-by-side and conflict resolution delegates to an external
  editor.
- Keyboard-first: **partial** (shortcuts exist; mouse is the obvious path).

### VS Code (IDE) — *inline SCM*
- Click `+` in the diff gutter to stage a line; hover buttons stage hunk; command palette
  for "Git: Stage Selected Ranges". GitLens enriches with inline blame + heatmap.
- Keyboard-first: **partial** (palette covers most actions).

**Pattern takeaway.** Two viable models: (a) *verb+context* (Magit/lazygit) for power users,
(b) *explicit checkboxes/buttons* (GitHub Desktop/Fork) for discoverability. TurboGit should
offer both: clickable line-stage gutter buttons (discoverable) **and** keyboard verbs with a
command palette (power).

## 2. Conflict-Resolution Flow (3-way)

### VS Code Merge Editor (1.70+; AI in 1.105)
- Open conflicted file → "Resolve in Merge Editor" → 3 columns: **Incoming | Result |
  Current** (base shown contextually), with "Accept Current/Incoming/Both" per conflict and
  a conflict counter ("2 conflicts remaining") that jumps between conflicts. Result pane is
  editable. 1.105 adds "Resolve with AI" (agentic, uses merge base + both sides as context).
- Keyboard-first: **partial**; wizard buttons are mouse-friendly.

### Tower / GitKraken Conflict Wizard
- Tower: step-through wizard, per-conflict "choose ours/theirs," live preview, undo.
- GitKraken: 3-way inline editor with take-ours/theirs/both and manual edit; visual markers.
- Keyboard-first: **partial**.

### Magit / lazygit / smerge (terminal)
- Magit: `M-x smerge` style; `n`/`p` next/prev conflict, `a`/`b` take A/B, `m` take merged.
- lazygit: basic — opens file with markers, relies on external editor for fine resolution.
- Keyboard-first: **fully** (Magit), partial (lazygit).

### Beyond Compare / Meld / P4Merge (specialist mergetool)
- Beyond Compare Pro: 3 panes (ours | base | theirs) + output, manual edit in any pane,
  syntax highlight, byte-level for binaries.
- Meld: 3-way merge view, color-coded, editable.
- P4Merge: classic 3-pane, the *de facto* free mergetool.
- Keyboard-first: **partial** (mouse-oriented but scriptable).

**Pattern takeaway.** The winning shape is a **3-pane (ours/base/theirs) + editable result**
with per-conflict accept actions and a "jump to next conflict" counter. TurboGit's egui
`ScrollArea` + trailing-spacer guard (documented in project MEMORY) maps directly to this;
make each conflict independently actionable and keyboard-navigable.

## 3. Navigation & Search Flow

| Tool | Jump next/prev change | Inline search | Keyboard |
| --- | --- | --- | --- |
| VS Code | `F7`/`Shift+F7` (SCM) | editor find (`Cmd+F`) | Partial |
| JetBrains | `F7`/`Shift+F7` | inline find | Yes |
| Magit | `n`/`p` in diff | isearch | Yes |
| lazygit | arrow + filter | `/` fuzzy | Yes |
| Sublime Merge | `F4` next, `Shift+F4` prev | command-palette find | Yes |
| Fork | arrow + `Tab` | search box | Partial |
| GitKraken | click in minimap | search | Partial |
| difftastic/delta | pager keys | pager `/` | Yes (pager) |

**Pattern takeaway.** Two conventions: IDE "next change" (`F7`) and terminal "isearch/arrow".
TurboGit should adopt `F7`/`Shift+F7` (familiar to IDE users) **and** a `/` inline filter
(familiar to terminal users), unified under a command palette.

## 4. Presentation: Unified vs Side-by-Side
- Terminal/default lean **unified** (VS Code default, GitHub Desktop, tig); power diff users
  and specialists lean **side-by-side** (Sublime Merge, Beyond Compare, difftastic, Neovim
  vimdiff).
- Evidence (storm A3/A1) shows it is **task-dependent**, not a fixed preference. TurboGit
  should ship both with a one-key toggle and remember per-file choice.

## 5. Evidence Captured (planned)
`research/evidence/<tool>/` would hold screenshots + captures from hands-on runs. In this
sandbox, hands-on capture was **not performed**; flows above are reconstructed from official
docs/guides and community write-ups. Flagged as a follow-up for a future hands-on pass
(see `gap-analysis.md` §Risks).
