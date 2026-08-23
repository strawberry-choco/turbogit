# Keyboard shortcuts preserved as-is through the redesign

Existing global shortcuts (Ctrl+K commit, Ctrl+Shift+K push, Ctrl+T refresh,
Ctrl+Shift+A find, Alt+` VCS ops popup) keep their current bindings. The
redesign adds no rebinding, no keymap UI, and no new required shortcut; the
mockup's inert Keymap settings row is visual-only.

Reason: the acceptance matrix requires these five combos to survive the shell
rewrite; changing them mid-redesign would confound regression triage (is it the
restructure or the remap?). Rebinding belongs to a future Keymap feature that
owns its own design pass.
