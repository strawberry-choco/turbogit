# Mockups are the single source of truth for the UI redesign

The UI redesign spec was translated from HTML mockups, but translation drift is
inevitable. We decided that where spec and mockup disagree, the mockup wins, and
where the spec claims a behavior that the mockup does not depict, the mockup's
depiction is the scope — no invented behavior. The spec is updated to match in
the same change; silent divergence is forbidden.

**Correction — what is actually in this repository.** Neither artifact named
when this was written is in the tree: `docs/ui-redesign-spec.md` does not exist
(`docs/product-spec.md` is the *product* spec and describes no visual language),
and the HTML mockups were never committed. What stands as the approved visuals
today is the capture set in `crates/turbogit-ui/tests/snapshots/`
(`01-welcome.png` … `08-settings-modal.png`), which
`crates/turbogit-ui/tests/acceptance_matrix.rs` renders
and compares against. Read the precedence rule above as *code follows those
captures*, with the same ban on silent divergence. Where a capture and the code
disagree, the capture wins.

**Amendment (2026-09-28, `ADR-0027`) — the five target frames are the approved
visuals for their five screens.** A second artifact now exists, and it outranks
the captures for a bounded set of screens. Local Changes (`3:1`), Git Log
(`3:333`), Branches (`3:664`), Worktrees (`3:995`) and Submodules (`3:1326`) are
five frames in one Ardot design file, held locally as a PNG and SVG pair each.
For those five screens the **frame** is the approved visual and the acceptance
capture for that screen is a **regenerated artefact**: evidence that someone
looked at the render and compared it against the frame, not the thing being
compared to. The precedence rule above is otherwise unchanged.

Two qualifications, because a precedence rule with an unbounded scope is how this
correction paragraph went stale the first time:

- **The frames' authority is bounded by ADR 0027.** They were drawn before the v2 rules
  existed, so on any axis R1–R7 changed they necessarily show the v1 treatment and the
  frame is the stale artefact. ADR 0027's "Where the target frames and this ADR disagree"
  states the resolution, names the one axis where the frames are demonstrably stale today
  (the selected row's saturated band, which R4 replaced), and records the one axis where
  the **code** is the one out of conformance (the shell's geometry and page background).
  Read that section before comparing a capture to a frame: it decides which of the two is
  the artefact on the axis you are looking at.
- **The frames say nothing about the shell, the sidebar or the dialogs**, and they do
  not depict welcome, the diff pane, the branches *popup*, the push dialog, the merge
  editor or the settings modal. Scope the precedence per screen, against the nine
  committed captures:
  - `03-git-log.png` and `03-git-log-selected.png` cover a screen a frame depicts, so
    they become regenerated artefacts and are reviewed against `3:333`.
  - `01-welcome`, `04-diff`, `05-branches-popup`, `06-push-dialog`,
    `07-merge-editor` and `08-settings-modal` are pages **no frame covers**, so they
    keep the authority the rule above gives them. `05-branches-popup` is the one that
    looks like an exception and is not: it is a floating picker, a different surface
    from the Branches *pane* the frame `3:664` depicts.
  - `02-commit` is the partial case. The Local Changes frame `3:1` depicts the commit
    tool window's changes card, so that capture is a regenerated artefact for that
    region; for the parts of the window the frame does not draw — the message well
    above all — the capture is still the reference, and the frame is silent.
- **Two of the five frames carry the least human intent.** Worktrees and Submodules
  were drawn *from* the current code (`ui/worktrees.rs`, `ui/submodules.rs`) rather
  than screenshotted, because those two views had no designed state before. For those
  two, a capture that matches its frame proves only that the code still agrees with
  itself; the review is a designer's judgement about whether the result is right.
  They also have no capture today at all, and the migration adds one for each — so
  these are the first artefact *and* the first review for two views nobody was
  watching, which is the pair most worth a second look.

**Capture size, settled at the same time: 1440×900.** The approved-visuals rule
makes the artefact authoritative, and an authoritative artefact captured at a size
the design was never validated at is a weaker contract than the frame it is checked
against. Measured today: `crates/turbogit-ui/tests/acceptance_matrix.rs:45` sets the
harness to `vec2(1280.0, 800.0)` and all nine committed PNGs are 1280×800, while the
frames are 1440×900. So the captures re-render at **1440×900**, the size the design
was drawn and validated at, and the cost of larger PNGs in the repository is
accepted. The regeneration happens **once, at the end of the migration**: rendering
mid-sweep produces baselines that mix old and new chrome, which is worse than no
baseline at all. `UPDATE_SNAPSHOTS` is set to the force form in that suite, so an
ordinary `cargo test` run rewrites all nine goldens — a run that is not the
regeneration leaves the working tree dirty, and the committed captures are restored
from git in that case.

This is a deliberate deviation from the obvious "code follows the written
spec" path: a future reader will find UI behavior with no code behind it
(inert topbar menus, Run/Debug rail buttons, Preview buttons) and might
"fix" it by removing the visuals or wiring up behavior that was explicitly
descoped. The decision keeps the redesign honest to its approved visuals
while making scope gaps explicit rather than papered over.
