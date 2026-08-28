# Landscape Map & Positioning — Diff Tools

> Cohort map for every in-scope tool from `docs/diff-capabilities-research-plan.md §2`.
> Versions/prices pinned to **2026-08-02** via vendor pages + 2026 roundups
> (gitsquid, scmgalaxy, bestpage, cnblogs). Hands-on not performed in-sandbox; see
> `storm-session.md` environment note.

## Cohort 1 — IDEs
| Tool | License | Platforms | Price (2026) | Target user | Git-engine model |
| --- | --- | --- | --- | --- | --- |
| VS Code | MIT (OSS core) | Win/Mac/Linux | Free | General devs | git CLI + built-in SCM |
| JetBrains (IntelliJ/PyCharm/WebStorm/Rider) | Proprietary + Apache parts | Win/Mac/Linux | $249/yr (1st), $159 renew | Pro devs/teams | Built-in (JGit/CLI hybrid) |
| Visual Studio | Proprietary | Win/macOS | Free Community / paid | .NET/enterprise | Built-in (libgit2-ish) |
| Xcode | Proprietary (free) | macOS | Free | Apple platform devs | Built-in (CLI + lib) |
| Eclipse | EPL (OSS) | Win/Mac/Linux | Free | Enterprise/legacy | EGit (JGit) |
| Zed | GPL/OSS + paid | Win/Mac/Linux | Free / $18/mo (Pro) | Modern-collab devs | Built-in (CLI) |
| Fleet | Proprietary (preview) | Win/Mac/Linux | Free preview | JetBrains-next | Built-in |

**Positioning.** IDEs win on *context* (diff in-place next to code, inline blame, semantic
highlight). They are not dedicated diff tools but set the bar for syntax/word highlighting
and now AI-assisted conflict resolution (VS Code 1.105, Sept 2025).

## Cohort 2 — Modal / Terminal Editors
| Tool | License | Platforms | Price | Target user | Git-engine model |
| --- | --- | --- | --- | --- | --- |
| Neovim (+ diffview.nvim, neogit) | Apache-2.0 | Win/Mac/Linux | Free | Power users | git CLI |
| Emacs Magit | GPL-3.0 | Win/Mac/Linux | Free | Power users | git CLI (magit wraps) |
| Helix | MPL-2.0 | Win/Mac/Linux | Free | Modal enthusiasts | git CLI |
| Sublime Text | Proprietary | Win/Mac/Linux | $139 (personal) | General devs | CLI integration |

**Positioning.** The keyboard-first, flow-obsessed cohort. Magit is the reference
implementation of *discoverable* partial staging; Neovim+diffview is the fastest
terminal side-by-side. Accessibility is weak (terminal).

## Cohort 3 — Standalone Git GUIs
| Tool | License | Platforms | Price (2026) | Target user | Git-engine model |
| --- | --- | --- | --- | --- | --- |
| GitKraken | Proprietary | Win/Mac/Linux | Free / $5+/mo | Teams/visual | Built-in (git) |
| Sourcetree | Proprietary (free) | Win/macOS | Free | Beginners/Bitbucket | Built-in |
| Fork | Proprietary | Win/macOS | $59 one-time | Devs (feel/speed) | Built-in (custom) |
| Tower | Proprietary | Win/macOS | $69/yr | Pros/teams | Built-in |
| SmartGit | Proprietary (free non-commercial) | Win/Mac/Linux | $99/yr commercial, free NC | Enterprise/Linux | Built-in |
| Sublime Merge | Proprietary | Win/Mac/Linux | $99 | Speed/power users | Built-in (custom engine) |
| GitHub Desktop | MIT (OSS) | Win/macOS | Free | Beginners/GitHub | git CLI |
| GitUp | MIT (OSS) | macOS | Free | Mac visual | Built-in (libgit2) |
| lazygit | MIT (OSS) | Win/Mac/Linux | Free | Terminal devs | git CLI |
| tig | GPL (OSS) | Win/Mac/Linux | Free | Terminal devs | git CLI |

**Positioning.** The commercial heart of the market. Fork/Sublime Merge/Tower win on
native feel + speed + conflict wizards; GitHub Desktop on simplicity; lazygit/tig on
terminal flow. Linux is absent from Fork/Tower/GitHub Desktop (Windows/macOS only) — a
gap SmartGit/Sublime Merge/GitKraken fill.

## Cohort 4 — Diff Specialists
| Tool | License | Platforms | Price | Target user | Git-engine model |
| --- | --- | --- | --- | --- | --- |
| difftastic | MIT (OSS) | Win/Mac/Linux | Free | Semantic-diff fans | Standalone (+ git difftool) |
| git-delta | MIT (OSS) | Win/Mac/Linux | Free | Pager/diff fans | Standalone (pager/difftool) |
| icdiff | MIT (OSS) | Win/Mac/Linux | Free | Terminal side-by-side | Standalone |
| Kaleidoscope | Proprietary | macOS | $79 one-time (ksdiff) | Mac designers/devs | difftool/mergetool |
| Beyond Compare | Proprietary | Win/Mac/Linux | $60 Std / $70 Pro | Compare/merge pros | difftool/mergetool |
| Meld | GPL (OSS) | Win/Mac/Linux | Free | OSS devs | difftool/mergetool |
| P4Merge | Freeware (Proprietary) | Win/Mac/Linux | Free | Perforce/git merge | mergetool |
| WinMerge | GPL (OSS) | Windows | Free | Windows devs | difftool/mergetool |

**Positioning.** Specialists are *difftool/mergetool* targets, not full Git clients. They
own the high-fidelity comparison (Beyond Compare hex/visual, Meld 3-way, difftastic
structural) and the 3-way merge editor category. difftastic (v0.67, Nov 2025) and
git-delta are the open semantic/word-level reference points.

---

## Cross-Cutting Observations
- **Linux parity gap.** Fork, Tower, GitHub Desktop, Kaleidoscope, GitUp are Windows/macOS-
  only. TurboGit (Rust/egui) gets 3-OS parity for free — a positioning advantage.
- **Structural diff is specialist-only.** No mainstream GUI ships AST diffing as default;
  difftastic/git-delta are CLI add-ons. Big whitespace for TurboGit.
- **AI conflict resolution is the new frontier.** VS Code 1.105 (Sept 2025) added
  agentic merge-conflict resolution; incumbents have not broadly followed yet.
- **Safety/polish is the paid wedge.** Undo (Tower), conflict wizards (GitKraken/Tower),
  AI assist — not the diff view itself — justify paid tiers.
