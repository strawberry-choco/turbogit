# R3 session 2 — menu ordering (D6) and settings defaults (D7), captured 2026-10-06

After the Accessibility + Screen Recording grants were given to the host app,
the GUI layer became capturable. Method: System Events / JXA for AX reads and
menu clicks, a JXA `CGEvent` bridge for mouse clicks and scroll-wheel events
(System Events `click at` proved unreliable), screenshots via `screencapture -R`.
Rebased was the instance left running from session 1 (clean config at
`/tmp/rebased-r3`, fixture `/tmp/tg-r3-fixture` open). All screenshots are in
`screenshots/`; the full menu-bar dump is `screenshots/menubar.txt`.

## D6 — menu item ORDER (main menu bar: closed)

`menubar.txt` (459 lines) is the full ordered tree, top level:
`Apple, Rebased, File, Edit, View, Navigate, Code, Run, Tools, Git, Window, Help`.

**Git menu, verbatim order** (separators as `|`):

> Commit… · Push… · Update Project... · Pull… · Fetch |
> Merge… · Rebase… |
> Branches… · New Branch… · New Tag… · Reset HEAD… |
> New Worktree… · Worktrees… |
> Show Git Log · Patch ▸ · Uncommitted Changes ▸ · Selected Directory ▸ |
> GitLab ▸ · GitHub ▸ |
> Manage Remotes… · Clone… |
> VCS Operations Popup…

`Uncommitted Changes ▸` = Shelve Changes… · Show Shelf · Stash Changes… ·
Unstash Changes… · Rollback…  `GitLab ▸` / `GitHub ▸` = Share Project on … ·
Clone Repository… | Manage Accounts…

**Still open under D6** (recorded, not captured): row *context-menu* order
(branch/commit/file rows — AX cannot open them without a hover dance),
`Patch ▸` and `Selected Directory ▸` (lazy-loading submenus that render empty
to AX), dialog button order, and the gear-menu composition. Main-menu ordering —
the load-bearing half of §7.2's table — is closed.

## D7 — settings defaults (mostly closed)

