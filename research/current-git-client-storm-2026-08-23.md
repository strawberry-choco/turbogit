# Current Git Client STORM — Pain Points, Features, and Market

Snapshot date: **2026-08-23**. This report uses a STORM-style method: multiple
perspectives, contradiction mapping, synthesis, and self-critique. It is **not** a
complete market audit. Product claims below use only official pages or repositories
already captured during this session; absent facts are marked `GAP`, not inferred.

## Method and evidence rules

- **Question:** What pain points do Git client users have, what feature sets do major
  clients expose, and where is the market concentrated?
- **Scope:** GitHub Desktop, GitKraken, Sourcetree, Fork, Tower, Sublime Merge,
  SmartGit, lazygit, Magit, VS Code, JetBrains IDEs, Visual Studio, Xcode, and
  Eclipse/EGit.
- **Evidence:** Official product/docs/pricing/source pages were preferred. Claims are
  tied to exact URLs in the profiles. Marketing language is identified as vendor
  positioning, not independent proof of satisfaction.
- **Completeness warning:** Research collection stopped early by user request. Several
  tools therefore have substantial gaps. The comparison matrix intentionally omits cells
  that cannot be supported by a current source.
- **Pain-point caveat:** The ranked pain points synthesize the repository's prior
  research and current product claims. They identify likely design opportunities; they
  do not quantify prevalence or user demand without primary-user research.

## Executive findings

1. **The market is editor/IDE-centric, but standalone clients still sell workflow.**
   GitHub Desktop is explicitly positioned as collaboration from the desktop for both new
   and seasoned users; Tower and SmartGit position around professional workflows; Magit is
   an Emacs porcelain; VS Code documents staging, graph/history, worktrees, conflicts, and
   GitHub integration inside the editor.
2. **Safety and guided recovery are recurring paid-client themes.** Tower advertises undo,
   hunk/line staging, conflict guidance, reflog recovery, drag-and-drop interactive rebase,
   Git LFS, submodules, GPG, and worktrees. SmartGit advertises undo and clean history.
3. **Conflict UX has moved from raw markers to guided surfaces.** VS Code offers inline
   CodeLens actions plus a three-way merge editor and experimental AI assistance; Tower
   advertises a Conflict Wizard; Fork advertises a merge-conflict helper/resolver;
   Visual Studio documents conflict handling and links to a dedicated resolver flow.
4. **Partial staging is broadly valuable but unevenly presented.** VS Code supports file,
   line, and selection staging; Tower advertises files, hunks, and lines. The prior local
   study found discoverability and keyboard power often split across tools.
5. **Cross-platform coverage remains a practical differentiator for native clients.**
   Fork and Tower document macOS/Windows only, while SmartGit documents Windows/macOS/Linux.
   Linux support for other requested clients was not verified in this pass.

## Market signals and limitations

- **No reliable public market-share table for Git clients** was available in the collected
  evidence. Therefore this report does not estimate client shares.
- **Concentration signal:** Visual Studio's documentation says its Git experience is
  optimized for GitHub and describes GitHub/Azure DevOps account integration; VS Code's
  source-control overview includes a “Collaborate on GitHub” section. These support IDE
  integration with hosted platforms but do not prove installed-base dominance.
- **Survey data gap:** Stack Overflow, JetBrains Developer Ecosystem, and Octoverse were
  not retrieved in this pass. Any Git-population or tool-usage claim from those surveys is
  deferred as a follow-up.
- **Competitive boundary:** Free editors/IDEs can commoditize basic Git operations, while
  paid standalone clients differentiate on safety, speed, conflict guidance, advanced
  history editing, team hosting integrations, and cross-platform polish. This is an
  interpretation of documented features and pricing, not measured willingness to pay.

## STORM perspectives

### Practitioner

Daily work rewards short loops: inspect change, stage exactly what belongs together,
resolve the next conflict, recover from a mistake, and continue. The strongest current
signals are explicit file/hunk/line staging in Tower and VS Code, interactive-history
operations in GitHub Desktop/Tower/Fork/SmartGit, and guided conflict surfaces in VS Code,
Tower, Fork, and Visual Studio. Opportunity: make the fast path obvious rather than hidden.

### Skeptic

A client must justify itself over the Git CLI by reducing risk and cognitive load, not just
duplicating commands. Documented undo/reflog recovery, visual history, guided rebase, and
three-way merge editors address this. Unsupported performance superiority should not be
claimed without hands-on benchmarks.

### Economist

