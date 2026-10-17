# Conventional commits, cocogitto as the one parser, and release notes from history

Three asks landed as one decision: commit messages the repo can lint, release notes
generated from those messages, and both of them true of a history that was only ever
conventional by habit. The habit was real — the recent log parses cleanly — but nothing
enforced it, no tag has ever been cut, `.github/` holds one CI workflow with no commit
gate, and the early subjects (`wip`, `icons`, `improve ui`, the `issue-N:` family)
predate the convention. Sixty-nine merge commits, five of them GitHub's
`Merge pull request #N`, sit in the same history. Making the asks true meant deciding
enforcement seams, tooling, and how much of the past to keep — decisions with
public-history blast radius, which is why they are recorded before any of it runs.

## Decision

**D1 — The spec is structural.** Every commit reachable from `main` parses as
`type(scope)!: subject` per the conventional-commit spec: one type verb, an optional
free-form scope, a subject. Free-form means free: the type vocabulary (`feat`, `fix`,
`refactor`, …) and the scope list are conventions, not an enforced enumeration — the
repo already uses multi-scopes (`refactor(ui,services)`), and a hardcoded list buys
lint friction, not quality. Both breaking notations are legal — `!` after type/scope as
shorthand, a `BREAKING CHANGE:` footer when a changelog-worthy description is needed.
**Rejected: subject rules** (lowercase, no trailing period, length caps). They match the
existing style anyway, and hard length limits breed `--no-verify`.

**D2 — One parser, three consumers: cocogitto's.** The commit-msg hook, the CI job, and
the future bump/notes path all parse with cocogitto's library (7.0.0, pinned in
`[workspace.dependencies]` with `default-features = false` — that drops the CLI's clap
tree). The hook consumes it through `commit-lint`, a bin-only workspace crate that
reads the message file and exits non-zero on a parse failure; contributors install
nothing and no `cog` binary needs to be on PATH.

*Rejected: a self-contained regex in the hook.* It is a complete implementation of a
purely structural spec and needs no dependency — it was the planning session's own
recommendation, revised when the dependency form was asked about. The regex's flaw is
not correctness but plurality: a second parser drifts from the one CI and the bump path
use, and the whole point of this decision is that the drift is impossible.

*Rejected: `cog verify` via a PATH-installed binary.* Same single-parser property, but
every fresh clone fails its first commit until someone runs
`cargo install cocogitto`.

*Cost accepted:* the first commit on a fresh clone cold-builds `commit-lint` plus
cocogitto's tree (git2 among it) — a minute or two, once per clone, warm thereafter.

**D3 — cargo-husky installs the hook; the hook checks one message.** cargo-husky is a
root dev-dependency in `user-hooks` mode (`default-features = false`): it becomes purely
the installer, copying a committed `.cargo-husky/hooks/commit-msg` script into
`.git/hooks` on the first workspace build. The script is one line:
`cargo run --quiet -p commit-lint -- "$1"`. Scope is `commit-msg` only — the message is
the unit the hook can see and the unit D1 governs.
**Rejected: a `pre-push` range check too.** CI is the range backstop; pre-push
re-linting adds seconds to every push for what the hook already prevents.

**D4 — CI lints ranges.** A `commits` job: on pull requests, the PR's commit range; on
pushes to `main`, the pushed range as a backstop. Direct pushes to `main` stay allowed —
the repo is effectively solo, and the hook is the primary gate either way. The job
needs `fetch-depth: 0` to see the range.

**D5 — Rebase-merge going forward; every main commit is a lint target.** PRs merge by
rebase, so what lands is what was written and linted, and the changelog generated from
main is clean by construction.
**Rejected: squash-merge** — it would make the PR title the only lint target and the
only changelog entry, erasing per-commit history the rebase policy keeps.
**Rejected: keeping merge commits as the merge mechanism** — the `Merge branch …`
subjects are noise every tool must then be taught to skip, and each branch commit needs
linting regardless. The sixty-nine merge commits already in history stay exactly where
they are; the tools skip them by default.