Walked every `Version Control | *` page (11 of 11) plus Advanced Settings
(partial). Evidence: `screenshots/final-*.png`, `git-*.png`,
`confirm-gutter2.png`, `page-commit-a.png`, `kb-test.png` (Changelists),
`nav-test.png` (Git top). Defaults below are from a **clean config** — first
run, nothing touched (dialog was cancelled, never OK'd).

### Version Control | Git

| Control | Default |
|---|---|
| Path to Git executable | Auto-detected: /usr/bin/git (+ `Test` button) |
| Set this path only for the current project | **off** |
| Automatically exclude ignored directories from the analysis | **off** |
| `Enable staging area` | **off** — "This will disable changelists support" (changelists are the default mode; §7.3 closed) |
| Warn if CRLF line separators are about to be committed | **on** |
| Warn when committing in detached HEAD or during rebase | **on** |
| Warn when committing files larger than **50 MB** | **on** |
| Warn when committing files with names that might cause issues on other systems | **on** |
| Add the 'cherry-picked from \<hash\>' suffix when picking commits pushed to protected branches | **on** — *refines the docs' "no suffix by default": the suffix is a protected-branch safety default* |
| Configure GPG Key… | "Commits signing with GPG key is not configured" |
| Auto-update if push of the current branch was rejected | **off** |
| Show Push dialog for Commit and Push | **on** |
| Show Push dialog only when committing to protected branches | off (greyed) |
| **Protected branches** | **`master;main`** — semicolon-separated |
| Load branch protection rules from GitHub | **on** — "rules are added to the local rules and synced on every fetch" |
| Update method | ⦿ **Merge** (◦ Rebase) |
| **Clean working tree using** | ⦿ **Shelve** (◦ Stash) — **the §7.1b withdrawal is itself corrected: the original Shelve-first inference was right; the settings page merely lists Stash first** |
| Filter 'Update Project' information by paths | All |
| Fetch remote changes automatically | **on** |
| Check remote for incoming changes | off (secondary) |
| Fetch tags | **Auto (Follow git config)** — §7.3's enum question closed |
| Use credential helper | **off** |
| Combine stashes and shelves in one tab | **off** (the §4.5 merge-tabs setting exists, default separate) |
| When "Show Diff" is called, compare stashed changes | ⦿ **With the local version of a file** (◦ With the parent commit) |

### Version Control | Commit
`Clear initial commit message` **on**. Commit message inspections: `Blank line
between subject and body` off · `Limit body line` off · `Limit subject line` off
— Severity dropdown (Warning) present but disabled while unchecked. The six
⌃O dialog options are *not* here (dialog-level, still R3).

### Version Control | Confirmation
`When files are created:` **Ask** (+ greyed "Apply to files created outside
Rebased") · `When files are deleted:` **Ask** · `Show options before:` Update
**on** · **`Ask for confirmation to drop commits` on** ("Applicable to Git" —
closes the §7.5 rebase-drop question and 06's runtime-capture item 2) ·
`Check for conflicts with the server every 60 minutes` off ·
`Highlight files changed in the last 31 days` off ·
`Highlight directories that contain modified files in the Project tree` off ·
**`When patch is created:` Ask** (§7.3 patches row closed) ·
`Restore workspace when switching branches` **on** · `Limit history to 1000
rows` on · Gutter: all three modified-line highlights **on**.

### Version Control | Changelists
`Create changelists automatically` off · `Allow putting changes within one file
into different changelists` **on** · inactive-changelist highlights off ·
`When an empty changelist becomes inactive:` Show options ·
`Highlight files with changelist conflicts` mixed-state.

### Version Control | Shelf
`Remove successfully applied files from the shelf` **off** (unshelve keeps by
default — verified) · `Shelve base revisions of files under distributed version
control systems` **on**, "base content of files larger than 500K will not be
stored" · `Change Shelves Location…` — **default location `.idea/shelf`**.

### Version Control | Directory Mappings
`<Project> Detected 1 → Git`; toolbar `+ − ✎`; footnote defining `<Project>`
scope; `Enable automatic mapping detection` **on**. The edit dialog
(`vc-fsc.png` accidentally captured it) has `Version control:` Git dropdown +
"More via plugins…", Directory/`Project` radio with **Project default**,
`Save Mapping`.

### Version Control | File Status Colors
Full colour vocabulary captured (green Added, blue Modified/Merged, red
Changelist conflict/Merged with conflicts/Unknown, orange Hijacked, olive
Ignored, grey Deleted/Obsolete/Suppressed/Up to date, plus inactive-changelist
variants) — the reference's "3-colour grammar" is richer than 3 colours;
`* Customized` footnote.

### Version Control | Issue Navigation
Empty state: "No patterns configured", Issue | Link table.

### Version Control | Log
`Show the log in the editor window` **on** — labelled **"Rebased setting"** (a
product-specific tweak, badge in the UI) · `Show only the first reference for a
commit in the table` **on** · `Show tag names in the table` **off** (§7.3
closed) · `Show the time when the change was committed rather than authored`
off · `Show references on the left of commit message` off ·
`Display changes to each merged commit separately` off ·
`Show the issue preview on hover over an issue id in 500 ms` on ·
`Show the diff preview panel` **off**, Location ⦿ Bottom (§7.3 closed) ·
Visible Columns: Author on · Hash off · Date on · GPG Signature off ·
Indexing: `Enable indexing for project` **on** · File History: Display details
panel off · Show file names off · **Show the diff preview panel on, Location ⦿
Right** (contrast with the Log's off/Bottom) · same Visible Columns.

### Version Control | GitHub / GitLab
Both: "No accounts added / Add account… (⌘N)" empty state, `+ − ✓` toolbar.
GitHub: Clone git repositories using ssh off · Automatically mark opened files
as viewed off · Enable unread markers on Pull Requests **on** · Connection
timeout 5 seconds. GitLab: mark-opened off · Clone using SSH off.

### Advanced Settings (partial)
Enable Islands UI for custom themes off · Show only line bookmarks in popup on ·
Restore bookmark context on branch switching on · All-in-one Diff for Code
Reviews off · Keep increased margins for Diff threads on · Maximum number of
recent expressions 50 · Show inlay Run to Cursor popup on.

## Widget hierarchy (partial, §7.2)

Full-window screenshots (`fullstate*.png`) show the main window: Project tool
window (tree + type-ahead search), editor tab strip (`Log | M+ README.md` —
M+ = modified marker), Log tool window with its filter toolbar, right-edge
notification bell, gear, status bar (TextMate, 1:1, LF, UTF-8, 4 spaces,
PsiDirectory path). Commit tool window was not opened — its topology capture
remains open, along with dialog button order.

## D8 — attempts and failures this session

| Attempt | Result |
|---|---|
| System Events `click at` | worked twice, then failed persistently (-10000) even with the dialog open — replaced by a JXA `CGEvent` mouse/scroll bridge |
| `entire contents` AX dump of the settings window | failed (-10000, huge Swing tree) — replaced by typed accessors + screenshots |
| AX row clicking in the settings sidebar | landed on the wrong page repeatedly (row name extraction went stale) — replaced by search + coordinate clicks |
| Two keystroke batches escaped into the editor/Project-tree type-ahead | no damage: disk file verified intact, nothing saved; recorded here per D8 |
| Escape key | closes the whole Settings dialog (not just a sub-dialog) — discovered, recorded |
| Lazy submenus (Git ▸ Patch, Git ▸ Selected Directory) | render empty to AX; need hover — still open |
| Context menus / Commit tool window topology | not captured this session — still open |

## What this closes

- **D6**: main menu bar ordering — closed (context menus/dialog buttons remain).
- **D7**: ~30 default values captured. Remaining open: the six ⌃O commit
  options, patch-encoding default, `Use Safe Force Push` checkbox state,
  `Recursively clone submodules` checkbox state, dialog-level flags in
  merge/rebase/pull dialogs, the staging-area save-vs-delete contradiction
  (this session's page shows only "This will disable changelists support").
