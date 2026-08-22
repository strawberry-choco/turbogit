# TurboGit — Phased Execution Plan (Rust + egui)

> **Scope of this document.** `product-spec.md` defines *what* to build; `feature-checklist.md` tracks *which features* belong to each phase. This document defines *how* to build it with **Rust** (language) and **egui** (UI framework), and adds the **technology setup steps** required end‑to‑end. It is the bridge between the spec and a working binary.
>
> It assumes the phase boundaries from spec §9 / the checklist (Phase 0 → 4) and augments each with: setup, crates, file layout, deliverables, and verification. Read this together with the two sibling docs.
>
> **Decisions made here** (see §2 for rationale; adjust if you disagree):
> - Git engine: **CLI executor by default** (wraps the `git` binary) behind a `GitExecutor` trait, with an optional `gix` (gitoxide) reader for hot paths.
> - UI threading: egui is single‑threaded immediate mode → all git work runs on a **background `GitService`** (worker threads + channels); the UI only renders a cached `AppState`.

---

## 1. How to read this plan

```
product-spec.md   ──►  what features exist (source of truth)
feature-checklist ──►  which feature IDs land in each phase
execution-plan    ──►  HOW: stack, setup, architecture, per-phase steps, verify   ← this file
```

Each phase below lists its feature IDs (from the checklist) so you can tick both documents in parallel.

---

## 2. Technology decisions

### 2.1 Language & UI

| Concern | Choice | Why |
|---|---|---|
| Language | **Rust** (edition 2021+, MSRV 1.77+) | Memory safety for file/git operations; strong ecosystem; cross‑platform desktop binaries. |
| UI framework | **egui** + **eframe** | Immediate‑mode, pure Rust, no native widget toolkit, trivially cross‑platform (Windows/macOS/Linux). `eframe` provides the native window + event loop. |
| App shell | `eframe::run_native` | Windowing, DPI, clipboard, file drops, persistence hook. |

### 2.2 Git engine (resolves spec Open Question #1)

- **`GitExecutor` trait** in crate `turbogit-git`. The only module allowed to talk to git.
- **Default implementation: `CliExecutor`** — shells out to the `git` CLI via `std::process::Command`.
  - *Rationale:* zero native build dependencies (no C compiler / libgit2), 1:1 fidelity with IntelliJ (which itself shells out for many ops), and the fastest correct path to a working app.
- **Optional implementation: `GixExecutor`** using **`gix` (gitoxide)** for high‑frequency reads (status, log graph, blame) once the baseline app works.
  - Pure Rust, no system deps, fast — ideal for the per‑frame/hot paths where spawning a process is too slow.
- **Rejected for v1:** `git2` (libgit2 bindings) as the *primary* — it needs a C toolchain/cmake to build and does not cover interactive rebase / push‑with‑lease / sequencer well. Keep as a future alternative if you want in‑process mutating ops.

> Engine selection is a **setting** (`VcsSettings.engine = Cli | Gix`). The rest of the app is engine‑agnostic.

### 2.3 Supporting crates (pin major versions in `Cargo.toml`)

| Need | Crate(s) | Used in |
|---|---|---|
| Windowing/UI | `eframe`, `egui`, `egui_extras` (tables, images) | All UI |
| Native file dialogs | `rfd` | Clone / Open project / Export patch |
| Clipboard | `arboard` (or egui built‑in) | Copy hash, copy path |
| Diff algorithm | `similar` (or `diffy`) | Diff viewer, partial commit, blame |
| 3‑way merge | `git` CLI merge + custom resolution UI over `similar` | Conflict resolver (G3) |
| Async git work | `std::thread` + `crossbeam_channel` (or `flume`) | GitService |
| Parallel per‑root ops | `rayon` | Multi‑root batch ops |
| File watching (status refresh) | `notify` | Auto status update on save |
| Serialization / settings | `serde`, `serde_json` (or `toml`/`ron`) | Settings, project layout, shelf |
| Config/home paths | `directories` | Per‑user settings location |
| Temp repos for tests | `tempfile` | Integration tests |
| Logging | `tracing` + `tracing-subscriber` | Diagnostics |
| Errors | `thiserror` / `anyhow` | Error handling |
| Icons / fonts | `egui` bundled + `include_bytes!` for custom | Toolbar |