Observed models range from free/open-source to paid evaluation, perpetual license, and per-
user annual subscriptions: Fork documents $59.99 after free evaluation; Tower documents
$69 and $99 per user annually; GitHub Desktop's source states MIT licensing; Microsoft's
VS Code product license differs from the MIT source-code license. Pricing alone does not
explain preference; it must be paired with workflow value and switching costs.

### Accessibility specialist

Accessibility could not be compared across the full set because current official
accessibility statements were not collected. Keyboard-first interaction, screen-reader
semantics, contrast, scaling, and conflict-navigation flows remain under-evidenced here.
This is a high-value validation area before making product claims.

### Platform strategist

IDEs integrate version control into code, tests, debugging, accounts, and cloud PR/issue
flows. Standalone clients can win on multi-host neutrality, focused graph/staging/conflict
UX, and OS parity. TurboGit's opportunity is not another feature checklist; it is a safe,
fast, accessible core loop with transparent Git semantics.

## Contradiction map

| Tension | Evidence-backed observation | Resolution for synthesis |
| --- | --- | --- |
| Feature breadth vs. flow depth | Tower lists many capabilities while VS Code emphasizes staged changes, graph/history, worktrees, conflicts, and GitHub collaboration. | Breadth matters less than discoverability of the daily loop. |
| Editor convenience vs. standalone focus | Visual Studio and VS Code embed Git near coding/cloud workflows; Tower/Fork/SmartGit sell dedicated Git surfaces. | Segment by context switching, repository scale, multi-host needs, and platform coverage—not by GUI versus IDE. |
| Safety abstraction vs. Git transparency | Guided conflict/rebase UIs reduce friction, but the collected pages do not establish whether users always understand underlying refs. | Pair guidance with command preview/recovery paths; validate with users. |
| Paid polish vs. free baseline | Tower/Fork monetize paid clients; GitHub Desktop is MIT licensed and VS Code source is MIT, though the distributed product uses a Microsoft license. | Compete on differentiated workflow quality, not simply free versus paid. |

## Ranked pain points and research hypotheses

Confidence reflects current evidence, not measured user prevalence.

| Rank | Pain point / hypothesis | Supporting signal | Confidence | Validation needed |
| --- | --- | --- | --- | --- |
| 1 | Partial-staging power and discoverability are hard to combine. | Tower advertises staging files/hunks/lines; VS Code documents file, line, and selection staging. Prior repo study found keyboard-powerful and mouse-discoverable flows split across tools. | Medium-high | Observe mixed-experience users staging interleaved edits; measure errors/time. |
| 2 | Conflicts require context, safe choices, and completion state. | VS Code documents inline actions, three-way editor, and experimental AI; Tower/Fork advertise helpers/wizards; Visual Studio links conflict resolution guidance. | High | Test complex rename/delete/semantic conflicts and interrupted merges/rebases. |
| 3 | History rewriting feels powerful but risky. | GitHub Desktop documents drag-and-drop cherry-pick/squash/reorder; Tower and Fork advertise interactive rebase; SmartGit advertises undo/clean history. | High | Measure comprehension of force-push consequences, ref backup, abort, and recovery. |
| 4 | Mistake recovery and trust need to be visible. | Tower advertises reflog recovery and undo; SmartGit advertises undo. | Medium-high | Test lost-commit scenarios and perception of reversible operations. |
| 5 | Large-repository performance and perceived heaviness may affect adoption. | Prior repo notes flag Electron/JVM concerns, but no current hands-on benchmark was completed. | Low-medium | Benchmark cold start, status, log/diff render, memory, and conflict loading on representative repos. |
| 6 | Cross-platform parity creates fragmented team standards. | Fork/Tower document macOS/Windows; SmartGit documents Windows/macOS/Linux. Other platform matrices were unverified. | Medium | Confirm current platform support and feature parity for every target tool. |
| 7 | Licensing/model friction may affect procurement and individual adoption. | Tower uses annual per-seat plans; Fork uses $59.99 after evaluation; VS Code distinguishes product license from MIT source. | Medium | Interview buyers and contributors about trial, seat, offline, and redistribution constraints. |
| 8 | Accessibility may be underserved, especially in graphical diff/merge surfaces. | Current accessibility sources were not collected; prior local study flagged this as a white space. | Low-medium | Audit screen-reader output, focus order, shortcuts, scaling, contrast, and reduced motion. |

## Per-tool profiles

Legend: **GAP** means no supporting current source was captured in this session. Do not
treat GAP as absence of functionality.

