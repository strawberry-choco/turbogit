# R3 session 3 — the residual capture list, 2026-10-06

Closed most of what sessions 1–2 left open, using the proven toolchain
(System Events for menus, the JXA `CGEvent` bridge for clicks/hover/scroll,
`screencapture -R`). Instance: the clean-config one from session 1, fixture
open. Second launch with a **brand-new config dir** (`/tmp/rebased-r3-first`)
for the first-run wizard. Screenshots: `screenshots/s3-*.png`, `wizard-*.png`.

## Closed this session

### Advanced Settings checkbox states (`s3-fp-s.png`, `s3-sub-s.png`)
- **`Use Safe Force Push` — ON** ("Use '--force-with-lease' when calling force
  push from IDE instead of just '--force'") — Version Control.Git group.
- **`Recursively clone submodules in the project` — ON** ("Checkout from Git
  calls 'git clone --recurse-submodules'").
- **Negative results, recorded:** searching Advanced Settings for
  `cherry-pick` and `compact references` returns **"Nothing found"** — the
  `On empty cherry-pick commit:` policy lives in the cherry-pick dialog, and
  `Compact References View` in the Log's gear menu, not in settings.

### Commit tool window topology (`s3-commit-s.png`, §5.3/§7.2)
Left Commit panel: icon toolbar (refresh, compare, tag, eye, move, split, close)
+ kebab; changes tree with `Changes 1 file` / `Unversioned Files 4 files`
checkbox groups, per-row status chip + root suffix (`M+ README.md  tg-r3-fixture`);
bottom commit card: `☐ Amend last commit ▾` + history icon, Commit Message
textarea, and the commit split as **two side-by-side buttons — `Commit` (primary)
+ `Commit and Push…` (secondary)** — with a settings gear at the card's right
end. The Log pane shows its **vertical `Branches` strip** on the left edge, the
five-icon filter toolbar, and the graph lane with refs rendered as chips on the
right of the message column.

### The ⌃O commit-options popup (`s3-opts-s.png`) — a finding, not a checkbox list
This build's popup contains a **`Git` group (`Author:` field, `☐ Sign-off
commit`)** and a **`Commit Checks` group** — the docs' six IDE checks
(reformat/rearrange/imports/cleanup/copyright/deps) are **absent from the
product**, consistent with §8.3 #1. The spec's §5.3 "six commit options" row
describes the docs' IDEA, not this build.

### Log commit-row context menu — ORDER (`s3-ctx-s.png`) — closes a §7.2 item
On a pushed commit, verbatim order with separators: Copy Revision Number
(⌥⌘C) · Create Patch… · Cherry-Pick | Checkout Revision · Show Repository at
Revision · Compare with Local | Reset Current Branch to Here… · Revert Commit ·
**Undo Commit… (disabled)** | Edit Commit Message… (F2, **disabled**) ·
Fixup… · Squash Into… · Drop Commits · Squash Commits… · Interactively Rebase
from Here… (**all disabled — the commit is pushed**) | Push All up to Here… |
New Branch… (⌥⌘N) · New Tag… | Go to Child Commit (←) · Go to Parent Commit (→).
**§4.7's "safety lives in enablement, not confirmation" is observed live: every
rewrite verb is greyed out on a pushed commit.**

### Log changed-file row context menu — ORDER (`s3-ctxf-s.png`)
Show Diff (⌘D) · Show Diff in a New Tab · Compare with Local · Compare Before
with Local · Edit Source (⌘↓) · Open Repository Version | Revert Selected
Changes · Cherry-Pick Selected Changes · Extract Selected Changes to Separate
Commit… (disabled) · Drop Selected Changes (disabled) | Create Patch… ·
Get from Revision · History Up to Here · Show Changes to Parents.
This also visually confirms verbs 4/5 of §5.8 (`Cherry-Pick Selected Changes`,
`Get from Revision`) in place.

### Create New Branch dialog (`s3-nb-s.png`) — closes a §7.3 row
`Branch Name:` **pre-filled with the current branch name, pre-selected** ·
**`Checkout branch` — ON by default** · `Overwrite existing branch` — off (a
checkbox no document mentions) · buttons **Cancel | Create**, affirmative
primary right-aligned. (TurboGit's "Switch to the new branch now" default-yes
matches.)

### First-run wizard on a fresh config (`wizard-*.png`, §4.10/09-onboarding)
No license/EULA gate: the first launch opens **"Welcome to Rebased"** directly —
sidebar `Projects | Customize | Plugins`, three large entries **New Project /
Open / Clone Repository**, tagline "Create a new project to start from scratch.
Open existing project from disk or version control.", gear bottom-left.
The Customize tab: Theme **Islands Dark** (Sync with OS off), Editor colour
scheme Islands Dark, Language **English**, **Region: China Mainland
(locale-detected default)**, IDE font 13.0, red-green adjustment off, Keymap
macOS, plus `Import Settings…` / `All settings…` links. Notably the pre-project
menu bar carries only `Rebased, File, Help` — the full IDE menu is built per
project.

## D8 — attempts that failed this session

| Attempt | Result |
|---|---|
| System Events `click at` (again) | unreliable — replaced everywhere by the CGEvent bridge |
| Keyboard-navigating the Git menu to `Patch ▸` | menus close when the opening script exits; arrow-nav landed on the wrong item once (opened Create New Branch — cancelled) |
| `Create Patch from Local Changes…` (6 attempts: AX click, CG click, hover-then-AX, keyboard) | **the submenu opens and is readable, but scripted clicks on its items never fire the action** — patch-encoding default remains open, retry recipe: real CG click at the item's coordinates while the menu is open, hover included in the same shell call |
| Branches strip chevron in the Log | click does not expand the pane — branch-row context-menu order remains open |
| Region default in the wizard | observed `China Mainland` — this is **locale-derived**, not a fixed default; recorded as such |

## Net state of the capture list

Closed: `Use Safe Force Push` on · submodules on · Checkout-branch default on ·
Commit-tool-window topology · commit-row and changed-file-row context-menu
order · dialog button order (Create New Branch) · first-run wizard screens ·
the ⌃O popup's real contents.

Still open (each with its reason): patch-encoding default (submenu clicks
blocked — recipe above) · branch-row context-menu order (strip chevron
unresponsive to scripted clicks) · `On empty cherry-pick commit:` and
`Compact References View` (dialog/gear-level, need those surfaces opened) ·
the staging-area save-vs-delete docs contradiction (three sources disagree).