### 2.4 Paths & persistence (resolves spec §3 / Open Question #3)

- **Project‑level state** (directory mappings, active changelist layout, multi‑root registry): a `.turbogit/` directory at the project root (our `.idea/vcs.xml` equivalent). Stored as TOML/JSON via `serde`.
- **User‑level settings** (git path, update method, protected branches, shortcuts): `directories::ProjectDirs` → config dir, e.g. `~/.config/turbogit/settings.toml`.
- **Shelf store:** `.turbogit/shelf/` patch files (git‑format patches).
- **Honor `.gitignore`** for free (git status already respects it); add IDE‑level ignore patterns in settings, filtered in the UI layer only.

---

## 3. Environment & tooling setup

> Run once on each developer machine and in CI. Windows is the primary dev OS per the project context; commands are shell‑compatible (Git Bash / PowerShell).

### Step 3.1 — Install Rust toolchain
```bash
# Via rustup (preferred)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# or on Windows (PowerShell):  https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe

rustup toolchain install stable
rustup default stable
rustc --version && cargo --version
```

### Step 3.2 — Install system prerequisites
| OS | Requirement | Command |
|---|---|---|
| **Windows** | Git for Windows (CLI), C++ build tools *only if* you later add `git2` | `winget install Git.Git` |
| **macOS** | Git (Xcode CLT), `cmake` *only if* adding `git2` | `xcode-select --install; brew install cmake` |
| **Linux** | `git`, `pkg-config`, `libssl-dev` *only if* adding `git2` | `apt-get install -y git pkg-config libssl-dev` |

> Because the default engine is the **CLI**, you do **not** need a C toolchain for v1. `git` itself is the only hard requirement (resolves spec A9 / WSL detection).

### Step 3.3 — Verify git CLI + WSL (Windows)
```bash
git --version
# Windows + WSL2: ensure `git` is reachable; detect later in Settings → VCS → Git (spec A9)
where git        # Windows
```
The app's `GitExecutor` will resolve the git binary path from settings, with auto‑detect for `\wsl$` paths (spec A9).

### Step 3.4 — Editor / IDE for Rust
- **VS Code** + `rust-analyzer` extension (recommended), **or**
- **IntelliJ Rust** plugin (fitting, given the product inspiration), **or**
- `neovim` + rust-analyzer.
- Enable `clippy` and `rustfmt` in the editor.

### Step 3.5 — Useful cargo tools (optional but recommended)
```bash
cargo install cargo-watch      # recompile on change during dev
cargo install cargo-nextest    # faster test runner
cargo install cargo-audit      # dependency security audit
```

---

## 4. Project scaffolding

### Step 4.1 — Initialize a Cargo workspace
We use a **workspace** so the engine‑agnostic core, the git engine, and the UI are independently testable crates.

```
turbogit/                      (workspace root)
  Cargo.toml                   (workspace manifest)
  rust-toolchain.toml          (pin stable)
  .gitignore                   (target/, etc.)
  crates/
    core/                      (turbogit-core: domain model + services, NO git, NO egui)
    git/                       (turbogit-git: GitExecutor trait + Cli/Gix impls)
    ui/                        (turbogit-ui: eframe/egui frontend + widgets)
    app/                       (turbogit-app: binary entrypoint, wires core+git+ui)
  tests/                       (workspace integration tests using temp repos)
  docs/
```

`Cargo.toml` (workspace):
```toml
[workspace]
resolver = "2"
members = ["crates/core", "crates/git", "crates/ui", "crates/app"]

[workspace.dependencies]
egui = "0.29"
eframe = "0.29"
egui_extras = "0.29"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
anyhow = "1"
crossbeam-channel = "0.5"
rayon = "1"
notify = "6"
rfd = "0.14"
similar = "2"
directories = "5"
tempfile = "3"
tracing = "0.1"
tracing-subscriber = "0.3"
gix = { version = "0.63", optional = true }
```

> Enable `gix` only when you start Phase 1/3 hot‑path optimization: `cargo build --features turbogit-git/gix`.

### Step 4.2 — `crates/core` (domain layer, engine‑agnostic)

