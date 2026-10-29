# History-reword mapping table (ADR-0028 D8. Gate 1 draft)

**Status: APPROVED (Gate 1), being executed (Gate 2 confirmed).** The operator
approved the table and the force-push; the reword runs against `main` via
`git filter-branch --msg-filter` applying the identical first-line-only dict below
(`git-filter-repo` is unavailable on this machine and ruled out by the operator,
the callback snippet stays as the record of the approved mapping).

Why `docs/` and not the issue tracker: the table must survive the history rewrite, a
fresh-clone verification looks it up afterward, and `.scratch/` is gitignored.

## Provenance

- Sweep: `git log --format=%s main` over the whole of `main`, **383 subjects =
  314 non-merge commits + 69 merge commits**, plus the merge-only sweep
  `git log --format=%s --merges main`.
- Parser: cocogitto 7.0.0 as configured by this repo's `cog.toml`, driven through
  `commit-lint` (the bin `cog verify` agrees with). Classification was mechanical:
  every one of the 314 non-merge subjects was fed to `commit-lint` individually,
  212 parse, 102 fail.
- Result: **83 unique non-conventional subjects** (covering those 102 commits) are
  mapped below; every row is enumerated, there is no "…and others".
- Reconciliation with ADR-0028's "roughly forty-five": the sweep found nearly twice
  that, 83 unique subjects / 102 commits. The ADR named the families (`issue-N:`,
  `wip`, `icons`, `improve ui`, `foundation`, `cleanup`, …) but undercounted them:
  the `issue-N:` family alone is 22 subjects, `welcome:` is 9, the bare `refactor`
  subject repeats on 14 commits, `ui adjustments` on 5. Per the ticket, the sweep is
  authoritative; the ADR's estimate was an eyeball figure.

## Rewording conventions

Applied consistently across the table so the choices review as a rule set, not 83
one-offs:

- **Type honesty.** New user-facing capability (pane, card, dialog, flow, engine
  capability) → `feat`; corrections → `fix`; restructuring/redesign without new
  capability → `refactor`; pure visual alignment/token tweaks → `style`; test-only →
  `test`; docs-only → `docs`; CI → `ci`; toolchain → `build`; dependency bumps and
  bookkeeping → `chore` (scope `deps` where it is a bump). Where a commit mixes
  concerns, the type follows the commit's dominant file mass; the note says so.
- **Scope.** Free-form per D1. UI-side work takes `ui`; an original pseudo-type tag
  that named an area (`welcome:`, `icons:`) becomes that scope verbatim.
