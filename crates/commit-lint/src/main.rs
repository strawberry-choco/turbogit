//! Validates one commit-message file with cocogitto's parser — the same
//! implementation `cog verify` runs, so the repo's commit spec has exactly one
//! parser (ADR-0028 D2). Tooling, not a layer: nothing in the crate stack may
//! depend on it.

use std::process::ExitCode;

fn main() -> ExitCode {
    // cocogitto 7.0.0 panics on any settings access when the config path was
    // never set — `get_config_path` unwraps its `OnceLock` outside tests, and
    // only the real `cog` binary sets it (from `--config`). Set the default
    // before anything touches `SETTINGS`; `Settings::get` then resolves it
    // against the discovered repo root, falling back to defaults when no
    // `cog.toml` is in the tree.
    cocogitto::set_config_path(cocogitto::DEFAULT_CONFIG_PATH.to_owned());

    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: commit-lint <commit-message-file>");
        return ExitCode::FAILURE;
    };

    let message = match std::fs::read_to_string(&path) {
        Ok(message) => message,
        Err(err) => {
            eprintln!("commit-lint: cannot read {path}: {err}");
            return ExitCode::FAILURE;
        }
    };

    match cocogitto::conventional::commit::verify(
        None, &message,
        // ADR-0028 D5: the lint tools skip merge and fixup commits by default,
        // and git runs commit-msg for both.
        true, true,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