> **Amended by [ADR-0001](adr/0001-executor-is-the-seam.md):** the engine seam is the `GitExecutor` interface itself — there is no `VcsManager` façade. Core services take `&dyn GitExecutor`; root discovery and `Root` snapshots live in the multi-root module (the Root scanner).

Holds the data model from spec §8: `Project`, `Root`, `Remote`, `Branch`, `Commit`, `Changelist`, `Change`, `Chunk`, `Shelf`, `Stash`, `Worktree`, `Conflict`. Plus service modules that the UI calls: `changes`, `branch_service`, `history_service`, `integrate_service`, `conflict`, `shelve_stash`, `sync_service`, and the multi-root module (`MultiRootManager` model + Root scanner). All take `&dyn GitExecutor` (ADR-0001). **No `std::process`, no `egui` imports allowed here** — this is what makes it unit‑testable.

### Step 4.3 — `crates/git` (engine layer)
- `GitExecutor` trait: `status(root)`, `log(root, range)`, `commit(...)`, `fetch/pull/push`, `branch_*`, `merge/rebase/cherry_pick`, `stash/shelve`, `diff(...)`, `blame(...)`.
- `CliExecutor` (default): builds `git` argv, parses porcelain output into core types.
- `GixExecutor` (optional): uses `gix` for reads.
- Parsing helpers for `git status --porcelain`, `git log --format=...`, `git diff --numstat`, etc.

### Step 4.4 — `crates/ui` (egui / eframe)
- `App` struct implementing `eframe::App`; owns `AppState`.
- `AppState { project, roots_status, ui_selection, settings, logs, toasts }` — the cached, render‑safe snapshot.
- Widgets: `CommitWindow`, `GitLogWindow`, `BranchWidget`, `DiffViewer`, `MergeEditor`, `PushDialog`, `BranchesPopup`, `VcsPopup`, `SettingsWindow`.
- `GitService` client: sends `GitCommand` to the background service, receives `GitEvent`, calls `ctx.request_repaint()`.

### Step 4.5 — `crates/app` (binary)
`fn main()` → load settings → open/restore project → `eframe::run_native(Box::new(App::new(...)))`.

---

## 5. Architecture: state & threading for egui

egui re‑renders every frame, so **never call git synchronously inside a widget**. The pattern:

```
┌─────────────── UI thread (egui) ───────────────┐        ┌──── Background GitService ────┐
│ AppState (cached snapshot)                      │        │ worker threads / rayon        │
│   render widgets from AppState                  │        │   receives GitCommand         │
│   on user action → send GitCommand ────────────┼─chan──►│   runs executor (CLI/gix)     │
│   on GitEvent   → update AppState              │◄─chan──│   sends GitEvent back         │
│   ctx.request_repaint()                         │        └─────────────────────────────┘
└─────────────────────────────────────────────────┘
```

- **`GitService`**: owns a `crossbeam_channel::Sender<GitCommand>` and a `Receiver<GitEvent>`. Spawns 1+ worker threads (or a `rayon` pool for multi‑root parallelism). On each `GitEvent`, the UI merges it into `AppState` and requests a repaint.
- **Per‑root parallelism (Phase 3):** batch Commit/Update/Push iterate `Root[]` and dispatch to the `rayon` pool; results stream back as they complete.
- **File watching:** `notify` watches the project tree; on change → enqueue a `Status` refresh.
- This satisfies the spec's layering rules (§4): Engine talks to git, Core is engine‑agnostic + testable, UI never calls git directly.

---

## 6. Phased execution plan

Each phase: **Goal · Setup/steps · Key files · Crates · Deliverable · Verification**. Feature IDs reference `feature-checklist.md`.

### PHASE 0 — Engine & Core  (spec §9, checklist "Phase 0")

**Goal.** A project loads and shows *real* Git status for one root.

**Steps**
1. Scaffold workspace (§4.1–4.5). `cargo build` is green.
2. `crates/core`: define `Project`, `Root`, `Remote`, `Branch`, `VcsSettings` (+ protected‑branch patterns). No git yet.
3. `crates/git`: implement `GitExecutor` trait + `CliExecutor`:
   - `git version`, locate binary (A9), WSL detection placeholder.
   - `git init` (A1), `git clone [--depth N]` (A2/A8), `git remote add/edit/remove` (A6).
   - `git status --porcelain` → `RootStatus` (modified/added/deleted/unversioned/ignored/conflicted).