### GitHub Desktop

- **License:** MIT for the `desktop/desktop` development project (`LICENSE` grants copyright
  permission to use, copy, modify, merge, publish, distribute, sublicense, and sell copies).
  Source: <https://raw.githubusercontent.com/desktop/desktop-development/master/LICENSE>.
- **Platforms / price / model / target user:** Platform requirements, pricing model (free),
  and system requirements: **GAP** beyond the product page's broad positioning.
- **Git engine:** **GAP.**
- **Commit/staging:** Product page says users can review code changes precisely and easily
  compare versions. Source:
  <https://github.com/apps/desktop>.
- **Branch/history graph:** Product page advertises drag-and-drop cherry-pick, squash, or
  reorder commits, including copying commits between branches and altering branch history.
  Source: <https://github.com/apps/desktop>.
- **Conflicts:** **GAP** in this pass.
- **Partial staging:** **GAP** in this pass.
- **Rebase/interactive history:** See branch/history above; full interactive-rebase scope is
  **GAP**.
- **LFS/submodules/worktrees:** All **GAP** in this pass.
- **Integrations/AI/accessibility/collaboration-cloud:** Product page says Desktop enables
  desktop collaboration and simplified workflow for new and seasoned users. AI and
  accessibility specifics: **GAP**. Source: <https://github.com/apps/desktop>.
- **Differentiators:** GitHub-first desktop onboarding and simple collaboration positioning;
  MIT development-project license. Source URLs above.

### GitKraken

All requested dimensions are **GAP**: no current official pricing, platform, engine, or
feature page was successfully captured in this session.

### Sourcetree

All requested dimensions are **GAP**: no current official product/pricing/documentation page
was successfully captured in this session.

### Fork

- **License / price / model:** Proprietary terms are **GAP**, except the homepage's purchase
  link showing `$59.99` and “free evaluation.” Source: <https://git-fork.com/>.
- **Platforms / target user:** Homepage download cards show Mac on “OS X 10.11+” and Windows
  on “Windows 7+”; pricing text appears for both. Source: <https://git-fork.com/>.
- **Git engine:** **GAP.**
- **Commit/staging / partial staging:** **GAP** in this pass.
- **Branch/history graph:** **GAP** in this pass.
- **Conflicts:** Homepage says merge conflicts can be resolved using the helper and built-in
  resolver. Source: <https://git-fork.com/>.
- **Rebase/interactive history:** Homepage advertises edit, reorder, and squash commits with
  visual interactive rebase. Source: <https://git-fork.com/>.
- **LFS/submodules/worktrees:** Homepage lists Submodules, Git LFS, stashes, merge, rebase,
  blame, reflog restoration, and Git-flow. Worktrees: **GAP**. Source:
  <https://git-fork.com/>.
- **Integrations/AI/accessibility/collaboration-cloud:** **GAP.**
- **Differentiator:** One-time displayed price with free evaluation and visual interactive
  history editing on macOS/Windows. Sources above.

### Tower

- **License / platforms / price / model:** Structured pricing data identifies Tower as a
  SoftwareApplication for macOS and Windows, with Basic at USD 69.00/user/year and Pro at
  USD 99.00/user/year. FAQ says subscription access ends when the latest paid period ends;
  30-day trial includes all Pro features and requires no card; students, educational staff,
  institutions, and nonprofits may be eligible for free use. Sources:
  <https://www.git-tower.com/windows>; <https://www.git-tower.com/pricing>.
- **Target user:** Basic is described for individuals; Pro tiers add team management,
  billing/admin roles, enterprise management, invoicing, priority support, deployment, and
  SAML SSO. Source: <https://www.git-tower.com/pricing>.
- **Git engine:** **GAP.**
- **Commit/staging / partial staging:** Windows page advertises integrated staged/unstaged
  diffs and “Stage files hunks and lines,” also promoted as single-line staging. Source:
  <https://www.git-tower.com/windows>.
- **Branch/history graph:** File/blame/history and compare-branches features appear in the
  structured feature list; detailed graph behavior is **GAP**. Source:
  <https://www.git-tower.com/windows>.
- **Conflicts:** “Conflict wizard for guided resolutions.” Source:
  <https://www.git-tower.com/windows>.
- **Rebase/interactive history:** Interactive rebase with drag and drop; page headline also
  promotes Interactive Rebase. Source: <https://www.git-tower.com/windows>.
