# Keyboard shortcuts preserved as-is through the redesign

**Amended (2026-10-10): the bindings held, "no new required shortcut" did not.**
No existing binding was touched, no keymap UI was built, the Keymap row is still
visual-only, and `tests/r4_polish.rs` still presses all five original combos.
But the parity programme added seven keys, because the reference's verbs had no
TurboGit equivalent:

- `⌃⇧D`, `⌃⇧⇥` and `F4` are dispatched in the shell (`ui/shell.rs`), gated on a
  diff being previewed — the options menu, the pane switch, Jump to Source.
- `⌘D` is the apply-patch window's `Show Diff` preflight (`ui/apply_patch.rs`),
  read while that window paints so it cannot fire under a text field.
- `R`, `E` and `U` set a rebase-plan row to Reword, Edit or Update Ref
  (`ui/interactive_rebase.rs`), joining the `P`/`S`/`F`/`D` already there.

Two of the seven are **required** in this record's sense — nothing else reaches
them: `F4` is the only Jump to Source, and `⌃⇧⇥` is the only writer of
`UiState::diff_pane`. The other five are accelerators over a control that also
answers a click (the file-row trigger, the `Show Diff` button, the verb chips), so
they broaden the surface without making a key load-bearing. The freeze itself is
undisturbed: none of these rebinds or removes an existing combo.

Existing global shortcuts (Ctrl+K commit, Ctrl+Shift+K push, Ctrl+T refresh,
Ctrl+Shift+A find, Alt+` VCS ops popup) keep their current bindings. The
redesign adds no rebinding, no keymap UI, and no new required shortcut; the
mockup's inert Keymap settings row is visual-only.

Reason: the acceptance matrix requires these five combos to survive the shell
rewrite; changing them mid-redesign would confound regression triage (is it the
restructure or the remap?). Rebinding belongs to a future Keymap feature that
owns its own design pass.