4. Directory mappings (A3/A4): store `.turbogit/mappings.toml`; load on startup.
5. Auto‑detect unregistered roots (A5): scan for `.git` dirs → notify (toast).
6. Settings UI stub: `Settings → VCS → Git` path + Test button (A9).

**Key files:** `core/src/model.rs`, `core/src/vcs_manager.rs`; `git/src/cli.rs`, `git/src/parse.rs`; `ui/src/app.rs`; `app/src/main.rs`.
**Crates:** egui, eframe, serde, directories, crossbeam-channel, rfd.
**Deliverable:** Open a folder → see real modified/unversioned files for one repo; `git init`/`clone`/`remote` work from the UI.
**Verification:**
- `cargo test` (core model unit tests).
- Manual: point at a real repo → status matches `git status`. Clone a URL → tree appears. `git init` a fresh dir → mapping saved.

### PHASE 1 — Single‑root daily flow  (checklist "Phase 1")

**Goal.** A solo dev can do a full day's work on one repo.

**Steps**
1. **Commit tool window** (C1–C5): changelists model + active concept; unversioned bucket (C2); multiple changelists + move between (C4/C5). `egui_extras::Table` for the change list.
2. **Staging‑area mode** (C6): toggle Unstaged/Staged; preserve changelists on switch.
3. **Diff viewer** (K1/K2/K4/K5): `similar` line/word diff; compare file versions, folders, with branch/working tree; whitespace + magic‑wand options.
4. **Commit** (C9–C14): message field w/ history + amend + template (C9); advanced options stub (C10); pre‑commit hooks toggle (C11); partial commit by chunk/line (C12); amend (C13); Commit & Push (C14).
5. **Branch widget + popup** (E1–E13 except multi‑root bits): current branch + incoming/outgoing arrows (E1/D8); popup Recent/Local/Remote/Tags + dir grouping (E2); new/checkout/rename/delete/compare/favorite (E3–E10); smart checkout (E5); workspace‑context restore stub (E13).
6. **Sync** (D1–D10): fetch (D1), pull/update w/ merge|rebase (D2/D3), push dialog (D4), force‑with‑lease + protected‑branch block (D5), push tags (D6), rejected handling (D7), incoming check mode (D9), clean‑tree Stash/Shelve choice (D10).
7. **Git Log** (H1–H4): graph + Branches/Commits/ChangedFiles/Details panes; filtering & search (H2); show repo at revision (H3); compare two commits (H4).
8. **File history + blame** (H5–H8): Show History, selection history (H6), directory history (H7), Annotate/Blame (H8).
9. **VCS Operations Popup** (M1): `Alt+`` quick actions.
10. Add `GixExecutor` for hot reads (status/log) behind the engine setting (optional, §2.2).

**Key files:** `ui/src/windows/commit.rs`, `log.rs`, `branch.rs`; `ui/src/widgets/diff.rs`, `blame.rs`; `git/src/porcelain.rs`.
**Crates:** egui_extras, similar, notify (status refresh), arboard (copy hash/path).
**Deliverable:** Full single‑repo daily loop: edit → see gutter/diff → stage/commit → branch → sync → investigate history/blame, all in the UI.
**Verification:**
- `cargo nextest run` green incl. `git` integration tests (temp repos).
- Manual end‑to‑end on a scratch repo: commit partial hunks, create/checkout/delete branch, fetch/pull/push to a test remote, resolve a trivial pull, view log + blame.

### PHASE 2 — Integrate & resolve  (checklist "Phase 2")

**Goal.** Team flows + conflict resolution on one root.

**Steps**
1. **Merge** (F1/F2): options (`--no-ff/--ff-only/--squash/-m/--no-commit/--no-verify/--allow-unrelated-histories`); smart merge (stash→merge→unstash) (F2); abort/continue (F8).
2. **Rebase** (F3/F4): options (`--onto/--rebase-merges/--keep-empty/--root/--update-refs/--autosquash`); pull‑into‑rebase (F4).
3. **Conflict resolver** (G1–G9): auto‑merge non‑conflicting (G1); conflicts dialog Accept Yours/Theirs/Merge + Resolve All Simple (G2); **3‑way merge editor** Local|Result|Server (G3); apply non‑conflicting (G4); resolve simple (G5); per‑line accept/ignore (G6); revert resolution (G7); LF/CRLF warning (G8); Merge Conflicts node in Commit window (G9).
4. **Stash** (J5–J8): stash per root w/ Keep index (J5); apply/pop (J6); stash as new branch (J7); drop/clear (J8).
5. **Revert / undo** (I1/I2): revert commit (I1); undo last commit (I2).

**Key files:** `ui/src/widgets/merge_editor.rs`, `conflicts.rs`; `git/src/merge.rs`, `rebase.rs`, `stash.rs`.
**Crates:** similar (3‑way diff), crossbeam (long ops off UI thread).
**Deliverable:** Merge/rebase with a real 3‑way merge editor; stash/unstash; revert/undo — on a single root.
**Verification:**
- Unit tests for merge‑editor conflict parsing + resolution output.
- Manual: intentionally create a conflict → resolve via 3‑way editor → commit; stash with dirty tree → pop; revert a commit.

### PHASE 3 — Multi‑root ⭐  (checklist "Phase 3")

**Goal.** Manage a parent + several repos as one workspace.

**Steps**
1. **Register / auto‑detect roots** (B1/B2): multiple mappings; per‑root status; changes grouped per repository.
2. **Unified Log** (B3/B11): merged, sorted commit graph across roots; colored root stripe + Show Root Names; Root filter section.
3. **Batch ops** (B4/B8): Commit / Update Project / Push across all roots; Push dialog lists every repo + outgoing commits.
4. **Synchronous branch control** (B5): `Execute on all roots` setting; popup shows only common branches; create/checkout/merge/delete on every root; propose‑on‑first‑use (resolves spec OQ #5).
5. **Per‑repo branch control** (B6): current‑root resolution (selected file → its root); other roots as sub‑popups.
6. **Divergence warning + rollback** (B7): partial‑failure detection → offer rollback of successful roots.
7. **Submodule + worktree roots** (B9/B10): register nested/submodule/worktree dirs as roots; "New Worktree from".
8. **Shelve** (J1–J4, J9–J11): shelf store in `.turbogit/shelf/`; shelve/unshelve/silently/save‑to‑shelf; delete + restore; import/export patches; combined stash+shelf tab.

**Key files:** `core/src/multi_root.rs`; `ui/src/windows/multi_log.rs`, `push_multi.rs`; `git/src/worktree.rs`, `submodule.rs`.
**Crates:** rayon (parallel per‑root dispatch), rfd (patch import/export).
**Deliverable:** Open a multi‑repo project → unified log, batch commit/update/push, sync + per‑repo branch control with divergence rollback, shelves.
**Verification:**
- Integration test: 3 temp repos, synchronous checkout succeeds; forced partial failure → rollback restores all to original branch.
- Manual: scaffold parent + 2 nested repos → batch push; toggle sync mode; create worktree.

### PHASE 4 — History editing & polish  (checklist "Phase 4")

**Goal.** Feature parity with the IntelliJ catalog.

**Steps**
1. **Interactive rebase** (F5): pick/stop‑to‑edit/reword/squash/fixup/drop/reorder → Commit and Rebase.
2. **Cherry‑pick** (F6/F7): commit + partial selected changes.
3. **History editing** (I3–I8): reword (I3), amend earlier (Fixup/Squash Into + Commit and Rebase) (I4), squash (I5), drop (I6), extract (I7), split (I8).
4. **Log indexing** (H9): index for fast filter / Search‑Everywhere history (consider `gix` or a lightweight SQLite/in‑memory index).
5. **Open on GitHub/GitLab** (H10).
6. **Local History** (N1/N2): IDE‑level snapshots on save/run/refactor; restore/compare (independent of git).
7. **Tags** (O1–O4): create/checkout/push/show in branches pane.
8. **Ignore UX** (P1–P3): add to `.gitignore`, ignore/unignore, settings patterns.
9. **Full settings UI** (Q): every row from spec §Q table.
10. **Hosting plugins** (L1–L6): GitHub/GitLab accounts (token/OAuth), clone, share, PR/MR review + create, open‑on‑web, gists. (Resolves spec OQ #6: read GitHub branch protection for protected‑branch enforcement.)

**Key files:** `ui/src/windows/interactive_rebase.rs`, `settings.rs`, `hosting/`; `git/src/rebase_todo.rs`, `tag.rs`.
**Crates:** reqwest (hosting API), oauth2 (optional), serde, sqlite (index, optional).
**Deliverable:** Complete IntelliJ‑parity feature set including hosting PR flows.
**Verification:**
- `cargo test` for history‑editing edge cases (squash/drop ordering, protected‑branch block).
- Manual: interactive rebase reorder+squash; cherry‑pick partial; create PR via GitHub plugin.

---

## 7. Cross‑cutting concerns

### 7.1 Testing strategy
- **Unit:** `core` domain logic (no git). `cargo test`.
- **Integration:** `git` crate against **temp repos** (`tempfile`) — real `git init/commit/branch/merge`. Run in CI with a known git version.
- **UI:** snapshot/behavior tests where feasible (egui has `egui::test` helpers); primarily manual + a smoke test that the app boots and opens a repo.
- Use **`cargo nextest`** for speed/parallelism.

### 7.2 CI (GitHub Actions example)
```yaml
jobs:
  build-test:
    runs-on: ${{ matrix.os }}
    strategy: { matrix: { os: [ubuntu-latest, windows-latest, macos-latest] } }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: "clippy, rustfmt" }
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo nextest run
```
> Pin the git version in CI (e.g. `actions/setup-git`) so porcelain parsing stays stable.

### 7.3 Error handling & safety
- Use `thiserror` for typed git errors; surface failures as non‑blocking toasts, never panic the UI thread.
- Destructive ops (force push, drop commit, delete branch) → confirmation dialogs + protected‑branch guard.
- All mutating git calls go through the background `GitService` so the UI never blocks.

### 7.4 Performance
- Status/log reads via `GixExecutor` once baseline works (Phase 1/3).
- Debounce `notify` events before enqueuing status refresh.
- Virtualize large lists/tables (`egui_extras::Table` scrolls lazily).

---

## 8. Risks & resolved open questions

| Spec OQ | Resolution in this plan |
|---|---|
| #1 Engine | CLI executor default + optional `gix` reader (§2.2). |
| #2 UI framework | **egui** (confirmed by you). |
| #3 Ignore strategy | Honor `.gitignore` (free); add IDE‑level patterns in settings, UI‑only filter. |
| #4 WSL/SSH | Detect `git` in WSL on Windows; rely on system `ssh-agent` for SSH creds (never store). Shallow v1. |
| #5 Sync default | Propose‑on‑first‑use in multi‑root (Phase 3, B5). |
| #6 Protected branch | Local patterns now; GitHub branch‑protection read via hosting plugin (Phase 4, L). |

**Key risks**
- egui immediate mode is awkward for very large log graphs → virtualize + cap rendered nodes.
- CLI parsing brittleness across git versions → pin in CI, prefer stable porcelain flags.
- Multi‑root rollback correctness → covered by the Phase 3 integration test.

---

## 9. Milestone summary & ordering

```
Setup ──► Phase 0 (engine+core) ──► Phase 1 (single-root daily)
                                      │
                                      ▼
                            Phase 2 (merge/rebase/conflict/stash)
                                      │
                                      ▼
                            Phase 3 (MULTI-ROOT ⭐)
                                      │
                                      ▼
                            Phase 4 (history edit + hosting + polish)
```

Each phase ends at a **runnable, independently usable** binary (per spec §9 exit criteria). Tick features in `feature-checklist.md` as you complete them; keep this plan and the spec in sync if scope changes.

---

## 10. Definition of done (end‑to‑finish)

TurboGit is "done" (v1) when:
1. A multi‑root project opens, shows real status, and supports the full daily loop (commit/branch/sync/diff/history/blame) on each root.
2. Merge, rebase, interactive rebase, cherry‑pick, and a true 3‑way conflict editor work.
3. Multi‑root batch operations + synchronous/per‑repo branch control + divergence rollback work.
4. History editing (squash/fixup/drop/extract/split/reword) and hosting PR flows are available.
5. `cargo test` + `clippy` + `fmt` are green on Windows/macOS/Linux; the app builds to a native binary via `cargo build --release`.
