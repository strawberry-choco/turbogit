# One engine: the backend choice is retired

`ADR-0022` left the Settings row with two adapters behind it and proved the
third had never selected anything. This record removes the row and the field
behind it: the composed adapter is the engine, and the factory builds nothing
else.

## Context

The picker's history is three options, then two, then none:

- The original Settings row offered three. `ADR-0022` measured that two of them
  (`Libgit2`, `Auto`) constructed a byte-identical object, and collapsed them
  onto the `InProcessReads` spelling.
- What remained was a genuine choice between the bare CLI adapter and the
  composed one, a choice that changed observable behavior (git's DWIM on
  `branch_create` from a remote start point records an upstream under the CLI
  adapter and under nothing else; `backend_options.rs` pins the pair).
- The choice was never the app's to offer: the composed adapter is the default
  and the only configuration the product shipped in practice, and a user who
  picked `Cli` was trading away in-process reads for one DWIM side effect the
  UI does not surface and no other surface compensates for.

## Decision

`build_executor` builds the composed executor unconditionally. The
`backend: GitBackend` field is deleted from `VcsSettings`, the `GitBackend`
enum with it, and the Settings row from the Git page.

A `state.ron` carrying the key, any of the four spellings it ever serialized
as, still loads: the key is an unknown field the loader ignores, exactly like
`staging_area` before it. The guarantee is pinned with the strongest probe
available: a saved `Cli` spelling must not bring the CLI adapter's DWIM
behavior back (`retired_settings_keys.rs`).

## Consequences

- The DWIM difference is now a fact of the one engine rather than a setting:
  `branch_create` never records an upstream from a remote start point, and the
  app writes tracking refs explicitly (the checkout path always did).
- The CLI adapter stays. It is the composed adapter's delegation core, and
  engine suites drive it directly as the parity oracle. What disappears is only
  the ability to *run on it alone*.
- `backend_options.rs` keeps its reason to exist: it pins where the two
  adapters genuinely differ, which is now the boundary of the composed
  adapter's in-process coverage rather than a user-facing choice.
