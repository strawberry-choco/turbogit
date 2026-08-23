# Welcome screen is the default surface; project_dir is a launch parameter

Historically TurboGit auto-scanned its working directory at startup, so a
project was almost always already open. The redesigned shell has no
repository-list sidebar to fall back on, so we decided the welcome screen is
what you land on unless a project directory is supplied at launch
(`turbogit.exe path\to\project`, or an OS file-manager "open with" handoff).
Choosing a recent project or opening one from the welcome screen enters the
shell; File → Welcome returns to it.

This removes the implicit CWD dependency: double-clicked exe lands on welcome,
CLI invocation stays scriptable, and the old behavior of silently scanning
whatever directory the process started in is gone rather than preserved behind
a flag.
