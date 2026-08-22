# Mockups are the single source of truth for the UI redesign

The UI redesign spec (`docs/ui-redesign-spec.md`) was translated from HTML
mockups, but translation drift is inevitable. We decided that where spec and
mockup disagree, the mockup wins, and where the spec claims a behavior that
the mockup does not depict, the mockup's depiction is the scope — no invented
behavior. The spec is updated to match in the same change; silent divergence
is forbidden.

This is a deliberate deviation from the obvious "code follows the written
spec" path: a future reader will find UI behavior with no code behind it
(inert topbar menus, Run/Debug rail buttons, Preview buttons) and might
"fix" it by removing the visuals or wiring up behavior that was explicitly
descoped. The decision keeps the redesign honest to its approved visuals
while making scope gaps explicit rather than papered over.
