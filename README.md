# TurboGit

Desktop Git client built in Rust with `eframe`/`egui`. Modeled after IntelliJ IDEA's Git integration, with first-class multi-repository (multi-root) support.

In-process libgit2 (`git2`) as the primary backend with the system `git` CLI as fallback for sync/credential operations.