- **LFS/submodules/worktrees:** Structured list includes Git LFS, submodule fetch/update/
  open/manage, reflog recovery, GPG signing/verification, dark mode, external diff
  integration, patches, service accounts, pull-request creation/review/merge/close, and
  background fetch; the same page promotes worktrees. Sources:
  <https://www.git-tower.com/windows>; <https://www.git-tower.com/pricing> confirms
  cross-platform licenses.
- **Integrations/AI:** Cloud-service manager covers GitHub.com, Bitbucket.org, GitLab.com,
  and Azure DevOps in Basic; Pro adds self-managed/enterprise variants. Pull requests cover
  creation, review, merge, and close. AI: **GAP**. Source:
  <https://www.git-tower.com/pricing>.
- **Accessibility:** Dark mode is listed; fuller accessibility support is **GAP**. Source:
  <https://www.git-tower.com/windows>.
- **Collaboration/cloud:** Hosted-service manager and PR operations above. Source:
  <https://www.git-tower.com/pricing>.
- **Differentiators:** Broad advertised safety/history surface—undo, reflog recovery,
  guided conflicts, drag-and-drop rebase—with per-seat subscription and optional
  enterprise/team controls. Sources above.

### Sublime Merge

All requested dimensions are **GAP**: no current official pricing/product/documentation page
was successfully captured in this session.

### SmartGit

- **License / price / model:** Current commercial/non-commercial terms and prices are **GAP**
  despite the homepage audience labels “Free Open Source” and “Free Educational
  Institutions.” Eligibility conditions are not established by the captured homepage.
- **Platforms / target user:** Homepage accessibility label says “Available for Windows,
  macOS, and Linux”; audience labels include Development Teams, Enterprises, Power Users,
  Free Open Source, Free Educational Institutions, and Command Line Fans. Source:
  <https://www.syntevo.com/smartgit/>.
- **Git engine:** **GAP.**
- **Commit/staging:** Workflow copy says SmartGit manages commits, branches, and merges in a
  graphical interface. Detailed staging controls: **GAP**. Source:
  <https://www.syntevo.com/smartgit/>.
- **Branch/history graph:** Homepage advertises customizable visual history. Source:
  <https://www.syntevo.com/smartgit/>.
- **Conflicts:** Homepage advertises conflict resolution and says users review changes,
  resolve merge conflicts, and keep history clean. Source:
  <https://www.syntevo.com/smartgit/>.
- **Partial staging:** **GAP.**
- **Rebase/interactive history:** Homepage advertises clean commit history, smart branching,
  drag-and-drop, and undo; exact interactive-rebase operations are **GAP**. Source:
  <https://www.syntevo.com/smartgit/>.
- **LFS/submodules/worktrees:** Homepage feature chips include LFS; body content highlights
  Git submodules. Worktrees: **GAP**. Source: <https://www.syntevo.com/smartgit/>.
- **Integrations/AI:** Homepage screenshot alt text mentions integration with GitHub, GitLab,
  Azure DevOps, Bitbucket, and self-hosted Git; feature chips include Pull Requests. AI:
  **GAP**. Source: <https://www.syntevo.com/smartgit/>.
- **Accessibility:** **GAP.**
- **Collaboration/cloud:** Hosting-integration and pull-request signals above; detailed
  review capabilities are **GAP**.
- **Differentiator:** Advertised professional/team focus across Windows/macOS/Linux, visual
  history, conflicts, undo, LFS/submodule support, and multi-host integrations. Sources
  above.

### lazygit

All requested dimensions are **GAP**: README/license/release evidence was not successfully
captured in this session.

### Magit

- **License / price / model:** License, packaging, and support model are **GAP** beyond the
  homepage's free project positioning; no license page was captured.
- **Platform / target user:** Homepage calls Magit “A Git Porcelain inside Emacs,” implying
  availability wherever Emacs runs; exact supported versions/platforms are **GAP**. Target
  user is Emacs users. Source: <https://magit.vc/>.
- **Git engine:** **GAP.**
- **Commit/staging / partial staging:** **GAP** in this pass.
- **Branch/history graph:** **GAP** in this pass.
- **Conflicts:** **GAP.**
- **Rebase/interactive history:** **GAP.**
- **LFS/submodules/worktrees:** **GAP.**
- **Integrations/AI/accessibility/collaboration-cloud:** Homepage news mentions Forge v0.4.x,
  but Forge's relationship to Magit and its capabilities are **GAP**. Source:
  <https://magit.vc/>.
