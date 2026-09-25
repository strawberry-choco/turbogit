# The Git engine answers with domain values, over one honest adapter

`ADR-0001` set the seam in the right place and described it wrongly. This record
replaces its description — and the direction that follows from the description
being wrong. The interface itself stays where 0001 put it: one trait, no
façade, services and UI dependent on the port only.

## Census, measured 2026-09-23 against `main`

Numbers are a snapshot; the conclusions below are the durable part.

- **Four implementations of the port, not two.** The CLI adapter, the composed
  libgit2-over-CLI adapter, the in-memory fake, and a hand-written forwarding
  adapter in `test-support` that exists so integration suites can observe calls.
- **Two of the three advertised backends construct a byte-identical object.**
  `build_executor` maps `Libgit2` and `Auto` to the same composed executor, so
  the Settings row's middle and third options selected nothing between them.
- **The composed adapter is a delegation wrapper, not an in-process engine.** It
  implements 66 of the port's methods and delegates **41** of them to
  `self.cli` — measured after ticket `00`; the review that opened this branch
  counted 39 of 68, and both totals have since moved.
- **The fake answers 5 of the port's 22 typed reads**, stubs 13 with a constant,
  and leaves 4 to the port's "engines that cannot answer" default. It records 9
  of the 41 mutations despite a header claiming it records every one. Ticket `00`
  has since made `config_get` a sixth seeded answer.
- **Nine `run_raw` production call sites**, seven in services and two in the app
  layer, none in presentation code.
- **One diff text format, thirteen parsers of it**, in seven modules:
  `services/src/diff_engine.rs`, `services/src/hunk_stats.rs`,
  `services/src/partial.rs`, `services/src/cherry_across.rs`,
  `app/src/diff_model.rs`, `app/src/diff_load.rs`,
  `ui/src/ui/cherry_across.rs`. Five independent `@@` header parsers plus a
  paint-time re-parse, four `diff --git` section scanners, four old/new path
  splitters. Three writers of the syntax: two producers plus a header rewriter.
  Only one of the four splitters handles git's quoted form for paths containing
  spaces, so the duplication is a latent bug, not merely a smell.

Tickets `00` through `12` moved the figures again, and the measurements above are
the 2026-09-23 snapshot they were written from. The port is now **76 methods**:
69 at the branch's start, 67 after `00` deleted the two dead ones, and seven
raised reads since. Of the nine raw sites, `00` raised one — the signing-key
`git config --get`, which the port already answered — `06` raised the numstat
compare, and `07` raised six more: the app layer's two (`rev-list --count` behind
a delete confirmation, `rev-parse` for its undo sha) and services' four
reference-transaction calls behind the rebase backup. What remains is the one this
record called permanent, and `tests/port_discipline.rs` fails if a second
appears. The diff text has one reader (`engine/src/patch.rs`) and one writer, the
value's own rendering in the domain.

## Decision

Raise the **Git engine**'s interface from git's text dialects to domain values,
and retire the adapters that exist only because it was low.

1. **Move the seam up, from git's text to the domain.** The engine answers with a
   push plan, conflict versions, change stats, a patch value — not stdout for
   services to substring. A caller should not be able to write a revspec string,
   an index stage rev, or a `@@` regex against this interface.
2. **Retire the nominal second adapter rather than make it real.** One honest CLI
   adapter plus one substitutable adapter beats a decorator that exists to answer
   one flag. The Settings row keeps two options, and the surviving one is named
   for the composed adapter it actually selects.
3. **Give services one adapter that answers reads**, fixture-backed per repository
   shape, so their suites stop requiring a `git` binary. Raising the interface is
   what makes a canned *value* possible where canned text used to be.
4. **Delete the fourth implementation** once the fake and the substitutable
   adapter are one thing.

## Rules this record settles

**A single caller is a smell only about shape, not count.** One method, one
call site is unremarkable when the method is an already-typed domain read —
`blame`, `worktree_dirty`. It is a smell when the method's shape is git's: argv
in, a text format out. Under that rule the survivors stay and the text-shaped
ones get raised; none gets deleted, because deleting a read pushes its parsing
up into a caller, which is the opposite of this decision.

**A read a fixture can answer is a value the substitutable adapter supplies; a
mutation whose only real test is git keeps a git-backed suite.** A recorded call
is not evidence that git moved. Rejected: an in-memory git — a project, not a
ticket.

