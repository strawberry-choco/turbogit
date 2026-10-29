# Repository setup and onboarding

> Surface `onboarding`. Filled by the agent pass from installed-bundle evidence plus
> verified documentation. Bundle wins on names/ids/keystrokes; docs win on UX and
> flow. See `research/evidence/rebased/DISTILL.md`.

## Sources

- https://www.jetbrains.com/help/idea/set-up-a-git-repository.html, verified 200, 12612 chars → `docs-text/set-up-a-git-repository.md`
- https://www.jetbrains.com/help/idea/working-with-git-tutorial.html, verified 200, 15473 chars → `docs-text/working-with-git-tutorial.md`
- https://www.jetbrains.com/help/idea/advanced-settings.html, fetched 2026-10-06 → `docs-text/advanced-settings.md` (describes the Git advanced settings; states no defaults)
- `actions-labeled.tsv`
- `strings/GitBundle.properties`, the `init.*`, `clone.*`, `git.add.*` families
- `strings/VcsBundle.properties``get.from.version.control=Clone Repository`

Docs resolve 2/2. This is the **smallest action set in the whole surface set (2)**
and that is the finding, not a gap in the evidence: onboarding in this product is
almost entirely *not* a Git surface. It is the IDE's project wizard, the trust
prompt, the build-tool configuration, and the platform welcome screen, most of
which is IDEA inheritance that `DISTILL.md` rule 5 tells us to ignore.

What remains after that filter is genuinely small and can be fully specified.

## UI, what exists

### Action inventory (bundle)

2 actions harvested from the installed bundle.

| Action id | Label | Key |
| --- | --- | --- |
| `Git.Clone` | Clone… | — |
| `Git.Init` | Create Git Repository… | — |

**Two actions, and one of them is conditional on a setting the docs barely
mention.** From the bundle:

- `init.destination.directory.title=Create Git Repository` (GitBundle:100)
- `init.destination.directory.description=Select directory where the new Git repository will be created.` (99)
- `init.warning.title=Git Init` (103)
- `init.warning.already.under.git=The selected directory <tt>{0}</tt> is already under Git.` (101)
- `clone.dialog.checking.git.version=Checking Git version…` (892)
- `clone.dialog.shallow.clone=Shallow clone with a history truncated to` (893)
- `clone.dialog.shallow.clone.depth=commits` (894)
- `advanced.setting.git.clone.recurse.submodules=Recursively clone submodules in the project` (1700), whose description (1701) is unusually explicit: "When enabled, Checkout from Git calls `git clone --recurse-submodules`, i.e. clones the main repository with all submodules if there are any"

**`init.warning.already.under.git` is the one genuinely Git-specific guard on the
surface**: it fires when the chosen directory is already inside a repository, and
it is a *warning with a named title* (`Git Init`), not a silent nesting. That is
the one onboarding decision a Git client must get right, because `git init` inside a
repo is almost never what the user meant.

Also from the bundle, and clearly belonging here:

- `get.from.version.control=Clone Repository` (VcsBundle:915), the welcome-screen
  button
- `stage.default.status=Nothing to commit, working tree is clean` (GitBundle:1020)
  and `git.commit.nothing.to.commit.error.message=Nothing to commit` (581)
- `commit.dialog.no.changes.detected.title=Nothing to Commit` (VcsBundle:287)
- `configuration.project.not.trusted.label=Safe mode, Version Control is not available. Trust the project to access full IDE functionality.` (VcsBundle:127)
- `advanced.setting.git.recent.projects.show.branch=Show branch names in the list of recent projects`, description "Branch names will be shown on the Welcome screen and in the list of recent projects in the main menu" (1715-1716), with a three-value mode (1717-1719): `Never` / `Projects with identical names` / `All projects`

### Topology

**A. The Clone dialog** (set-up:26-32). Fields:

1. **URL of the remote repository**, "or select one of the VCS hosting services on
   the left" (line 26)
2. **Hosting-service completion**, "If you are already logged in to the selected
   hosting service, completion will suggest the list of available repositories that
   you can clone" (line 27). **The service picker is on the left, the URL on the
   right**, a two-column layout, not a single field with a dropdown.