- **`issue-N:` prefix** (the app's issue tracker) becomes a trailing `(issue-N)` ref,
  mirroring the `(#7)` PR-ref style already present in the history. The
  `(ddd split issue NN)` parentheticals refer to a different tracker and are kept
  verbatim where they already trail.
- **Minimal touch.** Where the original wording already reads as a subject, it is kept
  verbatim after `type(scope): `, only the type prefix is added. Rewording happens
  only where the original is unusable as a subject (bare `refactor`, `foundation`,
  `cleanup`, `redesign`, `fix bugs`, …), and the note records the diff evidence used.
- **Repeated subjects share one replacement.** The callback dict is keyed by the
  message's first line, so two commits with the same subject necessarily get the same
  new subject. This matters only for `refactor` (×14) and `ui adjustments` (×5), whose
  occurrences are heterogeneous; their shared replacements are necessarily generic and
  the occurrences are listed per row.
- **Meaning notes** below cite the inspected diff (`git show <sha> --stat`) wherever
  the original subject was too vague to map from its wording alone.

## The mapping table (83 rows)

| Old subject | New subject | Commit(s) | Note |
|---|---|---|---|
| `Initial commit` | `chore: initial commit` | `bddd8b33d` | adds `.gitignore` + `README.md` only |
| `foundation` | `chore: set up the project foundation` | `decc30eff` | 40 files: workspace `Cargo.toml`/lock, `CONTEXT.md`, bundled fonts/assets, scaffolding, not a feature |
| `add agent instructions` | `docs: add agent instructions` | `5068cbdf6` | creates `AGENTS.md` |
| `add design system pointer` | `docs: add design system pointer` | `1dad97392` | `AGENTS.md` addition |
| `add ci` | `ci: add the CI workflow` | `68a4ffb1f` | creates `.github/workflows/ci.yml` |
| `bump rust edition` | `build: bump the rust edition` | `7b30a4c1f` | edition migration across 31 files |
| `update dependencies` | `chore(deps): update dependencies` | `36d0d0c17` | `Cargo.toml`/lock + two test files adjusted for the bump |
| `adjust ui` | `refactor: adjust the ui` | `ff492220e` | 16 files: theme + several panes; visual pass, no new capability |
| `improve ui` | `refactor: improve the ui` | `c23957bb1` | 32 files, ~11.6k insertions incl. core and deps; broad UI rework |
| `improve design` | `refactor(ui): improve the design` | `18aa95a21` | 60 files, ~25k insertions; design-system pass |
| `improve branches view design` | `refactor(ui): improve the branches view design` | `5d3ee0f85` | 14 files: branches view, theme, app state |
| `redesign` | `refactor(ui): redesign the interface` | `f4d0fdcee` | 172 files, ~40k insertions; the umbrella redesign commit |
| `ui adjustments` | `refactor(ui): adjust the ui` | `8791c125e`, `16f0c4d9`, `1b72f499`, `5e216e9a`, `1c6ab9b2` | ×5, shared replacement; occurrences range from a one-pane `log_window` tweak to a 31-file app+ui pass |
| `ui adjustments and performance optimization` | `perf: ui adjustments and performance optimization` | `0b690b86a` | app events/root_caches/state; the commit itself claims the perf gain, so `perf` is the honest type |
| `ui adjustments, performance improvement` | `perf: ui adjustments and performance improvement` | `ee4adb698` | 29 files incl. persistence; same mixed shape |
| `adjust project tree indentation` | `style(ui): adjust the project tree indentation` | `e289e0a4a` | theme + tree/sidebar spacing only |
| `refactor` | `refactor: general codebase refactoring` | `a9127ec8`, `5ad7f1a4`, `ac13c2ca`, `693dba34`, `53b104c3`, `df81a423`, `d90fba7b`, `9d59e256`, `e358c7ba`, `14a957bc`, `0508565c`, `5d1a3043`, `1d2a2615`, `a1ebd422` | ×14, one shared replacement, the dict is keyed by subject and the originals carried no detail to preserve. Occurrences span UI passes, core-service reworks, doc pruning (`a9127ec8` removes ~77k lines) and CI/workspace churn |
| `refactoring` | `refactor: rework the app diff plumbing` | `18035c0b9` | 30 files, mostly turbogit-app `diff_data`/`diff_load`/`diff_model`/`granular`/`keyed_read` |
| `refactor caching` | `refactor: rework the caching` | `755cd6817` | engine + `root_caches` + lib wiring |
| `refactor ui` | `refactor: rework the ui` | `6b44d5288` | app state + branches UI, net −200 lines |
| `worktree refactor` | `refactor: rework the worktree lifecycle` | `45ea1dbd4` | app `worktree_lifecycle` + events/state/shell |
| `add branch context menu` | `feat(ui): add branch context menu` | `a50c8fb8a` | branch menu + tree view + `widgets/menu` |
| `context menu for commits` | `feat(ui): context menu for commits` | `f4a46faa7` | log commit actions through app operation/`shell_reads` |
| `add virtual scroll` | `feat: add virtual scroll` | `8694fa32c`, `173a6e25` | ×2; first spans app+ui+engine wiring, second continues it on the diff (ADR-0014) |
| `branches grouping by repo` | `feat(ui): branches grouping by repo` | `deec03915` | new grouping across app state + branches UI |
| `implement branches` | `feat: implement branches` | `452c72ffd` | cross-crate: domain/engine/services + UI |
| `implement image and binary diff` | `feat: implement image and binary diff` | `60963c1d3` | ADR-0015; engine + core + UI |
| `implement partial staging` | `feat: implement partial staging` | `60ce09312` | ADR-0013; engine + core |
| `implement advance hunk` | `feat(ui): implement advance hunk` | `c45d5d6ba` | `hunk_nav` + commit window/diff |
| `paginated commit log` | `feat: paginated commit log` | `629ad1f6f` | app pagination + log window |
| `icons: lucide painter primitives with debug-log misses (#7)` | `feat(icons): lucide painter primitives with debug-log misses (#7)` | `fca7cea48` | the `icons:` pseudo-type becomes a scope; the `(#7)` PR ref is kept |
| `fix bugs` | `fix(ui): fix the branches view bugs` | `2b71684a8` | diff touches `branches.rs` + `components.rs` + their tests; scope inferred from the diff |
| `fix panic` | `fix(recents): fix the panic` | `7f9e70f0c` | turbogit-app `recents.rs` guard (+34/−1) |
| `fix image snapshot location` | `fix(test): fix the image snapshot location` | `aad9b8162` | snapshot directory move across 21 files |
| `changed files redesign` | `refactor(ui): redesign the changed files pane` | `4fc0861da` | 16 files: commit window/diff + app state |
| `changes view redesign` | `refactor(ui): redesign the changes view` | `ad9a32b92` | 10 files: commit window + widgets |
| `diff redesign` | `refactor(ui): redesign the diff pane` | `8e7dbdade` | diff module restructure |
| `cleanup` | `chore: remove the superseded redesign docs` | `435d771d2` | deletes `docs/execution-plan.md`, `ui-redesign-audit.md`, `ui-redesign-spec.md` (−1155 lines, no code) |
| `remove scratch folder` | `chore: remove the scratch folder` | `71e6fab4a`, `c3fce6b0` | ×2; first gitignores it, second deletes `.scratch/ddd-subcrate-split` |
| `remove useless content` | `refactor: remove the useless content` | `c88933c0c` | 25 files, app state + ui popups, mixed ins/del, broader than a deletion |
| `remove useless files` | `chore: remove the useless files` | `d67648a68` | deletes `MIGRATION_PLAN.md`, `ddd-subcrate-proposal.md`, `followups.md` |
| `remove useless remotes button` | `fix(ui): remove the useless remotes button` | `a6a20ffc9` | branches UI button + its test; correcting a mistake → `fix` |
| `remove useless toolbar` | `refactor: remove the useless toolbar` | `5dbb76ba4` | 33 files: harness, app state, tests, a removal pass, not a one-widget fix |
| `complete composition root cutover and remove shims (ddd split issue 10)` | `refactor: complete composition root cutover and remove shims (ddd split issue 10)` | `1b9cf886f` | ddd-split tracker ref kept verbatim |
| `extract turbogit-app crate (ddd split issue 08)` | `refactor: extract turbogit-app crate (ddd split issue 08)` | `1f07307f6` | structural extraction, no behaviour change |
| `extract turbogit-ui and test-support crates (ddd split issue 09)` | `refactor: extract turbogit-ui and test-support crates (ddd split issue 09)` | `c4679cc18` | same |
| `merge issue-16 into ui: keep typed-toast renderer and settings modal, drop legacy inline settings` | `refactor(ui): keep typed-toast renderer and settings modal, drop legacy inline settings (issue-16)` | `c3cdbc55b` | **not a git merge**, integrates the issue-16 topic into the ui stream; the `merge … into ui` pseudo-type is dropped |
| `test+docs: first tests through the engine seam; CONTEXT.md; ADR-0001` | `test: first tests through the engine seam; CONTEXT.md; ADR-0001` | `1d9c98ef2` | mixed commit; `test` chosen as the type (cocogitto takes one), the docs half stays visible in the subject |
| `issue-6: shared widget library with token-driven state styling` | `feat(ui): shared widget library with token-driven state styling (issue-6)` | `ab044b4d2` | |
| `issue-9: IDE shell frame — topbar, toolbar, rail, tab strip, status bar` | `feat(ui): IDE shell frame — topbar, toolbar, rail, tab strip, status bar (issue-9)` | `8e044e51f` | |
| `issue-10: global recents store + launch/open/close state seams` | `feat: global recents store + launch/open/close state seams (issue-10)` | `9db6a5baf` | app-level, hence no `ui` scope |
| `issue-10: welcome screen — action cards, recents, live branch indicators, launch flow` | `feat(ui): welcome screen — action cards, recents, live branch indicators, launch flow (issue-10)` | `ac3e4d7f1` | |
| `issue-11: commit tool window — canonical changelists, preview, actions` | `feat(ui): commit tool window — canonical changelists, preview, actions (issue-11)` | `2e3e56ea0` | |
| `issue-12: Git Log four-pane workspace — branches, graph, changed files, details` | `feat(ui): Git Log four-pane workspace — branches, graph, changed files, details (issue-12)` | `9ddce39f0` | |
| `issue-12: engine seam — ref decorations, per-commit changed files, full log messages` | `feat: engine seam — ref decorations, per-commit changed files, full log messages (issue-12)` | `341f6f534` | engine capability → `feat`, no `ui` scope |
| `issue-12: cargo fmt housekeeping for previously unformatted sources` | `style: cargo fmt housekeeping for previously unformatted sources (issue-12)` | `cc409ea98` | formatting only → `style` |
| `issue-13: budget test settle by wall-clock time instead of frame count` | `test: budget test settle by wall-clock time instead of frame count (issue-13)` | `bc833c9f7` | |
| `issue-13: restyle diff viewer onto tokens - mode control, comparison chips, hunk nav` | `feat(ui): restyle diff viewer onto tokens - mode control, comparison chips, hunk nav (issue-13)` | `d40fbfc63` | adds mode control/chips/hunk nav → `feat`, not `style` |
| `issue-13: tooling unblock - gix_reader placeholder + cargo fmt housekeeping` | `chore: tooling unblock - gix_reader placeholder + cargo fmt housekeeping (issue-13)` | `fb18183af` | |
| `issue-14: branches popup — groups, row actions, keyboard flow` | `feat(ui): branches popup — groups, row actions, keyboard flow (issue-14)` | `e9de765e6` | |
| `issue-15: three-pane merge editor with read-only composed result` | `feat(ui): three-pane merge editor with read-only composed result (issue-15)` | `dce6e3787` | |
| `issue-16: settings as gear-only category-list modal` | `feat(ui): settings as gear-only category-list modal (issue-16)` | `1ef0e7b3d` | |
| `issue-17: welcome clone card end-to-end tests + windows path name fix` | `test: welcome clone card end-to-end tests + windows path name fix (issue-17)` | `9baa50af4` | tests dominate; the path-name fix stays in the subject |
| `issue-18: commit window sub-tabs with phase-j placeholders and inert advanced options` | `feat(ui): commit window sub-tabs with phase-j placeholders and inert advanced options (issue-18)` | `588628357` | |
| `issue-19: path-scoped file history from the log context menu` | `feat(ui): path-scoped file history from the log context menu (issue-19)` | `b1abeb75f` | |
| `issue-20: push dialog core with aggregated outgoing tree` | `feat(ui): push dialog core with aggregated outgoing tree (issue-20)` | `105972af4` | |
| `issue-21: push dialog safety — verbatim dry-run preview, force-with-lease ack, protected-branch blocking` | `feat(ui): push dialog safety — verbatim dry-run preview, force-with-lease ack, protected-branch blocking (issue-21)` | `4a1080348` | |
| `issue-22: semantic typed toasts, dialog/popup chrome, palette shell actions` | `feat(ui): semantic typed toasts, dialog/popup chrome, palette shell actions (issue-22)` | `0e20e5950` | |
| `issue-23: focus rings, scroll hardening, shortcut/perf audit` | `fix(ui): focus rings, scroll hardening, shortcut/perf audit (issue-23)` | `b7ccc2c27` | rings + scroll hardening are corrections; the audit is a doc rider |
| `issue-24: legacy dead-code gates removed, acceptance screenshots, owed spec corrections` | `chore: legacy dead-code gates removed, acceptance screenshots, owed spec corrections (issue-24)` | `5f372e418` | lint-gate removal in `Cargo.toml` + docs/screenshots → `chore` |
| `welcome: add getting-started card and the lower two-column band` | `feat(welcome): add getting-started card and the lower two-column band` | `39186d89c` | `welcome:` pseudo-type → scope, for all nine rows below |
| `welcome: align card/panel right edges and unify the horizontal gutter` | `style(welcome): align card/panel right edges and unify the horizontal gutter` | `758a00ecb` | alignment only |
| `welcome: cardify recents with a CONTENT_BG frame, SELECTION branch chip` | `refactor(welcome): cardify recents with a CONTENT_BG frame, SELECTION branch chip` | `ccc469716` | restructure without new capability |
| `welcome: hero replaces the brand header and What's-new link` | `refactor(welcome): hero replaces the brand header and What's-new link` | `a3d160977` | restructure |
| `welcome: lock the redesign in tests and repair the r4_polish welcome seam` | `test(welcome): lock the redesign in tests and repair the r4_polish welcome seam` | `2cfee8faf` | both files touched are test suites |
| `welcome: merge the clone door and cut quick actions to three cards` | `refactor(welcome): merge the clone door and cut quick actions to three cards` | `7fdd4ee28` | consolidation, net −32 lines |
| `welcome: route the changelog overlay radius through CARD_RADIUS` | `refactor(welcome): route the changelog overlay radius through CARD_RADIUS` | `685a2926a` | one-line token routing |
| `welcome: size the getting-started card to one card module under Attach` | `style(welcome): size the getting-started card to one card module under Attach` | `cc5fe6d2d` | sizing only |
| `welcome: size the hero band to its content so the text keeps padding` | `style(welcome): size the hero band to its content so the text keeps padding` | `5fa46ed1d` | sizing only |
| `wip: create a patch as git's own bytes, with base path and the counting repair pair` | `feat: create a patch as git's own bytes, with base path and the counting repair pair` | `945d1339e` | ticket-18b work across domain/engine-api/app → `feat` |
| `wip: reflog-derived recent checkouts port and adapter with branches-recent suite` | `feat: reflog-derived recent checkouts port and adapter with branches-recent suite` | `ec8a4bb62` | engine-api/engine port + adapter |
| `wip: ticket 02 conflict kinds (agent interrupted, partial)` | `test: ticket 02 conflict kinds (agent interrupted, partial)` | `202d397ca` | both files touched are test suites → `test`, not `feat` |
| `wip: ticket 13 push rejections (resumed)` | `feat: ticket 13 push rejections (resumed)` | `31894716b` | operation/model/`sync_service` + push-dialog test |

## Merge commits: retained, not mapped (D8)

All **69 merge commits** on `main` are retained exactly as they are, their subjects are
never mapped and the callback dict below cannot touch them (verified: zero of the 83 old
subjects appear as any merge-commit subject). The tooling skips them by design:
`cog check`/`cog verify` skip git-level merges via parentage
(`ignore_merge_commits = true` in `cog.toml`), and the commit-msg hook skips messages
beginning `Merge ` / `Pull request` inside cocogitto's `verify`.

Breakdown of the 69 retained subjects:

- **5 × `Merge pull request #N`**. GitHub's merge subjects, called out per D8:
  - `30110de`, `Merge pull request #1 from strawberry-choco/arch`
  - `4865403`, `Merge pull request #25 from strawberry-choco/ui`
  - `c9b2a87`, `Merge pull request #26 from strawberry-choco/arch`
  - `8a64126`, `Merge pull request #27 from strawberry-choco/arch-2`
  - `b84647b``Merge pull request #28 from strawberry-choco/strawberry-choco/partial-staging`
- 25 × `Merge branch '…' into …`
- 26 × `merge: ticket …` (hand-written merge-commit subjects from the ticket era)
- 1 × `merge issue-13 into ui: …` (same family, older shape)
- 12 × conventional-typed subjects that happen to sit on git-level merge commits
  (e.g. `feat(ui): copy the branch a row names from its context menu`), retained like
  the rest; D8 keeps merge commits, not merge subjects, out of the rewrite

## Ready-to-run rewrite command (executed only after Gate-1 approval, by a later ticket)

`git filter-repo` is not installed on this machine and must not be run now, this
snippet is the verbatim input for the human-gated execution ticket. It applies the dict
to each commit's **first message line only**; message bodies, authors, committer
dates/timestamps and parentage (hence merge topology) are untouched, and any first line
not in the dict (all 69 merge subjects, all 212 already-conventional subjects) passes
through byte-identical.

```sh
git filter-repo --message-callback '
MAPPING = {
    "Initial commit": "chore: initial commit",
    "foundation": "chore: set up the project foundation",
    "add agent instructions": "docs: add agent instructions",
    "add design system pointer": "docs: add design system pointer",
    "add ci": "ci: add the CI workflow",
    "bump rust edition": "build: bump the rust edition",
    "update dependencies": "chore(deps): update dependencies",
    "adjust ui": "refactor: adjust the ui",
    "improve ui": "refactor: improve the ui",
    "improve design": "refactor(ui): improve the design",
    "improve branches view design": "refactor(ui): improve the branches view design",
    "redesign": "refactor(ui): redesign the interface",
    "ui adjustments": "refactor(ui): adjust the ui",
    "ui adjustments and performance optimization": "perf: ui adjustments and performance optimization",
    "ui adjustments, performance improvement": "perf: ui adjustments and performance improvement",
    "adjust project tree indentation": "style(ui): adjust the project tree indentation",
    "refactor": "refactor: general codebase refactoring",
    "refactoring": "refactor: rework the app diff plumbing",
    "refactor caching": "refactor: rework the caching",
    "refactor ui": "refactor: rework the ui",
    "worktree refactor": "refactor: rework the worktree lifecycle",
    "add branch context menu": "feat(ui): add branch context menu",
    "context menu for commits": "feat(ui): context menu for commits",
    "add virtual scroll": "feat: add virtual scroll",
    "branches grouping by repo": "feat(ui): branches grouping by repo",
    "implement branches": "feat: implement branches",
    "implement image and binary diff": "feat: implement image and binary diff",
    "implement partial staging": "feat: implement partial staging",
    "implement advance hunk": "feat(ui): implement advance hunk",
    "paginated commit log": "feat: paginated commit log",
    "icons: lucide painter primitives with debug-log misses (#7)": "feat(icons): lucide painter primitives with debug-log misses (#7)",
    "fix bugs": "fix(ui): fix the branches view bugs",
    "fix panic": "fix(recents): fix the panic",
    "fix image snapshot location": "fix(test): fix the image snapshot location",
    "changed files redesign": "refactor(ui): redesign the changed files pane",
    "changes view redesign": "refactor(ui): redesign the changes view",
    "diff redesign": "refactor(ui): redesign the diff pane",
    "cleanup": "chore: remove the superseded redesign docs",
    "remove scratch folder": "chore: remove the scratch folder",
    "remove useless content": "refactor: remove the useless content",
    "remove useless files": "chore: remove the useless files",
    "remove useless remotes button": "fix(ui): remove the useless remotes button",
    "remove useless toolbar": "refactor: remove the useless toolbar",
    "complete composition root cutover and remove shims (ddd split issue 10)": "refactor: complete composition root cutover and remove shims (ddd split issue 10)",
    "extract turbogit-app crate (ddd split issue 08)": "refactor: extract turbogit-app crate (ddd split issue 08)",
    "extract turbogit-ui and test-support crates (ddd split issue 09)": "refactor: extract turbogit-ui and test-support crates (ddd split issue 09)",
    "merge issue-16 into ui: keep typed-toast renderer and settings modal, drop legacy inline settings": "refactor(ui): keep typed-toast renderer and settings modal, drop legacy inline settings (issue-16)",
    "test+docs: first tests through the engine seam; CONTEXT.md; ADR-0001": "test: first tests through the engine seam; CONTEXT.md; ADR-0001",
    "issue-6: shared widget library with token-driven state styling": "feat(ui): shared widget library with token-driven state styling (issue-6)",
    "issue-9: IDE shell frame — topbar, toolbar, rail, tab strip, status bar": "feat(ui): IDE shell frame — topbar, toolbar, rail, tab strip, status bar (issue-9)",
    "issue-10: global recents store + launch/open/close state seams": "feat: global recents store + launch/open/close state seams (issue-10)",
    "issue-10: welcome screen — action cards, recents, live branch indicators, launch flow": "feat(ui): welcome screen — action cards, recents, live branch indicators, launch flow (issue-10)",
    "issue-11: commit tool window — canonical changelists, preview, actions": "feat(ui): commit tool window — canonical changelists, preview, actions (issue-11)",
    "issue-12: Git Log four-pane workspace — branches, graph, changed files, details": "feat(ui): Git Log four-pane workspace — branches, graph, changed files, details (issue-12)",
    "issue-12: engine seam — ref decorations, per-commit changed files, full log messages": "feat: engine seam — ref decorations, per-commit changed files, full log messages (issue-12)",
    "issue-12: cargo fmt housekeeping for previously unformatted sources": "style: cargo fmt housekeeping for previously unformatted sources (issue-12)",
    "issue-13: budget test settle by wall-clock time instead of frame count": "test: budget test settle by wall-clock time instead of frame count (issue-13)",
    "issue-13: restyle diff viewer onto tokens - mode control, comparison chips, hunk nav": "feat(ui): restyle diff viewer onto tokens - mode control, comparison chips, hunk nav (issue-13)",
    "issue-13: tooling unblock - gix_reader placeholder + cargo fmt housekeeping": "chore: tooling unblock - gix_reader placeholder + cargo fmt housekeeping (issue-13)",
    "issue-14: branches popup — groups, row actions, keyboard flow": "feat(ui): branches popup — groups, row actions, keyboard flow (issue-14)",
    "issue-15: three-pane merge editor with read-only composed result": "feat(ui): three-pane merge editor with read-only composed result (issue-15)",
    "issue-16: settings as gear-only category-list modal": "feat(ui): settings as gear-only category-list modal (issue-16)",
    "issue-17: welcome clone card end-to-end tests + windows path name fix": "test: welcome clone card end-to-end tests + windows path name fix (issue-17)",
    "issue-18: commit window sub-tabs with phase-j placeholders and inert advanced options": "feat(ui): commit window sub-tabs with phase-j placeholders and inert advanced options (issue-18)",
    "issue-19: path-scoped file history from the log context menu": "feat(ui): path-scoped file history from the log context menu (issue-19)",
    "issue-20: push dialog core with aggregated outgoing tree": "feat(ui): push dialog core with aggregated outgoing tree (issue-20)",
    "issue-21: push dialog safety — verbatim dry-run preview, force-with-lease ack, protected-branch blocking": "feat(ui): push dialog safety — verbatim dry-run preview, force-with-lease ack, protected-branch blocking (issue-21)",
    "issue-22: semantic typed toasts, dialog/popup chrome, palette shell actions": "feat(ui): semantic typed toasts, dialog/popup chrome, palette shell actions (issue-22)",
    "issue-23: focus rings, scroll hardening, shortcut/perf audit": "fix(ui): focus rings, scroll hardening, shortcut/perf audit (issue-23)",
    "issue-24: legacy dead-code gates removed, acceptance screenshots, owed spec corrections": "chore: legacy dead-code gates removed, acceptance screenshots, owed spec corrections (issue-24)",
    "welcome: add getting-started card and the lower two-column band": "feat(welcome): add getting-started card and the lower two-column band",
    "welcome: align card/panel right edges and unify the horizontal gutter": "style(welcome): align card/panel right edges and unify the horizontal gutter",
    "welcome: cardify recents with a CONTENT_BG frame, SELECTION branch chip": "refactor(welcome): cardify recents with a CONTENT_BG frame, SELECTION branch chip",
    "welcome: hero replaces the brand header and What's-new link": "refactor(welcome): hero replaces the brand header and What's-new link",
    "welcome: lock the redesign in tests and repair the r4_polish welcome seam": "test(welcome): lock the redesign in tests and repair the r4_polish welcome seam",
    "welcome: merge the clone door and cut quick actions to three cards": "refactor(welcome): merge the clone door and cut quick actions to three cards",
    "welcome: route the changelog overlay radius through CARD_RADIUS": "refactor(welcome): route the changelog overlay radius through CARD_RADIUS",
    "welcome: size the getting-started card to one card module under Attach": "style(welcome): size the getting-started card to one card module under Attach",
    "welcome: size the hero band to its content so the text keeps padding": "style(welcome): size the hero band to its content so the text keeps padding",
    "wip: create a patch as git's own bytes, with base path and the counting repair pair": "feat: create a patch as git's own bytes, with base path and the counting repair pair",
    "wip: reflog-derived recent checkouts port and adapter with branches-recent suite": "feat: reflog-derived recent checkouts port and adapter with branches-recent suite",
    "wip: ticket 02 conflict kinds (agent interrupted, partial)": "test: ticket 02 conflict kinds (agent interrupted, partial)",
    "wip: ticket 13 push rejections (resumed)": "feat: ticket 13 push rejections (resumed)",
}
MAPPING = {k.encode(): v.encode() for k, v in MAPPING.items()}
nl = message.find(b"\n")
first = message if nl == -1 else message[:nl]
if first in MAPPING:
    new = MAPPING[first]
    message = new if nl == -1 else new + message[nl:]
return message
'
```

The `bytes` conversion keeps the dict readable as UTF-8 text while the callback works on
the `bytes` `commit.message` git-filter-repo supplies. First-line-only replacement means
a multi-line message keeps its body verbatim; the merge commits are unreachable by
construction (none of their first lines is a dict key) even though the callback runs on
them.

## Verification (mechanical, every row, no sampling)

How this table was verified, all against the repo's own parser (`commit-lint`, i.e.
cocogitto 7.0.0 reading this `cog.toml`):

