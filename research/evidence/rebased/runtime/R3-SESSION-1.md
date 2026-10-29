# R3 session 1, the `idea.log` layer, captured 2026-10-06

The plan's §4 risk said "check the debug-log categories in the first ten minutes;
if it does not work, drop it." **It works.** The release build (1.1.19) honours
`-Didea.log.debug.categories`, so the behaviour layer at the `GitExecutor`
boundary is obtainable without any GUI interaction.

## Launch method (reproducible)

Clean config, nothing under the user's real `~/Library` is touched:

```
/Applications/Rebased.app/Contents/MacOS/rebased \
  -Didea.config.path=/tmp/rebased-r3/config \
  -Didea.system.path=/tmp/rebased-r3/system \
  -Didea.log.path=/tmp/rebased-r3/log \
  -Didea.log.debug.categories="#git4idea,#git" \
  -Didea.log.trace.categories="#git4idea,#git" \
  /tmp/tg-r3-fixture
```

`idea.log` confirms the categories took: `LogLevelConfigurationManager - Level
TRACE is set for the following category: #git4idea` (and `#git`). Log copy:
`idea-log-session1.log` (this directory, 336 lines).

## Fixture

`/tmp/tg-r3-fixture`, built to `acceptance_matrix.rs::seeded_repo()`'s shape:
`main` + `feature/one`, tag `v1.0`, remote-tracking refs under a fake
`https://example.com/fixture.git` remote, one dirty file (`README.md`).

## What project open alone invokes (before any user interaction)

Every line below is `GitHandler` output from the log, the reference's own
startup behaviour at the git boundary:

| Purpose (inferred) | Invocation |
|---|---|
| version check | `git -c core.quotepath=false -c log.showSignature=false version` |
| **identity probe** | `config --null --get user.name` / `user.email`, run at project open, not at first commit |
| encoding probes | `config --get i18n.logoutputencoding` / `i18n.commitencoding` |
| safety probes | `config --get commit.gpgSign`, `config --get credential.helper` |
| full config read | `config --null -l` |
| branch/remote scan | `for-each-ref refs/heads/** refs/remotes/** --no-color --format=%(refname) %(objectname) %(HEAD)` |
| **Recent branches** | `reflog --max-count 50 --grep-reflog "checkout: --"`, **this is the mechanism behind the Recent node** (spec §4.2/Ticket 03); capped at 50 reflog entries, checkout entries only |
| worktree discovery | `worktree list --porcelain` (with `credential.helper=` explicitly cleared) |
| log load, **two passes** | `log HEAD --branches --remotes --max-count=2000` then `--max-count=200`, a wide first fill, then a narrow refresh, both with a full `--pretty=format` field contract |
| status, two variants | `status --porcelain -z --untracked-files=no --ignored=no -- .` and `--no-renames --untracked-files=all --ignored=matching --` |
| **stash scan at startup** | `stash list --pretty=format:%H:%P:%at:%gd:%s` |
| .idea inspection | `ls-files -s -- .idea`, `ls-files --exclude-standard --others -- …/vcs.xml` |
| **auto-fetch on open** | `fetch origin --recurse-submodules=no --progress --prune`, confirms fetch-on-open and that `--prune` is on by default |

**Behaviours, not just commands:**

- **Failed fetch degrades, does not fail.** Against the fake remote the fetch
  died (`fatal: unable to access …`) and the app's response was to re-emit the
  incoming/outgoing widget state (`GitIncomingOutgoingStateApiImpl … Sending new
  state`) and carry on. No error dialog in the log, no retry storm.
- The app wrote `.idea/vcs.xml` + module files into the fixture repo on open,
  the "team-shared VCS mapping" mechanism §5.6 cites, observed live.
- Default `-c` wrapper on *every* invocation: `core.quotepath=false`,
  `log.showSignature=false`; `credential.helper=` added only where auth is
  irrelevant.

## D8, capture attempts recorded, including failures

| Attempt | Result | What would retry it |
|---|---|---|
| AX tree via osascript (`System Events`) | **failed**. `osascript is not allowed assistive access` (-25211) | Grant Accessibility to the host app (System Settings → Privacy & Security → Accessibility), then re-run; no relaunch of Rebased needed |
| Screenshot via `screencapture -x` | **failed**. `could not create image from display` | Grant Screen Recording to the host app |
| Debug-log categories in release build | **worked** — this session | — |
| Fixture repo seeding | **worked** | — |
| Project open + startup log capture | **worked** | — |

Still open and needing the GUI (Accessibility grant): menu ordering (D6),
settings-default screenshots (D7 checkbox states), wizard/first-run screens,
widget hierarchy. **The app is left running** (`/tmp/rebased-r3` config) so a
post-grant session can attach to it and continue; the fixture is in place.

## Parity leads worth tickets

1. **Recent-branches implementation**: reflog-derived, ≤50 checkout events. If
   TurboGit builds the Recent node (Ticket 03), matching the reference's
   mechanism (not just its ≤5 display cap) makes recents survive restarts and
   match user expectation after external checkouts.
2. **Two-pass log load** (2000 then 200), a loading strategy TurboGit's keyed
   reads could mirror for large repos.
3. **Identity probed at open**, not at first commit, §5.3's identity-prompt row
   gains a timing fact.
