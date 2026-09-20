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

This is a deliberate deviation from the obvious "code follows the written
spec" path: a future reader will find UI behavior with no code behind it
(inert topbar menus, Run/Debug rail buttons, Preview buttons) and might
"fix" it by removing the visuals or wiring up behavior that was explicitly
descoped. The decision keeps the redesign honest to its approved visuals
while making scope gaps explicit rather than papered over.