**One raw escape is designed, the rest are accidents.** Exactly one thing runs
arbitrary git: the fleet's user-typed custom command, whose feature genuinely is
"run arbitrary git". The other eight raw sites each ask one typed question, and
each is raised by its own ticket rather than wrapped.

## What the patch value does with git's syntax

Reading and writing unified diff are separated on purpose, and the split follows
the one honest adapter rather than the layer boundary:

- **The reader is the engine's.** `engine/src/patch.rs` is the only code in the
  workspace that interprets git's diff syntax. The CLI adapter answers
  `diff_patch` with it; the libgit2 adapter builds the same value from patches it
  never renders as text. Nothing above the port tests a line prefix.
- **The writer is the value's.** `git apply` is fed bytes, and the diff pane
  paints git's header lines as labels, so the value renders itself
  (`Patch`'s `Display`, `PatchFile::section_header_line`,
  `PatchHunk::header_line`). It lives in the domain because both adapters and
  every upstream caller need the same spelling; a second emitter in a upper layer
  is what thirteen parsers used to be.

Four conventions the text format implied and the value now states, each with a
named test in `tests/patch_value.rs`:

| Convention | Where the fact lives |
|---|---|
| git's C-style quoted path (`"a/we\tx"`) | resolved once by the reader; `old_path`/`new_path` and the source pair hold plain names, and quoting is re-derived on write |
| `\ No newline at end of file` | `PatchLine::no_newline`, a property of the line it follows — never a line of the patch |
| mode-only change | `PatchFile::mode_only()`, distinct from "no hunks", which is also what a rename and a **Binary change** look like |
| rename without a content change | a section with `RenameFrom`/`RenameTo` and no hunks — a whole answer, not an absence |

The corpus assertion is byte-exact: for every shape, `parse(text).to_string()`
*is* the text it was read from, including real `git diff` output over a fixture
repository with an edit, a rename of a path containing a space, a binary change
and a mode change. The value is not lossy in the direction that matters.

**One convention is normalized rather than represented.** A combined section —
git's `diff --cc` view of an unmerged path — carries one range per parent on its
`@@@` headers. The value has two sides, so it keeps the first parent's range and
the new side's, and says it was combined (`PatchFile::combined`), which renders
as an ordinary `@@` header. The pane's hunk label for a conflicted file therefore
changes wording, `@@ -1 +1,5 @@` where git wrote `@@@ -1,1 -1,1 +1,5 @@@`; that
section was never parsed before, and painted only because `@@@` happens to start
with the prefix the old row builder tested. No caller can be harmed by the
normalization: staging refuses a conflicted file, so a combined patch is never fed
back to `git apply`, and the body lines keep their two-column prefixes as
content. If a merge-conflict view ever needs each parent's span, the fact to add
is a list of ranges on the hunk, not a second format.

## Consequences

- `ADR-0001`'s locality claim is corrected, not deleted. It said engine additions
  touch the trait and adapters only. That was true of the *count* of files and
  false of the *cost*: an addition crossed four implementations, which is the
  opposite of the locality that justified deleting `VcsManager`. After move 4 the
  claim holds in fact — the port plus two adapters.
- The **Git engine** glossary entry stops asserting a policy no code implements.
  Production does not run an `Auto` backend that answers from libgit2 and
  delegates for the rest; it runs one composed adapter that is mostly CLI.
- `ADR-0013`'s contract — that **Granular op** composes its patches from cached
  raw text — is amended by this decision: patches will be composed from the spans
  of a patch value. What 0013 was protecting, that a staged selection is appliable
  because it was built from text git would accept, must be restated in terms of
  the value when that ticket lands. `ADR-0014`'s cached **Display row** model
  survives the amendment: side-by-side pairing stays a presentation concept, and
  the object a viewer scrolls is not a hunk.
- The diff-text collapse is a wide refactor, not a slice, and is sequenced
  expand-contract-batch for that reason. The count above is the justification.
- The sanctioned impurity stands. This decision does **not** inject
  `Arc<dyn GitExecutor>` from the composition root; `AppState` continues to call
  `build_executor`, per `AGENTS.md`. Nothing here makes that job this branch's,
  and it should not be "finished" alongside it.
