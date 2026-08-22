# Command palette and VCS popup keep their action sets; palette gains shell actions

Both surfaces are restyled to the new popup chrome, but their invokable action
lists do not shrink in v1. The command palette additionally gains the shell
navigation actions the new shell makes meaningful (Go to Git Log, Open
Welcome, Toggle Toolbar) and drops nothing: actions whose dialogs are restyled
in R3 (Merge, Rebase, Stash, Shelve, Tag, New Branch) keep working through
their existing dialog implementations until their restyle lands.

Reason: the palette is the keyboard-accessibility fallback for everything the
redesign turns inert or relocates; cutting entries there would strand
shortcuts the acceptance matrix requires to keep working.