- **Differentiator:** Complete text-based Git UI inside Emacs, intended to bridge CLI and
  GUIs with mnemonic keys. Source: <https://magit.vc/>.

### VS Code

- **License:** Distributed Visual Studio Code is licensed under Microsoft's product license;
  the same license states its source code is available under MIT at the public repository.
  Source: <https://code.visualstudio.com/license>.
- **Platforms / price / model:** Platform matrix and marketplace/licensing model: **GAP** in
  this pass.
- **Target user:** Developer/editor users working in VS Code; broader segmentation is
  **GAP**.
- **Git engine:** **GAP** in this pass.
- **Commit/staging / partial staging:** Overview documents the Source Control view as central
  for staging/committing, staging by file/all changes, fine-grained staging of specific
  lines or selections from diff view, commit messages generated by AI based on staged
  changes, and remote sync status. Source:
  <https://code.visualstudio.com/docs/sourcecontrol/overview>.
- **Branch/history graph:** Overview documents a Source Control Graph representing commit
  history and branch relationships. Source:
  <https://code.visualstudio.com/docs/sourcecontrol/overview>.
- **Conflicts:** Merge-conflict docs describe inline CodeLens actions for current/incoming/
  both changes and comparisons, plus a three-way merge editor with Incoming, Current, and
  Result views; AI-assisted conflict resolution is documented as experimental. Source:
  <https://code.visualstudio.com/docs/sourcecontrol/merge-conflicts>.
- **Rebase/interactive history:** Rebase/cherry-pick scope is **GAP**, although conflict docs
  mention merge/rebase/pull/cherry-pick as operations that can pause on conflicts. Source:
  <https://code.visualstudio.com/docs/sourcecontrol/merge-conflicts>.
- **LFS/submodules/worktrees:** Branches/worktrees navigation is visible, but capability
  details are **GAP**; LFS/submodules are **GAP**. Source:
  <https://code.visualstudio.com/docs/sourcecontrol/overview>.
- **Integrations/AI:** Overview includes a “Collaborate on GitHub” section and AI-generated
  commit messages from staged changes. Extension/marketplace specifics are **GAP**. Source:
  <https://code.visualstudio.com/docs/sourcecontrol/overview>.
- **Accessibility:** **GAP** in this pass.
- **Collaboration/cloud:** GitHub collaboration section noted above; PR/review scope is
  **GAP**.
- **Differentiator:** Embedded source-control loop with graph, granular staging, guided
  three-way conflict resolution, experimental AI conflict help, and GitHub collaboration
  context. Sources above.

### JetBrains IDEs

- **Current evidence:** Captured IntelliJ IDEA “Git” help shell confirms the topic exists and
  was built on 2026-08-18, but rendered article content did not load in the saved capture.
  Source: <https://www.jetbrains.com/help/idea/using-git-integration.html>.
- All requested dimensions are otherwise **GAP**, including pricing and platform matrix.

### Visual Studio

- **License / price / model:** Visual Studio licensing/pricing is **GAP** in this pass.
- **Platforms / target user:** Windows Visual Studio developers; explicit platform/system
  matrix is **GAP**.
- **Git engine:** Documentation tells command-line users to install Git for Windows and calls
  it “not a Microsoft product”; it does not state the embedded engine. Source:
  <https://learn.microsoft.com/en-us/visualstudio/version-control/git-with-visual-studio?view=vs-2022>.
- **Commit/staging:** Overview covers clone/create repository and commit workflows through
  linked tasks. Granular staging detail is **GAP**. Source:
  <https://learn.microsoft.com/en-us/visualstudio/version-control/git-with-visual-studio?view=vs-2022>.
- **Branch/history graph:** The Git Repository window is a consolidated view of repository
  details including local/remote branches and commit history. Source URL above.
- **Conflicts:** Docs explain that Git halts a merge and enters conflicted state, then link
  dedicated resolve-conflict guidance. Source URL above.
- **Partial staging:** **GAP.**
- **Rebase/interactive history:** **GAP.**
- **LFS/submodules/worktrees:** **GAP.**
- **Integrations/AI:** Users can copy GitHub/Azure DevOps permalinks and search/link GitHub
  Issues and Azure DevOps work items. AI: **GAP**. Source URL above.
- **Accessibility:** **GAP.**
- **Collaboration/cloud:** Documentation recommends GitHub, supports adding GitHub/GitHub
  Enterprise accounts, and references GitHub/Azure DevOps links/issues/work items. Source
  URL above.