1. **Complete classification.** All 314 non-merge subjects from `git log --format=%s
   main` were fed to `commit-lint` one message per file: **212 pass, 102 fail**. The 102
   failing commits reduce to exactly the 83 unique old subjects in the table above,
   the table's old-subject set is set-equal to the parser-failing set, so the
   enumeration is complete by construction, not by review.
2. **Old side fails.** Each of the 83 old subjects was written to a temp file and run
   through `cargo run -p commit-lint -- <file>` (prebuilt binary): **83/83 exit
   non-zero** with a parse error. No row relies on sampling.
3. **New side passes.** Each of the 83 new subjects, same procedure: **83/83 exit 0**.
   (Checked edge cases that could silently pass/fail: no new subject begins with
   `Merge ` or `Pull request`, which cocogitto's `verify` would skip instead of parse;
   scopes `welcome`/`icons`/`recents`-style free-form and trailing `(issue-N)` refs in
   the subject all parse under the free-form `cog.toml`.)
4. **Merge isolation.** The 83 old subjects were intersected with the 69 merge-commit
   subjects: **zero collisions**, so the dict can never reword a retained merge commit.
5. **Subject-line integrity.** For all 314 non-merge commits, the literal first line of
   `%B` equals `%s` (0 mismatches, 0 comment-led first lines), so the `%s`-derived dict
   keys are exactly the first lines the callback will see, and bodies are never
   consulted.
6. **Dict/table agreement.** The 83-entry dict in the snippet above and the 83 rows of
   the table were cross-checked programmatically (parsed from this file): same keys,
   same values, no duplicates.

No Rust code is touched by this ticket, so the four cargo quality gates are not
exercised here; the only artifact is this document.
