# Global recents live in the OS config directory, not per-project state

Recent projects must be visible on the welcome screen before any project is
open, but all existing persistence is per-project (`<project>/.git/turbogit/…`
style state files), which cannot host a global recents list. We decided to
store recents in one global file in the OS config directory
(`dirs::config_dir()/TurboGit/recents.ron`), holding `{ path, name,
last_opened }` only.

Branch indicators on welcome cards are computed fresh at render time (cheap
`git rev-parse` per visible recent, cached in memory), never persisted — a
stored branch snapshot would be stale the moment the user switches branches
outside TurboGit. This is the app's only global state file; everything else
stays per-project.