3. **`Shallow clone with a history truncated to` + depth** (line 28;
   GitBundle:893-894), a checkbox and a number, with the unit in the *label*
   (`commits`), not a suffix on the field
4. Progress: `Checking Git version…` (GitBundle:892), **the dialog validates the
   environment before cloning**

**B. The Create Git Repository dialog** (set-up:63-65): a single directory
picker. "In the dialog that opens, specify the directory where a new Git
repository will be created" (line 65; GitBundle:99-100). One field.

**C. The welcome screen**. `Clone Repository` (VcsBundle:915; set-up:24: "If no
project is currently open, click `Clone Repository` on the Welcome screen"), plus
the recents list with optional branch names (GitBundle:1715-1719).

**D. The trust prompt**, `Trust and Open Project '<project_name>'?` with
`Trust Project` / `Preview in Safe Mode` (set-up:34), and the safe-mode label
explains the cost: "Safe mode, **Version Control is not available**. Trust the
project to access full IDE functionality." (VcsBundle:127).

### The four onboarding routes, and which are Git

The doc names four (set-up:13-77). Sorting them by whether they are Git-client
behaviour or IDE inheritance is the first act of judgement this surface requires:

| Route | Doc | Git or IDE? |
| --- | --- | --- |
| **Clone a remote** | set-up:13-32 | **Git**, two real fields and a guard |
| **Enable VCS on an existing project** | set-up:41-57 | **Git**, the whole-project case |
| **`VCS | Create Git Repository` on a subdirectory** | set-up:59-69 | **Git**, the multi-root case |
| *Trust prompt* | set-up:34 | IDE, but with a Git-visible consequence (VcsBundle:127) |
| *Build-tool configuration prompt* | set-up:36-39 | **IDE inheritance, excluded** per `DISTILL.md` rule 5 |
| *Share project settings files* | set-up:55 | **IDE inheritance, excluded** |
| *WSL2 git detection* | set-up:9-11 | Platform, excluded, but the *detection-failure* behaviour is Git |

**The multi-root route is the one most often overlooked.** "Associate different
directories within the project with different Git repositories … In the main menu,
go to `VCS | Create Git Repository`. In the dialog that opens, specify the directory
where a new Git repository will be created" (set-up:59-65), with a constraint worth
copying: "Git does not support external paths, so if you choose a directory that is
outside your project root, make sure that the folder where the repository is going
to be created **also contains the project root**" (line 66). And: "If you are
creating multiple Git repositories inside the project structure, **repeat the
previous steps for each directory**" (line 68), no batch affordance.

### State matrix

| State | Which region | What it shows | Source |
| --- | --- | --- | --- |
| default | Clone dialog | the default clone location; no shallow | set-up:26-28 |
| default | init dialog | the project root as the directory | set-up:65 |
| **progress** | Clone dialog | `Checking Git version…`, **before** any clone work (GitBundle:892) | GitBundle:892 |
| **warning** | init dialog | `The selected directory <tt>{0}</tt> is already under Git.` under title `Git Init` (GitBundle:101, 103) | GitBundle:101, 103 |
| disabled | `Enable Version Control Integration` | "**If there is no `Enable Version Control Integration` option available in the VCS Operations Popup, it means that Git version control is already enabled** for the project" (set-up:57), **absence is the "already done" signal** | set-up:57 |
| **safe mode** | whole product | "Safe mode, **Version Control is not available**. Trust the project to access full IDE functionality." (VcsBundle:127) | VcsBundle:127 |
| empty | commit area, clean tree | `Nothing to commit, working tree is clean` (GitBundle:1020) | GitBundle:1020 |
| empty | commit dialog | `Nothing to Commit` (VcsBundle:287) / `Nothing to commit` (GitBundle:581) | — |
| hover | recents row | branch name, if enabled (GitBundle:1715-1716) | GitBundle:1715-1716 |
| selected | hosting service | the repository list completes (set-up:27) | set-up:27 |
| progress | after init | "You will get a notification that the local Git repository has been created for your project" (tutorial:23) | tutorial:23 |
| progress | after clone | "Git root mapping will be automatically set to the project root directory" (set-up:31) | set-up:31 |

**Two states worth stealing outright.** First, `stage.default.status=Nothing to
commit, working tree is clean` (GitBundle:1020) is a *status-bar* statement, not
an empty panel, the clean tree is announced, not merely shown as absence. Second,
`Enable Version Control Integration` disappearing is the "already enabled" signal,
which avoids a redundant dialog entirely.

### What `Add to Git` does, the boundary between onboarding and the commit surface

The untracked-file path is documented in three places and is the hinge between this
surface and `01-commit-tool-window`:

- "When Git integration is enabled in your project, IntelliJ IDEA **suggests
  adding each newly created file under Git, even if it was added from outside
  IntelliJ IDEA**" (set-up:77). Bundle: `checkbox.including.files.created.outside.ide=Apply to files created outside {0}`.
- The suggestion is a **dialog with an `Add` button** (tutorial:42). "In the
  dialog that opens, click Add so that Git could start tracking the file".
- The accidental-dismissal recovery is documented: "If you accidentally close this
  dialog, you can add the file to Git in the Project tool window: right-click the
  file and select `Git | Add`" (tutorial:44).
- The ignore interaction: "you can ignore them [and] the IDE will not suggest
  adding them to Git and will **highlight them as ignored**" (set-up:82). Bundle:
  `advanced.setting.vcs.process.ignored=Highlight ignored files`.
- Force-adding an ignored file warns, and **Cancel is not neutral** (set-up:78; see
  `01-commit-tool-window`).

## UX, how it behaves

### How is it reached, and what hides behind a secondary affordance?

**Clone, four entries**, one per integration state (set-up:19-24):

| State | Route |
| --- | --- |
| VCS integration already enabled | `Git | Clone` |
| VCS integration not yet enabled | `VCS | Get from Version Control` |
| — | `File | New | Project from Version Control` |
| No project open | `Clone Repository` on the Welcome screen (VcsBundle:915) |

**That table *is* the onboarding logic**: the entry point moves depending on how
much setup is already done, and a client that only offers one of them is worse at
every state. TurboGit's `welcome.rs` and `recents.rs` own the no-project case.

**Init, two entries** (set-up:49-50, 63): `⌥\`` → VCS Operations Popup →
`Enable Version Control Integration…`, or `VCS | Create Git Repository` for a
subdirectory. The first is the whole-project case, the second the per-directory
case, and **the docs route them differently** (`⌥\`` popup vs main menu) because
they are different scopes.

Hidden behind secondary affordances:

- **The hosting-service picker is a left column**, not a dropdown (set-up:26), so
  account state is always visible, and the completion list is contextual.
- **Shallow clone hides behind one checkbox** (set-up:28).
- **Recursively clone submodules is a setting, not a checkbox in the dialog**
  (GitBundle:1700), **and it is described in git terms**: "Checkout from Git calls
  `git clone --recurse-submodules`" (1701). So the user is told the exact command.
- **The post-clone "create a project from these sources?" prompt** (set-up:31) is a
  separate dialog: "If you want to create a project based on the sources you have
  cloned, click `Yes` in the confirmation dialog", so clone and open are
  separable.
- **"If there is no `Enable Version Control Integration` option available … it
  means Git is already enabled"** (set-up:57), the affordance's *absence* is the
  state.

### Which verbs are destructive, and what do they confirm or preflight?

**Onboarding is almost entirely non-destructive, the verbs create.** That is the
finding, and it is why the surface is small.

| Verb | Destructive | Confirmation | Evidence |
| --- | --- | --- | --- |
| `Clone` | no, creates a new directory | no; but a follow-up "create a project?" prompt | set-up:31 |
| `Enable Version Control Integration` | no | no | set-up:52-53 |
| `Create Git Repository` on a subdirectory | no | **Yes, if already under Git**: `The selected directory {0} is already under Git.` (GitBundle:101) | GitBundle:101 |
| `Add` an untracked file | no | a suggestion dialog with `Add` (tutorial:42) | tutorial:42 |
| Force-add an ignored file | no | **Yes**, and Cancel is partially non-neutral (set-up:78) | set-up:78 |
| **Share project settings files** | no | a prompt: "IntelliJ IDEA will ask you whether you want to share project settings files via VCS" (set-up:55) | set-up:55 |
| Trust the project | **yes, in effect**, it grants VCS access | **Yes**, `Trust Project` / `Preview in Safe Mode` (set-up:34) | set-up:34 |
| Unshallow (later) | **yes**, see `08-remotes-sync` | **UNVERIFIED**, re-tested 2026-10-06: set-up:29 is still the only mention in the doc set; no page documents a confirmation → R3 | set-up:29 |

**The trust prompt is the one destructive verb here, and it is destructive by
omission**: refusing it removes Version Control entirely (VcsBundle:127). **The safe
choice's cost is stated in the safe choice's own label.** That is a small,
excellent piece of copy design.

**And the already-under-git guard is the one Git-specific preflight**, it fires
on the exact condition that produces a nested, confusing repository.

### What can be undone, from where, and for how long?

**Very little, and the doc is refreshingly honest about it.**

- **Clone**: undoable only by deleting the directory. No undo is offered, and none
  is described. **UNVERIFIED** whether a notification offers a cleanup.
  Re-tested 2026-10-06: set-up and working-with-git-tutorial never describe the
  post-clone notification's contents → needs runtime capture (R3).
- **`git init`**: undoable by deleting `.git`. Not offered.
- **`Add` an untracked file**: undoable via `Rollback`, "If a file is added to Git
  but not committed, you can right-click it in the Commit window and choose
  `Rollback`" (set-up:83). **The distinction is exactly at *committed*: before
  commit, Rollback; after commit, a revert commit.**
- **Force-add an ignored file**: likewise `Rollback` while uncommitted.
- **A refused trust prompt**: fully reversible, trust later. The cost is having no
  VCS meanwhile.
- **The `Add` suggestion itself**: dismissing the dialog loses nothing, because
  `Git | Add` remains in the context menu (tutorial:44). **The dismissal is
  non-destructive by construction.**

**The pattern: everything before the first commit is reversible by a visible
route, and nothing after it claims to be.** That boundary is the same one the
commit surface draws, and stating it is more useful than adding an undo stack.

### Which options are on by default, and what is remembered between uses?

| Option | Default | Evidence |
| --- | --- | --- |
| Shallow clone | **off**; with `[N] commits` as the depth unit (set-up:28; GitBundle:893-894) | set-up:28 |
| Recursively clone submodules | **on by documented behaviour**, the clone flow states unconditionally "If your project contains submodules, they will also be cloned and automatically registered as project roots" (set-up:32); advanced-settings describes the toggle with no stated default, so the checkbox state itself remains R3 | set-up:32; GitBundle:1700; docs: advanced-settings |
| Show branch names in recents | a setting with a three-value mode: `Never` / `Projects with identical names` / `All projects` (GitBundle:1715-1719) | GitBundle:1715-1719 |
| Suggest adding newly created files | **on**. "IntelliJ IDEA **suggests** adding each newly created file under Git" (set-up:77) | set-up:77 |
| Apply the suggestion to files created **outside** the IDE | a checkbox, implying a narrower default (VcsBundle `checkbox.including.files.created.outside.ide`) | set-up:77 |
| Highlight ignored files | a setting (VcsBundle `advanced.setting.vcs.process.ignored`) | set-up:82 |
| When files are created | `Ask` / `Add silently` / `Do not add`, **a three-way policy** (VcsBundle `radio.after.creation.*`) | VcsBundle |
| Remote name after clone | **`origin`**, "The default name Git gives to the remote you've cloned from is `origin`" (set-up:109) | set-up:109 |
| Trust on open | **prompt**, not assumed (set-up:34) | set-up:34 |
| `Update Info` auto-open | on (sync:78) | see `08-remotes-sync` |

**`radio.after.creation` is the fourth instance of the ask/silently/nothing
triple**, after changelist removal, file deletion, and patch creation. At this
point the convention is unmistakable: **Rebased models "when do I ask about this
class of change?" as a per-class three-way setting.** That is the single most
transferable *pattern* in this whole spec, and it is more valuable than any
individual feature.

Remembered between uses:

- **`git config --local commit.template`** persists and pre-fills the message
  (commit-and-push-changes.md:33), a git-config mechanism, not an app setting.
- **Git identity persists in `.git/config`** once entered (tutorial:65).
- **The clone location persists** as a recents entry, with an optional branch name
  (GitBundle:1715-1716).
- **Nothing about the init dialog persists**, it is a one field, one action.

### What feedback appears, and where does focus land?

Progress and success:

- `Checking Git version…` **in the dialog, before cloning** (GitBundle:892)
- "You will get a **notification that the local Git repository has been created**
  for your project" (tutorial:23)
- "**Git root mapping will be automatically set to the project root directory**"
  (set-up:31), an explicit statement of what happened to the mapping
- "If your project contains **submodules, they will also be cloned and
  automatically registered as project roots**" (set-up:32), the multi-root
  consequence of a clone, stated up front
- "**Now that you have created a new project with a Git repository, dedicated tool
  windows for working with Git become available**" (tutorial:24), the surface
  reveals itself only after init

**That last one is the onboarding design in a single sentence: the Git surfaces do
not exist before init.** Not empty, *absent*. TurboGit's `Tab` enum is
unconditional, which is a genuine difference; see divergences.

Errors and refusals:

- `The selected directory {0} is already under Git.` (GitBundle:101)
- **Git-not-found**: "IntelliJ IDEA automatically detects if Git is installed on
  your computer. **If the IDE can't locate a Git executable, it suggests downloading
  it**" (tutorial:22; set-up:8), **the missing-dependency path is an offer, not an
  error**
- Safe mode: VcsBundle:127
- Nothing to commit: GitBundle:1020, 581; VcsBundle:287

**Optimistic paint: no**, the notification is the mechanism (tutorial:23).

Focus after the action:

- After init, the Git tool windows appear (tutorial:24). Where focus lands is
  **UNVERIFIED**, re-tested 2026-10-06: the docs name only the tool windows
  appearing (tutorial:24), never focus → runtime-only (R3).
- After clone, the "create a project?" prompt decides whether a project opens at
  all (set-up:31).
- The `Add` dialog's focus. **UNVERIFIED**; tutorial:44 implies the recovery route
  assumes the user is in the Project view, so the dialog likely takes focus.
  Re-tested 2026-10-06: no page describes the Add dialog's layout or focus →
  runtime-only (R3).

## Flow, how tasks chain

### Entry points

1. Welcome screen → `Clone Repository` (VcsBundle:915; set-up:24).
2. `Git | Clone…` (integration on), `VCS | Get from Version Control` (off), or
   `File | New | Project from Version Control` (set-up:19-22).
3. `⌥\`` → VCS Operations Popup → `Enable Version Control Integration…`
   (set-up:49).
4. `VCS | Create Git Repository` (set-up:63).
5. **The new-project wizard's `Create Git repository` option** (tutorial:19), the
   IDE's own wizard, IDE inheritance, but it is the route the tutorial uses.
6. Right-click a file → `Git | Add` (tutorial:44; set-up:76).

### Internal sequence, with dialog chaining

**Clone, the reference onboarding flow:**

```
Welcome screen (or Git | Clone, or File | New | Project from Version Control)
  → Clone Repository dialog                                           [dialog]
      [hosting services | URL]        ← two columns; services on the left
      [ ] Shallow clone with a history truncated to  [N] commits
      Checking Git version…            ← environment check happens HERE
  → Clone
  → (submodules) cloned and registered as project roots automatically
  → "Git root mapping will be automatically set to the project root directory"
  → "If you want to create a project based on the sources you have cloned,
     click Yes in the confirmation dialog"                            [dialog]
  → (first time only) build-tool configuration prompt                 [chained, IDE]
  → (first time only) trust prompt: Trust Project | Preview in Safe Mode  [chained]
  → the Git tool windows become available
```

(set-up:26-34; tutorial:19-24.) **The trust prompt is last, after the work is
done**, which is the correct order, and the reason the prompt can be safe: the
clone has already happened, the user just chooses whether to run hooks from it.

**Init, two scopes, one dialog each:**

```
whole project:
  ⌥` → VCS Operations Popup → Enable Version Control Integration…
  → choose Git → OK
  → "The entire project will then be associated with a single Git directory"
  → "IntelliJ IDEA will ask you whether you want to share project settings files
     via VCS"                                                       [IDE, excluded]
  → notification: the local Git repository has been created
  → the Git tool windows become available

one subdirectory (multi-root):
  VCS | Create Git Repository
  → directory picker                                                  [dialog]
      (warns if the directory is already under Git)                  [guard]
  → repeated for each directory — no batch affordance
```

(set-up:41-69.) Note that the whole-project route **has no directory dialog at
all**, it infers the project root, while the subdirectory route does. And the
external-path constraint (set-up:66) is stated as prose with no bundle string,
which suggests it may be a warning or may be documentation-only. **UNVERIFIED.**
Re-tested 2026-10-06: the constraint appears only as prose in set-up:66; no
bundle string and no other page mentions it → enforcement is runtime-only (R3).

**Add a file to Git, the hinge into the commit surface:**

```
a file is created (inside or outside the IDE)
  → "IntelliJ IDEA suggests adding each newly created file under Git"
  → Add dialog                                                       [dialog]
        (a = Add, or Cancel)
  → the file appears under the Changes changelist in the Commit tool window
  → Git tool windows handle the rest
```

(tutorial:40-51; set-up:77.) **Dismissed by accident?** "right-click the file in
the Project tool window and select `Git | Add`" (tutorial:44).

**Ignore instead of add:**

```
right-click an untracked file or folder
  → Git | Add to .gitignore      → "Add selected files to {0}"     (VcsBundle)
  → or Git | Add to .git/info/exclude
→ the file is no longer suggested, and is highlighted as ignored
→ optionally: Changes view toolbar → Show Ignored Files
```

(set-up:101-104; bundle `vcs.add.to.ignore.file.action.group.text=Add to {0}`,
`git.add.to.exclude.file.action.text=.git/info/exclude`.) **Two ignore files,
distinguished in the menu by name**, because one is shared and one is local, and
the doc says so (set-up:84-90).

**Force-add an ignored file, the partially-cancelling dialog:**

```
try to add a .gitignore'd file
  → IntelliJ IDEA will suggest force-adding it
  → Cancel cancels ONLY the force-add; "all other files will be added"
```

(set-up:78.) **A batch action with a per-item veto**, the most surprising
behaviour on this surface, and the one most likely to be mis-implemented.

**Shallow clone, completed later:**

```
Clone dialog → [x] Shallow clone … [N] commits
  → …work…
  → Git | Unshallow Repository        ← the other half, on another surface
```

(set-up:28-29; `Git.Unshallow` in `actions-labeled.tsv`.)

### What happens next on success

- **Init**: a notification confirms it (tutorial:23) and **the Git tool windows
  become available** (tutorial:24). The product's surface area grows as a direct
  consequence of the action.
- **Clone**: root mapping is set automatically, submodules are registered as roots
  (set-up:31-32), and a prompt offers to open the result as a project (line 31).
- **Add**: the file is tracked and appears in the `Changes` changelist
  (tutorial:51). Then ordinary commit work.
- **Ignore**: the file stops being suggested and is highlighted as ignored
  (set-up:82).

### What happens next on failure, and how the user recovers

| Failure | Evidence | Recovery |
| --- | --- | --- |
| **Git is not installed** | "If the IDE can't locate a Git executable, **it suggests downloading it**" (tutorial:22; set-up:8) | download, **an offer, not an error** |
| Directory already under Git | `The selected directory <tt>{0}</tt> is already under Git.` under title `Git Init` (GitBundle:101, 103) | choose a different directory. **UNVERIFIED** whether the dialog blocks or merely warns, re-tested 2026-10-06: no page states the dialog's severity → R3 |
| VCS already enabled | the action is **absent** from the popup (set-up:57) | nothing to do, absence is the signal |
| Refused trust | safe mode: "Version Control is not available" (VcsBundle:127) | trust later; fully reversible |
| No repositories listed for a hosting service | — | **UNVERIFIED** — no empty state specified for the service's repository list. Re-tested 2026-10-06: set-up:27 is the only page describing the service completion; no empty state documented → R3 |
| Clone target directory not empty | — | **UNVERIFIED** — no string found. Re-tested 2026-10-06: official docs silent; JetBrains support forums report a "Directory already exists and is not empty" clone failure, but forum posts are not docs-grade → R3 |
| `Add` dialog dismissed by accident | tutorial:44 | `Git | Add` from the context menu |
| Cancel on force-add | set-up:78 | only the force-add is cancelled; the rest proceeds |
| Nothing to commit | `Nothing to commit, working tree is clean` (GitBundle:1020) |, a status, not an error |
| Clone completed but the project looks wrong | "create a project based on the sources … click `Yes`" (set-up:31) | decline, and the clone still exists |

**The two Git-specific failure paths are both offers, not errors**, download the
executable, or add the file another way. And the "already enabled" case avoids a
dialog entirely. **Onboarding's failure design is to redirect, not to report.**

### Escape / dismiss semantics

- **The `Add` dialog is dismissible with no loss**, because `Git | Add` remains in
  the context menu (tutorial:44). **A dismissal is a deferral, not a decision.**
- **`Preview in Safe Mode` is the safe half of the trust prompt**, and its cost is
  in its own label: no Version Control (VcsBundle:127).
- **The clone's "create a project?" prompt is declineable** and the clone still
  stands (set-up:31). **The work is done; only the next step is optional.**
- **`Enable Version Control Integration` disappearing is the whole escape** from
  re-init (set-up:57), no "already initialised" error.
- **The force-add dialog's Cancel is the least escapable thing here**, because it
  does not cancel the batch (set-up:78). Worth designing against.

## Divergences (intentional)

| Dimension | Rebased behaviour | TurboGit behaviour | Why |
| --- | --- | --- | --- |
| UI | The Git surfaces **do not exist before init**: "Now that you have created a new project with a Git repository, dedicated tool windows for working with Git become available" (tutorial:24) | `Tab::{Commit, Log, Branches, Worktrees, Submodules}` is unconditional; `welcome.rs` shows when no project is open | Deliberate departure. A tab strip that always exists is not confusing, the states are mutually exclusive, so the tabs are never *wrong*, only absent-in-Rebased. Revealing them progressively would add a state machine to the shell for no user benefit. |
| UI | Clone dialog is **two columns**, hosting services left, URL right (set-up:26) | **UNVERIFIED whether a clone dialog exists** | **Open gap.** The service picker being always-visible (rather than a dropdown) is what makes account state legible. If TurboGit has no clone dialog, this is the whole feature. See ticket 19. |
| UX | The environment check happens **inside the clone dialog**: `Checking Git version…` before any work (GitBundle:892) | **UNVERIFIED** | **Open gap, high value and very cheap.** A missing/slow git binary is the single most common first-run failure, and checking it in the dialog where the user is about to type a URL is the right place. One call, one status line. |
| UX | `init.warning.already.under.git`, a titled warning when the target directory is already inside a repository (GitBundle:101, 103) | **UNVERIFIED** | **Open gap, and it is the one genuinely Git-specific preflight on this surface.** A nested `git init` is almost never intended and is confusing to undo. |
| UX | Entry points **move with setup state**, four clone routes and two init routes, chosen by what is already configured (set-up:19-24, 49-50, 63) | `welcome.rs` + `popups.rs` workspace picker; **UNVERIFIED** whether the routes are state-dependent | **Partial gap, and the principle matters more than the routes.** Offering `Clone` to a user who has not enabled VCS yet, or hiding it from one who has, is the difference between an onboarding flow and a menu. |
| UX | `Enable Version Control Integration` **disappears** when Git is already enabled (set-up:57) | **UNVERIFIED** | **Open gap, one line of logic.** Encoding "already done" as *absence of the affordance* is cheaper than an already-initialised error. |
| UX | "If the IDE can't locate a Git executable, **it suggests downloading it**" (tutorial:22) | **UNVERIFIED** | **Open gap, small.** A missing dependency is an *offer*, not an error, and in a Rust client that shells out to `git`, the check is one `which git`. |
| UX | `stage.default.status=Nothing to commit, working tree is clean`, the clean tree is **announced in the status bar**, not merely shown as an empty panel (GitBundle:1020) | Not established | **Open gap, small and high-value.** The difference between "an empty list" and "Nothing to commit, working tree is clean" is the difference between ambiguity and information. |
| UX | Force-add of an ignored file prompts, and **Cancel only cancels that sub-action** (set-up:78) | `Unversioned Files` bucket stages directly; no force-add affordance | **Deliberate, and recorded as a trap.** If TurboGit adds force-add, the partially-cancelling semantics must be specified deliberately rather than inherited by accident. |
| UX | Ask/silently/nothing is a **per-class three-way policy**, at least four instances: file creation, file deletion, changelist removal, patch creation (VcsBundle `radio.after.creation.*`, `radio.after.deletion.*`, `remove.changelist.combobox.*`, GitBundle:132-135) | `PendingConfirm` is a 10-variant enum with per-case confirmation | **Deliberate improvement, stated as such.** TurboGit's `PendingConfirm` already *is* this pattern, just without the "do it silently" third state. Adding a per-class `Never / Always / Ask` alongside each `PendingConfirm` variant would generalise a decision the product has already made structurally. The strongest transferable pattern in this spec. |
| UX | `Show branch names in the list of recent projects` with a three-value mode. `Never` / `Projects with identical names` / `All projects` (GitBundle:1715-1719) | `recents.rs` + `open_recent.rs` exist; branch display **UNVERIFIED** | **Open gap, small, and the middle value is the interesting one.** "Only when project names collide" is a *conditional* disclosure that most clients would not think of: a branch name is only useful when it disambiguates. |
| UI | WSL2 detection and `\\wsl$` path handling (set-up:9-11) | macOS/Linux client; no WSL concept | **Intentionally absent**, platform-specific and irrelevant to the target. Recorded so the omission is understood as scoping, not oversight. |
| UI | Trust prompt, build-tool configuration prompt, share-project-settings prompt (set-up:34, 36-39, 55) | None | **Intentionally absent per `DISTILL.md` rule 5**. IDE inheritance, not Git-client behaviour. The one part worth keeping is the *shape*: VcsBundle:127 proves the safe choice's label states its own cost ("Safe mode, Version Control is not available"). That copy pattern is transferable even where the prompt is not. |
| UX | Shallow clone with a depth in `commits` (set-up:28) and `Git | Unshallow Repository` as its later half (set-up:29) | `Git.Unshallow` exists in the bundle; **UNVERIFIED** in TurboGit | **Open gap, small and well-specified**, the two halves of one journey, and the label/depth-field layout is known. |
| UX | The `Add`-newly-created-file suggestion fires **even for files created outside the IDE**, with a narrower checkbox for that case (set-up:77) | **UNVERIFIED** | **Open gap, and the external case is the valuable one.** A file created in a terminal is invisible to an IDE that only watches its own editor, so this is how a non-IDE user gets their file tracked at all. |

**Re-tested 2026-10-06:** the `UNVERIFIED` entries in the TurboGit column above are
questions about TurboGit's own code (clone dialog existence, state-dependent
entry routes, the already-under-git guard, the git-download offer, recents branch
display, Unshallow, the external-file Add suggestion), no JetBrains page can
close them; route them to a TurboGit codebase check, not R3.

## Needs runtime capture

1. **Whether `init.warning.already.under.git` blocks or merely warns**, and what
   the dialog offers as a consequence. This is the one Git-specific preflight on
   the surface and its severity is unknown.
2. **Whether a clone dialog exists in TurboGit at all**, and if so whether it has a
   hosting-service column. The two-column layout is the documented shape
   (set-up:26).
3. **The empty state of a hosting service's repository list**, no string was found
   for "no repositories", only the search strings in `GitBundle` for the
   add-to-remote-branch picker.
4. **The checkbox state of `Recursively clone submodules`** (GitBundle:1700),
   the *behaviour* is documented as unconditional (set-up:32), but the toggle's
   default is stated nowhere (docs: advanced-settings).
5. **What `Clone Repository` on the Welcome screen looks like**, the button string
   exists (VcsBundle:915) but the screen's composition does not, and it is the
   first thing a new user sees.
6. **The external-path constraint on init** (set-up:66), whether it is enforced
   with a warning or is documentation-only. No bundle string was found for it.
7. **Whether a clone into a non-empty directory is refused, and with what message.**
   No evidence either way, recorded as a capture item because it is the most
   likely first-run mistake.
