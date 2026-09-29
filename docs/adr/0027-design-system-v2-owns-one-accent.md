# Design system v2 owns one accent, one ink ramp, and one card

The Local Changes redesign turned out to change more than one screen. The design it
was drawn to removed nested card borders, collapsed several competing blues into one
accent, and split the type ramp so hierarchy exists at all — and every one of those is
a **design-system** decision, not a Changes-screen decision. `theme.rs` and
`docs/design-system-roles.md` are where they are owned.

Left alone, the same problem gets solved again on every screen. Today: a window frame
with a 1px stroke, containing a bordered panel, containing a bordered card,
containing a well, containing a bordered diff card, with one screen stacking five of
them; two ink tokens that differ by two, two and one per channel and are therefore the
same colour wearing two names; one accent filling a button, a tab, a selection band, a
solid "Current" pill and a worktree's branch name; at least four different things
called a chip; four invented pane headers and no column header on any of the three
column-oriented panes; and three competing selection fills, one per view that needed
one. Five screens conformed separately produce five selection fills again.

## Decision

**Seven rules, R1–R7, enforced once in the token and widget layer and adopted by every
surface.** They are written here with the alternative each one was chosen over, because
a later change that seems to violate one has to be able to point at the argument that
was already had rather than re-derive it.

These rules **amend** the design contract rather than replacing it. The dark-only
palette (`ADR-0003`), the embedded data face (`ADR-0002`), the 24px minimum click
target, "no shadows — separation comes from surface colour", and the
monospace-for-data / proportional-for-chrome split all survive untouched, and the
migrations that carry them name no change.

### R1 — One accent

`Palette::BRAND #3574F0` may fill **only** the view's single primary action button, the
active tab underline, and a selected row's 2px leading rail. It is never a fill behind
running text, never a chip, never a state.

This kills the solid `SELECTION`-filled branch pill, the solid `BRAND` "Current" pill,
and the `Palette::BRAND` worktree branch label. The readable accent *inks* —
`ACCENT_TEXT #8BB5F5` and `LINK #74A3E8` — are unaffected: R1 constrains fills, and ink
is how a colour is allowed to touch text.

The number that settles the boundary is the accent's own contrast on the content
surface: 3.6:1. That is what a rail is for and what a fill behind running text is not
for. It is the same boundary the design-token suite asserts, and the current chip
below is the one documented exception — "this is the current ref" is a fact about a
ref, not a call to action.

**Rejected: one accent *family* — a small tinted ramp derived from `BRAND` (70% for
pills, 40% for hovers, 20% for rails), so identity and state share a hue and nothing
has to be grey.** Rejected because the moment the accent is three tints, the screen has
three blue objects and none of them is "the thing you press". Tinting is the treatment
this migration is removing, not an alternative to it.

**Rejected: keep the four distinct blues and rely on the layout to disambiguate.**
Rejected for the same reason the borders went: a value stops carrying meaning the
moment everything has one, and the eye parses the set before the content.

### R2 — Cards are surfaces, not boxes

A content region is `CONTENT_BG #232529` at `CARD_RADIUS 8` with **no stroke**. Regions
inside a card divide by spacing and at most one `DIVIDER #2B2D30` hairline. A 1px
stroke means *this floats* — popovers, dialogs, menus, toasts — and nothing else.
`widgets::window_stroke` already implements that half.

