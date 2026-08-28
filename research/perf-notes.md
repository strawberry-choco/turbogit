# Performance Probing — Notes & Benchmark Table

> **Caveat up front.** The plan's §6 scripted tasks (5k-line render, 10k-commit history)
> require *hands-on* execution on each tool. That is not possible in this sandbox (no GUI
> install/launch, no sample repos with those scales pre-staged). The table below records
> **documented/vendor-claimed/review-reported** behavior only, with confidence marked.
> Items marked ⚠ are estimates from reviews, not measured here, and need a hands-on pass.

## Benchmark Table (documented behavior, 2026-08-02)

| Tool | Engine | 5k-line diff render | Large-repo scroll | Memory profile | Confidence |
| --- | --- | --- | --- | --- | --- |
| Sublime Merge | Custom C++ | Fastest cited; purpose-built parser | Smooth | Low–moderate | High (reviews consistent) |
| Fork | Custom | Fast, native | Smooth | Low | High |
| Tower | Custom | Fast | Smooth | Low–moderate | High |
| SmartGit | Java/Swing | Good; Swing overhead noted | Mostly smooth | Moderate–high (JVM) | Med |
| GitKraken | Electron | "Slower on very large repos" per reviews | Can stutter | High (Electron+node) | High (repeated) |
| VS Code | Electron | Good via virtualized diff editor | Smooth | High (Electron) | High |
| JetBrains | JVM | Good; indexed | Smooth after index | High (JVM) | High |
| GitHub Desktop | Electron | Adequate; no side-by-side | Adequate | High | Med |
| lazygit / gitui | Rust TUI | Fast (terminal) | Fast | Very low | High |
| tig | C TUI | Fast | Fast | Very low | High |
| Neovim+diffview | C/Lua | Fast (vimdiff) | Fast | Low | High |
| Magit | Emacs Lisp | Good; can slow on huge buffers | Good | Moderate (Emacs) | Med |
| difftastic | Rust | ⚠ Poor scaling on many-change files; high RAM | ⚠ Memory cliff | ⚠ High on big diffs | High (self-documented) |
| git-delta | Rust (pager) | Fast for typical; word-level cheap | Fast | Low | High |
| Beyond Compare | Native | Fast; can load huge files | Smooth | Low–moderate | High |
| Meld | Python/GTK | ⚠ Slower on very large; can hang | ⚠ Variable | Moderate | Med |
| P4Merge | Native | Good | Good | Low | Med |
| WinMerge | Native (Win) | Good | Good | Low | Med |

## Key Findings
1. **Native > Electron > JVM** for steady-state memory and large-diff smoothness. Sublime
   Merge/Fork/Tower (native) and lazygit/gitui (Rust TUI) lead; GitKraken/VS Code/GitHub
   Desktop (Electron) are repeatedly cited as heavy on large repos.
2. **difftastic has a real memory cliff** on files with many changes (its own "Known
   Issues"); this is the central risk for making structural diff the *default* — it must be
   opt-in or bounded for 5k-line refactors (see `recommendations.md` R3).
3. **Virtualization is the unlock.** VS Code's diff editor and the native GUIs all virtualize;
   TurboGit's existing `egui_extras::Table` virtualization (project MEMORY) is the right
   primitive for the diff view too.
4. **Rust TUI proves the floor.** lazygit/gitui show a Rust git UI can be the fastest diff
   surface at trivial memory — validates TurboGit's Rust/egui choice for flow, separate from
   rendering fidelity.

## Scripted-Task Status (plan §6)
| Task | Status |
| --- | --- |
| T1 5k-line render + scroll | ⚠ Documented only; hands-on blocked in sandbox |
| T2 stage hunk + unstage line | Covered by UX-patterns (flow-level, not measured) |
| T3 3-way conflict resolve | Covered by UX-patterns (flow-level) |
| T4 image + rename diff | Covered by feature-matrix (capability-level) |
| T5 search + jump | Covered by UX-patterns |
| T6 ignore-whitespace toggle | Covered by feature-matrix |

**Follow-up:** a hands-on pass (real 5k-line/10k-commit samples, per-tool timing) is the
single biggest evidence gap and should be scheduled if GUI access becomes available.