**D6 — Release notes are generated from history at release time, published on GitHub
releases only. There is no `CHANGELOG.md` and none will be created.** Section taxonomy
is the tool default: Breaking changes, Features, Bug fixes, Miscellaneous —
`refactor`, `test`, `docs`, `chore` fold into Miscellaneous.
**Rejected: a committed changelog file.** It duplicates the release page and needs a
second write path.
**Rejected: elevating `refactor` to its own section.** Defensible for this repo —
seventy refactor commits and counting — but the default taxonomy was chosen, and a
section that catches half the commits is not a summary.

**D7 — Versioning is single; releases are deferred.** The product has one version — the
root package's — and the eight crates ride along; none is published independently. The
release machinery (`cog bump --auto` computing semver from commits, a post-bump hook
updating the root `Cargo.toml`, a tag-push workflow building Linux and Windows binaries
and publishing the generated notes) is configured but dormant: with no tags, none of it
fires until the first tag is cut, and "no release for now" is the current answer. The
baseline question — tag the rewrite point as pre-history, or generate the first
release's notes over the entire fixed history — **re-opens at the first release** and
is deliberately not decided here.

**D8 — One-time history surgery: reword-only.** The roughly forty-five non-conventional
subjects are reworded to the spec through `git filter-repo --message-callback` with an
explicit old→new mapping table approved before it runs; authors and dates are
preserved; the merge commits, including the five GitHub PR merges, are kept. Rewriting
means a force-push to `main`: all clones and forks go stale, and the SHAs quoted in
PR discussions orphan. **Rejected: full linearization** — flattening the merge commits
costs the PR pages their commit links and rewrites ten times more history for nothing
the tools need. **Rejected: leave history untouched** — the rewrite is what makes the
whole history uniformly spec-clean, which is the premise the release-notes path leans
on whenever the deferred D7 question is answered with "all history".

## Status

Planned; nothing executed. The working plan lives in
`.scratch/conventional-commits/execution-plan.md`; the history rewrite carries two
hard gates — mapping-table approval, then the force-push itself.

## What this amends, and what it does not

- **Amends AGENTS.md's "Commit & Pull Request Guidelines"** (execution step): the loose
  "often prefixed as `docs:`" wording becomes the D1 rule, the rebase-merge policy, and
  the four quality gates gain a fifth — the commits CI job.
- **Amends the project-structure section of AGENTS.md** with a one-line note that
  `commit-lint` is tooling, not a domain layer — the `crates/*` glob pulls it into the
  workspace, and nothing in the layer stack may depend on it.
- **Touches no existing ADR.** No earlier decision concerns commits, tags or releases.
  The quality-gate list in AGENTS.md and the CI workflow are the only enforcement
  surfaces this extends.

## Consequences

- **The spec has one implementation, and it is in-tree.** Changing commit rules means
  changing `cog.toml` and recompiling the hook bin — not a wiki page three tools
  interpret independently.
- **A fresh clone's first commit is slow once.** Documented in the hook script's
  directory; not papered over with fallback logic that silently disables the gate.
- **Force-push to `main` is accepted exactly once**, for the reword. Ordinary pushes
  stay fast-forward; branch protection, if ever added, must not quietly forbid what D8
  already did.
- **Release notes list every commit** — `test:` and `docs:` commits included, folded
  into Miscellaneous. Rebase-merge keeps all of them on main, so a release note's
  length is the release's real size.
- **`cog.toml` becomes load-bearing config** for hook, CI and bump alike; it is where
  the D1 structure and the D6 sections live as tool settings.
- **The hook is an installer-plus-script, not magic.** cargo-husky acts on the first
  workspace build; a contributor who commits before building gets the pre-existing
  behaviour once and the gate from then on.