Card padding is the panel padding and **does not change in this migration**; that is
precisely why removing the stroke does not collapse the region, because the 12px of air
was already doing the separating and the hairline was redundant with it. (The
specification's prose says padding "rises to the panel padding" in this change; measured
against the tree it already is, so the rule is recorded as a constraint rather than as a
change. The consequence that *is* a change is negative: removing a stroke must move no
pane's content, so each carded region's inner content rect ends up identical minus the
stroke's two pixels, and that is asserted rather than eyeballed.)

**Rejected: keep the card stroke but weaken it** — a 1px line at lower contrast, or at
`RULE_CONTENT`'s tone rather than `LINE`. Rejected for the reason above, restated
precisely: the stroke is not too strong, it is too *many*. Five nested borders is not a
contrast problem.

**Rejected: keep the stroke on the outermost container only and re-decide the inner
ones per screen.** Rejected: that leaves the inner boxes to an argument per view, which
is the argument this migration exists to end.

**Rejected: a shadow or a lifted fill instead of the border.** Rejected as already
settled — `ui::components`'s module contract ("no shadows anywhere, separation comes
from surface colour") — and R2 applies the same principle to borders: separation is
carried by surface tone.

**Rejected: a card with no stroke *and* no padding variation** — a flat sheet of colour
with no internal structure. Rejected: the card's job is to be a region you can see, and
R2's answer is that the region is carried by the fill and the padding, with at most one
hairline inside it.

### R3 — Ink is a four-step ramp

| Role | Token | On `CONTENT_BG` | Use |
| --- | --- | --- | --- |
| names, values, the row being read | `INK #DFE1E5` | 13.0:1 | filenames, branch names, values |
| body, secondary labels | `INK_2 #B0B3BB` | 7.4:1 | prose, key labels, input text |
| section labels, column headers, metadata, refs | `INK_3 #8A8E96` | 4.56:1 | `LOCAL`/`REMOTE` bands, `TYPE_SECTION` titles, column headers |
| placeholders, dim path suffixes, hatches | `INK_4 #6E727A` | 3.16:1 | never the only rendering of information the user needs |

`INK_3` moves from `#AEB2BA` to `#8A8E96` and `INK_4` is new. The two figures that are
load-bearing are that `INK_3` still clears 4.5:1 on every surface it paints muted ink on,
and that `INK_4` is the only step below it. Both are asserted as maths over the token
values, not read off that table.

`INK_4` **fails AA and that is the whole reason it is a separate name** rather than
"INK_3 but dimmer". A name is the only thing in this codebase that can enforce a
restriction: a comment saying "do not use this for a section label" is advice, and a
token that is named for the restriction can be asserted.

`INK_3`'s documented contract also **narrows**, and that is the part worth reading: it
stops being legal everywhere. It is legal on the app background, the panel background
and the content surface; on a *raised or selected* surface the caller steps up to
`INK_2`. This is R5's insight applied to ink instead of fills — a value's legality
depends on the surface it sits on.

**Rejected: keep two muted steps and get hierarchy from size, weight and case alone.**
Rejected: the type ramp is real and it is not enough, because the pairs that have to
separate include a 9px column header and an 11px metadata value, and no amount of case
distinguishes an 11px label from an 11px value. Hierarchy needs a value.

**Rejected: leave `INK_3` at `#AEB2BA` and add a new dim token underneath it.**
Rejected because the current value is not illegible — it is *indistinguishable* from
`INK_2` (1.01:1 between them), which is the actual defect. Adding a fifth step below an
undistinguished third step produces a five-step ramp with a hole in the middle.

**Rejected: a fifth, "ink 5", for real hatches.** Rejected: four steps are what the
roles in the table need, and a fifth is a token whose only argument for existing is that
it exists.

**Correction to the plan.** The plan's ticket for this ramp also listed replacing
`#7E838C` literals. That literal no longer exists anywhere in the tree — the recent UI
refactors removed it — so only the token change remains and the literal sweep is dropped
as already-done work.

### R4 — Selection is a composite, not a saturated fill

A selected list row is `ROW_SELECTED #243456` — the opaque equivalent of the existing
translucent `Palette::selection_bg()` composite — **plus** a 2px `Palette::BRAND` rail at
the row's leading edge. Ink does not invert: a selected row's text is the colour an
unselected row's text is, so selecting a row does not cost the user the ability to read
it. `Palette::SELECTION #2E4369` is **not deleted**; its scope narrows to the heavier
"this is the current ref" treatment, and the row-fill decision function documents that a
list row must not resolve to it.

The rail is **paint, not layout**. Reserving the rail's width as padding would shift
every row's text origin, and "a selected row's first text origin equals an unselected
row's" is a hard criterion of this rule, not a detail.

The three existing selection row states narrow to the three that are real: a list row
(`ROW_SELECTED` + rail), a current ref (`SELECTION`), and the focus band the log table,
the sidebar tree and blame still use. The row-state enum drops the state that was only a
duplicate of another and keeps the distinctions that carry meaning.

**Rejected: a saturated brand fill for the selected row**, which is what
`RowState::BrandSelected` and the `RowState::from_flags(selected, hovered)` constructor
hand every hand-painted row today. Rejected because the loudest blue in the app would
then be the default selection for hand-painted rows — it out-shouts the primary button
and the accent stops meaning *press this* — and because it also forces the ink to invert
for legibility, which is a second cost bought for no gain.

**Rejected: the flat `ROW_SELECTED` fill with no rail.** Rejected: at 1.24:1 against
`CONTENT_BG` the fill is a weak signal in a long list. The rail is what makes the row
findable without tracking a colour across the screen, and it is the one place R1 allows
the accent.

**Rejected: delete `SELECTION` outright** and give the current-ref treatment a new value.
Rejected: the current-ref band is a *fact about a ref*, and keeping the value while
narrowing its scope costs one documented line and migrates no call site. The row-fill
decision asserts the narrowing where it is made rather than by a sweep.

### R5 — Raised controls step up relative to their own surface

| The surface the control sits on | Raised fill |
| --- | --- |
| the app background | `SURFACE #2B2D30` |
| a `CONTENT_BG` card | the raised-on-card role, an alias of `SURFACE_2 #313438` |

A control that does not step up from the surface it actually sits on is invisible, and
this is currently decided per call site and drifts. The role is a **name** rather than a
constant precisely so that R5 is a rule and not a per-call-site judgement.

**Rejected: one raised token for the whole app** — keep `SURFACE` everywhere, or promote
`SURFACE_2` everywhere. Rejected on the arithmetic: `SURFACE` on a `CONTENT_BG` card is
almost the card, and `SURFACE_2` on the app background is a step the hover role already
makes correctly. One value cannot be right on both surfaces, so the role is a mapping.

**Rejected: let each call site keep choosing, and document the ladder in the roles
document only.** Rejected: that is the current state. The drift is invisible until a
button vanishes into a card, and a document cannot assert.

**Rejected: derive the raised fill automatically from whatever the widget is painted
over.** Rejected: the surface a control sits on is a token the *caller* chose, so a
widget cannot read it back — it has to be handed it, which is what the named role is.

### R6 — Ref chips are neutral; state is coloured

One `ref_chip()`: raised-on-card fill, `INK_2` text, `CHIP_RADIUS 3`, `TYPE_CHIP 10`,
monospaced face, because it holds a ref name. Used by branch names, remote refs,
worktree branches and the log's ref pills.

The chip vocabulary is **closed at three**: the ref chip above; the **current chip**,
which is the selected-row fill with accent text and is the *only* chip permitted to
carry the accent, because "this is the current ref" is a fact about a ref rather than a
call to action; and the **count chip**, the raised fill with secondary monospaced ink,
for the ad-hoc counters — including every counter that was borrowing the reserved
counter orange for something that was not a dirt or unpushed count.

Semantic state is **not a chip**. `dirty`, `↓ 2 behind`, `Up to date` and
`Needs update` render as coloured text or a leading dot, coloured from the one
`RepoState::color()` map that already exists. This is what removes the blue soup from
the branches screen.

A ref chip that is a *label* does not gain a click plane: `widgets::hash_chip` is a
label by decision (`ADR-0024` moved copying to the commit menu) and a ref chip inherits
that. A chip that is a **control** is a different thing and says so in its own
accessibility node — see the branches scope chip under *The two named reversals*.

**Rejected: a status-coloured chip vocabulary — one chip per state, so the colour is a
legend.** Rejected: a filled pill per state is the badge soup this migration removes,
and the same fact is just as readable as coloured text at the same ink step. It also
makes "a chip" mean five things, which is the v1 complaint restated.

**Rejected: make the current-branch pill neutral like every other chip, and drop the
distinction.** Rejected: the distinction is real (a current branch is a repository fact,
not a pointer state) and the current chip carries it without introducing a second blue —
the selected-row fill does the work.

**Rejected: no chips at all — refs as bare monospaced text.** Rejected: the complaint is
that four *different* chips existed, not that chips existed. The chip is what makes a
ref scannable down a column, and the ref-chip role is what makes that one decision
rather than five.

### R7 — One pane-chrome vocabulary

Every tool pane — the log's branches strip, worktrees, submodules, and the changes
card — uses, in this order: a 9px tracked `INK_3` title, an optional count chip, a
right-aligned action slot, one hairline, then content. Column-oriented panes (the log's
commit table, worktrees, submodules) add one shared column-header row: 9px tracked
`INK_3` over a `RULE_STRUCTURAL` underline. The header and the data rows derive their
column offsets from the same constants, so a column cannot be narrow in the header and
wide in the rows.

`widgets::toolwindow_header` is **retired** in favour of that one implementation, and
the branches pane's differently-cased title goes with it. The conflict panes' private
header is absorbed rather than left behind, because a second pane-header
implementation that nothing flags as unused is exactly the drift.

**Column headers use `INK_3`, not `INK_4`.** 9px is normal-size text and `INK_4` is
3.16:1. The target frames were drawn with the dim value before this was checked, so this
is pinned deliberately: an implementer who copies the frame finds a test disagreeing
with them and the disagreement is the point.

Rows stop hand-rolling their own geometry too. One row shell owns row height per row
kind, leading padding, the hover fill, the radius, and the selected fill plus rail; one
rail painter owns the 2px accent and absorbs the two independent rail sites that exist
today (the sidebar's stroked segment and the diff pane's filled rect) so every row that
grows a rail places it identically.

**The log is the one documented exception, and the design accounts for it.** The log is
virtualised: its own row allocation, its own measured pitch, its own eliding and its own
paint helpers that bypass the shared vocabulary entirely. Routing it through a row shell
that re-measures per row would be a rewrite of the hottest path in the app. So the row
shell takes a variant that accepts an already-allocated rect — the log keeps its pitch
and its virtualisation and shares the fills, the rail and the ink ramp — and log
conformance lands last in the view phase for the same reason.

**Rejected: fix each pane's header separately.** Rejected for the argument R1 and R2
make: four headers means the same three decisions taken four times, and five panes
conformed independently is the state this migration exists to leave.

**Rejected: standardise on the existing `toolwindow_header` as it stands.** Rejected:
it is a title/action strip with no count chip and no column-header row, and the branches
pane's title case is a fifth variant of the same idea. Promoting it would enshrine a
shape that was never drawn at this size.

**Rejected: a configurable pane header** — caller-supplied title, optional count,
optional action slot, optional second line. Rejected on the non-negotiable boundary this
document already states: a configuration-heavy universal widget is how a chip becomes a
row and an alert becomes a toast. The slots are fixed and the vocabulary is one.

**Rejected: no column headers, with each label carried in the first row's cells.**
Rejected because a column that is narrow in the header and wide in the rows is the
re-anchoring complaint verbatim, and because a bare hash or timestamp is not
self-describing.

**Rejected: a full row-shell adoption of the log** (one measured pitch for every list in
the app, virtualisation included). Rejected on the frame budget: re-measuring per row
would trade the hottest path in the app for symmetry with lists that are not hot. The
variant that takes an allocated rect is the same rule with a documented seam, and it is
recorded here so the exception does not read as a violation.

## Status

Accepted, and landing. `.scratch/design-system-v2/` holds the twenty-one tickets; the
plan and the tickets are the working documents and are gitignored, so this record and
`docs/design-system-roles.md` are the durable pair.

| Rule | Lands in |
| --- | --- |
| R1 | 05 (chip vocabulary), 07 (rail), and every view ticket — 09, 10, 12, 16, 17, 18, 19 |
| R2 | 04 (card), 19 (shell, sidebar, dialogs) |
| R3 | 02 (tokens), then 06 (column ink) and every view ticket |
| R4 | 03 (token, rail width, row-state narrowing), 07 (rail painter), 14 (log variant) |
| R5 | 03 (raised-on-card role), 04 (card surface) |
| R6 | 05 (chip vocabulary), 07 (state marks), 16, 17, 18 |
| R7 | 06 (pane and column chrome), 07 (row shell), 13 (log chrome), 14, 16, 17, 18, 19 |
| Reversal recorded and implemented | 16 (branches scope chip) — landed; the "not a chip" comment at the call site is replaced, not deleted, so the reversal is readable where the code is |
| Rationale confirmed and implemented | 18 (submodules status text and its state dot) — landed; the "not a chip" comment survives byte-intact and the dot is a mark, not a chip |

All twenty-one tickets have landed. A rule in this table is **decided** and a call site
it sweeps is conformed; `docs/design-system-roles.md` carries the same mapping next to
each rule, because a reader who lands in the roles document and not here needs the same
answer: which is current, the code or the rule.

## Where the target frames and this ADR disagree

ADR 0016, as amended, makes the five Ardot target frames the approved visuals for their
five screens and demotes the acceptance captures for those screens to regenerated
artefacts. That amendment is correct about what the frames are for and **silent about
what happens when a frame shows a pre-v2 treatment.** The frames were drawn before these
rules existed, so this is a live contradiction, not a hypothetical one, and it was found
by reviewing the regenerated captures against the frames rather than by reading.

**The resolution: the frames' authority is bounded by the rules in this ADR.** A frame
is authoritative for composition, hierarchy, spacing and the *identity* of what a screen
shows. It is **stale on any axis this ADR changed**, because it necessarily depicts the
v1 treatment there. Where they disagree on such an axis, the frame is the artefact that
is out of date — not the code.

This is not ADR 0016 being overridden. It is this ADR applying the same precedence
principle to itself: the design system's own rules are the design system's authority,
and a mockup drawn before them cannot outrank them. It is also the reason the captures
are still worth regenerating: a capture compared against a stale frame produces a
confusing diff, and saying which axis is stale is what makes the comparison honest.

**The one axis where the frames are demonstrably stale today.** The Log frame (`3:333`)
paints the selected commit row as a **solid saturated blue band with inverted text** —
exactly the treatment R4 removes, and the reason R4 exists (it was, with the Commit
button, one of the loudest things in the app). The shipped render paints the
row-selected fill `#233455` plus a 2px `BRAND` rail at the leading edge, with the row's
own inks unchanged. **The frame is right about the screen and wrong about the
selection; R4 is the decision, and the capture is the evidence.**

**The one axis where the code is out of conformance, recorded rather than absorbed.**
All five frames depict a **different shell** from the one that shipped: a 244 px sidebar
(the code's is 295 px), tool panes drawn as separated rounded cards with 10 px gutters
(the code draws them edge to edge with 1 px rules), and a `#1E2023` page background —
which is the `PANEL_BG` token this migration **deleted** in its dead-token sweep. That
is a real and systematic divergence, not a nit, and it is **not** fixed here: the shell
was out of scope for all twenty-one tickets, and a sweep that "conformed" the shell to a
frame would have rewritten geometry that nine view tickets had just measured. It is
follow-up work in its own right.

**What this changes about reviewing a capture.** A reviewer comparing a capture to a
frame must first establish which axis is under comparison. On composition the frame
wins and the capture is the artefact. On R1–R7 the capture wins and the frame is stale.
On shell geometry neither is currently authoritative and the divergence is open.



Two decisions this repo already made **in writing** are affected, and both are recorded
in full in `docs/design-system-roles.md`, which is where the argument lives because the
two code comments that touch it are near-identical and cannot be told apart:

- **`ui::branches::scope_label` — reversed.** The comment says it is "not a chip, and
  deliberately not one", because a background would newly register an accessibility
  node for a piece of status text. **R7 overrode it**, and the reversal answers the
  argument rather than overruling it: under R7 the pane's scope is a **control**, not
  status text — R7 gives every tool pane one chrome row with a right-aligned action slot,
  and a pane whose scope is a *filter* has to express that filter in that row as
  something you can act on. An accessibility node for a control that changes the
  repository filter is the affordance, not a side effect. Ticket 16 implements it and
  **must delete or rewrite the comment it reverses**; until then the comment and this
  record disagree, and this record is the one that says so.
- **`ui::submodules::status_label` — kept.** The same comment, the same reason, and
  **no rule overrode it: R6 ratifies it**, because R6 says semantic state renders as
  coloured text or a dot and never as a filled pill. The leading state dot ticket 18
  adds to that row is a **mark**, not a chip — a filled circle, no fill rect, no radius,
  no chip geometry, no node of its own. The comment stays, and it stays true of the code
  it describes.

## What this amends, and what it does not

- **Amends `docs/design-system-roles.md`**, whose shared-role sections now state R1–R7
  and record the retirements and narrowings this ADR makes. The "similar names with
  different roles" catalogue is deliberately left intact: v2 narrows roles, it does not
  delete the vocabulary that keeps two similar names apart.
- **Amends `ADR-0016`'s correction paragraph** — by amending that file, not by
  superseding it. The five Ardot target frames (Local Changes `3:1`, Log `3:333`,
  Branches `3:664`, Worktrees `3:995`, Submodules `3:1326`, one design file) are the
  approved visuals for their five screens, so the acceptance captures for those screens
  become regenerated artefacts rather than the authority, and the captures re-render at
  1440×900 — the size the design was drawn and validated at.
- **Extends `ADR-0003` (dark-only).** Still exactly one palette; R3 adds two steps to it
  and no mode returns.
- **Extends `ADR-0002` (embedded JetBrains Mono).** R6's ref chip is monospaced *because*
  it holds a ref name, and the chrome/data split is unchanged.
- **Does not contradict `ADR-0010`.** The changes header's five icon buttons become
  three and the two unwired ones fold into the overflow menu. They gain no behaviour,
  which is what the inert-control convention requires; what moves is where a control
  that cannot yet do anything is parked, and the Local Changes target frame is what
  moved it.
- **Does not touch `ADR-0011`, `ADR-0012`, `ADR-0023` or `ADR-0024`.** No action surface
  moves in this migration: the branch and commit single-menu rules stand, and the chip
  and pane-chrome work is presentation only.
- **Does not touch `ADR-0026`'s frame budget.** R7's log exception exists so that it
  does not.
- **Does not touch the deferred architectural work** — executor injection, splitting
  `GitExecutor`, decomposing UI state. None of it is a prerequisite, and none of it is
  scheduled here.

## Considered options

- **Conform the five screens first and generalise afterwards.** Rejected: five screens
  conformed separately each invent their own selection fill, which is the state the
  tree is in now. The token and widget layer lands first because no view can conform
  until the values and the shared widgets exist; the log lands last, and the captures
  regenerate only after every view has landed, because regenerating mid-sweep produces
  baselines that mix old and new chrome.
- **Put the seven rules in the code comments at each call site.** Rejected: a rule
  enforced at twenty call sites is twenty rules that can drift, and the two "not a chip"
  comments this migration reverses are the proof — a rule with no definition gets applied
  by contradiction. One definition, in the token layer, asserted as maths and as painted
  output.
- **Keep the rules in this ADR and delete `docs/design-system-roles.md` as the second
  source of truth.** Rejected: the roles document answers a different question — which
  similar names must stay apart, and who owns what — and that catalogue is what stops a
  future sweep from folding a chip into a row. Two documents, one rule set, each with
  one job.
- **Make the acceptance captures continue to outrank the design, at 1280×800.** Rejected
  in the `ADR-0016` amendment: an authoritative artefact captured at a size the design was
  never validated at is a weaker contract than the frame it is supposed to be checked
  against.
- **Give the diff pane the same treatment for consistency.** Rejected and out of scope:
  the diff pane was never the problem, and its background/text pair, the removed accent
  and every diff row fill stay exactly as they are. Only the added accent moves, because
  it is the same token as the added-file accent.

## Open follow-up work

Found by reviewing the regenerated acceptance captures against the frames, after all
twenty-one tickets landed. **None of these is a v2 rule and none was in scope for the
migration**; they are recorded here because the working plan is gitignored and this is
the only durable place they survive.

- **The shell's geometry is out of conformance with all five frames.** Sidebar width,
  pane separation and the page background all differ, and the frames' background is a
  token this migration deleted. Described in full above. This is a piece of work in its
  own right, not a sweep at the end of this one.
- **`07-merge-editor` captures an empty merge editor.** All three panes render blank
  against a genuinely conflicted repository on disk, so this reads as a data-loading
  defect rather than an empty fixture. The capture currently ratifies a merge editor
  that displays no merge.
- **`06-push-dialog` captures an empty push** ("No outgoing commits"), so no capture
  observes the dialog's populated state — the state the dialog exists to show.
- **The status bar does not paint on any repo-backed page**, though all five frames show
  one. `show_status_bar` defaults to `true`, so something in the repo-backed layout is
  claiming the band.
- **The acceptance captures are not byte-reproducible.** Repo-backed pages name a
  `tempfile` root and render relative dates, varying 0.02–0.29% of pixels per run. A
  reviewer cannot distinguish a real regression from a re-run. A neutral fixture would
  fix it; `01-welcome` is the one page that is now deterministic, because its recents
  store is injected rather than read from the developer's config directory.
- **The Branches *tab* has a frame and no capture.** `05-branches-popup` is a floating
  popup over the Log page, so the tab body is unobserved despite having a target frame.

## Consequences

- **The token delta is small and closed.** Only `INK_3`, the new `INK_4`, the new
  `ROW_SELECTED`, the added/diff-added accent, and the new raised-on-card and rail-width
  roles move or arrive. `SELECTION`, `SURFACE_2`, `AHEAD`, the counter orange, danger,
  the link colour, the accent-text colour, every radius and every height stay exactly as
  they are. A ticket that changes a value not in that list is arguing with this ADR.
- **The added accent is the one token whose contrast was measured and found failing on
  the surfaces the palette audits** — 4.33:1 → 5.84:1 on `CONTENT_BG`, a real correction
  rather than a preference, and it is the same token as the added-file accent, which is
  why the diff pane's own palette is touched by exactly one value.
  *(The spec says 4.21:1 → 5.72:1 and calls this "the only token in the palette that
  actually fails AA today". Both figures are wrong, and the exclusivity claim is false as
  written: `BRAND`/`ACCENT` `#3574F0` fails on every surface audited, `DANGER` measures
  4.40:1 on `CONTENT_BG`, and `AHEAD` measures 4.36:1 on `SURFACE_2`. The figures above
  are the measured ones and the claim is scoped to what was actually swept. This is the
  same correction ticket 02 applied in the token layer: the spec checked one surface and
  generalised, and the measurement is the record.)*
- **A token that exists only to explain why nobody uses it is not a contract.** The
  `SIDEBAR` token is the named example: the sidebar adopts it or the token dies, in the
  same change, and no token is kept alive by loosening the assertion that pins it.
- **A stroke in the app now means "this floats"**, and that is assertable at the sites a
  test enumerates: the default card paints no stroked rect, and every surface that does
  stroke is a popup, a dialog, a menu or a toast.
- **Two rules are negative** — "no list row fills with the selection token", "no raised
  control on a content card uses the wrong fill" — and a render seam can only prove those
  at the sites a suite enumerates. They are pinned at their **construction sites**
  instead: the row-fill decision, the raised-control role, the chip constructors.
- **The capture set and the design set now have a written precedence per screen**, which
  is the thing that was missing: a disagreement between a capture and its frame is
  settled by looking, and a disagreement about a page no frame depicts is still settled
  by the capture.
- **The rules are asserted, not trusted.** Every rule gets an assertion, and every
  assertion gets hand-broken once. A ratchet that has never been seen to fail is a
  comment with an assertion in it. Three suites pin the v1 values and go red on purpose
  while the token and widget layers land; each is updated by the ticket that moves the
  value it pins, not by a follow-up.
- **A reviewer landing between this record and unswept code is the failure this Status
  table exists to prevent.** The rule is current; the call site is not; the ticket column
  says which change makes them agree.