- **Differentiator:** Repository management embedded in the Windows IDE, with consolidated
  branches/history, provider permalinks, and issue/work-item linking. Source URL above.

### Xcode

All requested dimensions are **GAP**: no valid Apple source-control documentation or
licensing/pricing source was successfully captured in this session.

### Eclipse / EGit

All requested dimensions are **GAP**: `/egit/` returned a 404 capture; no current EGit/JGit
project or license evidence was captured.

## Compact comparison matrix

Only sourced cells are filled. A blank cell means evidence was unavailable in this pass;
it does not mean the feature is absent.

| Tool | Platforms | Price/model | Partial staging | Conflicts | Interactive history | LFS | Submodules | Worktrees | AI |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| GitHub Desktop | GAP | MIT dev project; distribution model GAP | GAP | GAP | Cherry-pick/squash/reorder via drag-and-drop | GAP | GAP | GAP | GAP |
| GitKraken |  |  |  |  |  |  |  |  |  |
| Sourcetree |  |  |  |  |  |  |  |  |  |
| Fork | macOS 10.11+; Windows 7+ | $59.99 after free evaluation | GAP | Helper/resolver | Visual interactive rebase | Listed | Listed | GAP | GAP |
| Tower | macOS; Windows | Basic $69/user/year; Pro $99/user/year | Files/hunks/lines | Conflict Wizard | Drag-and-drop rebase | Yes | Yes | Promoted | GAP |
| Sublime Merge |  |  |  |  |  |  |  |  |  |
| SmartGit | Windows/macOS/Linux | GAP | GAP | Conflict resolution | Undo/clean history advertised; exact scope GAP | Listed | Highlighted | GAP | GAP |
| lazygit |  |  |  |  |  |  |  |  |  |
| Magit | Emacs implied; exact matrix GAP | GAP | GAP | GAP | GAP | GAP | GAP | GAP | GAP |
| VS Code | GAP | Product license distinct from MIT source | Lines/selections/files | Inline actions + 3-way merge editor + experimental AI | GAP | GAP | GAP | Navigation visible; detail GAP | Commit messages + experimental conflict assist |
| JetBrains IDEs |  |  |  |  |  |  |  |  |  |
| Visual Studio | Windows implied; matrix GAP | GAP | GAP | Dedicated guidance linked | GAP | GAP | GAP | GAP | GAP |
| Xcode |  |  |  |  |  |  |  |  |  |
| Eclipse/EGit |  |  |  |  |  |  |  |  |  |

## Opportunities for TurboGit

1. **Dual-mode partial staging.** Expose obvious gutter/file affordances and keyboard verbs
   for hunk/line/selection staging, with clear index-state feedback and undo.
2. **Conflict cockpit.** Provide base/current/incoming/result context, per-conflict actions,
   remaining-count navigation, operation abort/continue, and explicit save/stage semantics.
   Treat AI as optional assistance after deterministic controls are trustworthy.
3. **Safe history mode.** Before rebase/cherry-pick/squash/reset, show affected commits,
   remote divergence, force-push implications, backup/abort path, and plain-Git equivalent.
4. **Transparent recovery.** Surface reflog-style recovery and operation journals without
   requiring users to memorize plumbing.
5. **Performance budget as product promise.** Define reproducible large-repo/large-diff
   benchmarks before claiming speed; report startup, status/log refresh, scroll jank, memory,
   and conflict-open latency.
6. **Accessibility-first Git UX.** Make diff/merge semantics machine-readable, provide
   complete shortcut coverage, preserve focus during conflict jumps, and test screen readers,
   scaling, contrast, and reduced motion early.
7. **Three-OS parity.** Avoid accidental Windows/macOS-only assumptions; verify filesystem,
   credential, LFS, subprocess, and UI behavior consistently.

## Limitations and follow-up validation

- **Incomplete source set:** GitKraken, Sourcetree, Sublime Merge, lazygit, JetBrains,
  Xcode, and Eclipse/EGit lack usable current evidence in this artifact.
- **No hands-on testing:** No application installation, timing benchmark, accessibility
  audit, or user interview was performed.
- **Vendor wording risk:** Feature chips and marketing summaries can compress behavior.
  Manual-level verification is required before competitive claims.
- **Version drift:** Prices/features may change; re-fetch each source immediately before
  publication or release planning.
- **Priority follow-ups:** official pricing/support matrices; release notes; Git-engine
  architecture; accessibility and platform-support statements; survey datasets; and
  moderated task studies for staging, conflict, recovery, and history rewriting.
