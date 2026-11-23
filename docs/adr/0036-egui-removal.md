# 0036 — Remove egui and ship nsh bundles

Status: accepted (N7, the cutover)

## Context

ADR-0035 chose Native Shell (nsh) 0.5.x as TurboGit's presentation layer and
kept the egui UI frozen while N1 to N6 built the nsh app screen by screen. By
N6 the nsh crate carried the whole product: the bridge, the token layers, the
controls, every route screen, the dialog layers, the headless suites, and the
macOS lanes. N7 is the retirement phase: one UI, one harness, one release path.

The old UI was 84 suites, their PNG baselines, `kittest.toml`, and an egui
harness feature in `test-support`. Keeping any of it alive would mean two UIs
and two contracts forever, which the migrate-callers-then-delete rule rejects.

## Decision

1. **The nsh crate takes the name.** `crates/turbogit-ui-nsh` is renamed to
   `crates/turbogit-ui`. The old egui crate and its package are deleted; the
   last egui tree stays on the local annotated tag `egui-final` so a
   regression can be bisected.
2. **The root binary is a launcher and nothing else.** `src/main.rs` parses an
   optional project directory and calls `turbogit_ui::launch::launch`, which
   mounts `AppRoot` on the native backend at 1280x820 with a 1000x680 minimum
   content size (`MainWindowOptions.attributes.min_size`), dark, with
   `theme.json`. The root manifest depends on `turbogit-ui` alone; the old
   `TurbogitApp` and `src/app.rs` are gone. The crate's own `main.rs` remains
   the entry `nsh run` and `nsh package` build.
3. **The old harness goes with its crate.** `kittest.toml`, the egui harness
   and WCAG modules, and the `harness` feature are deleted from
   `test-support`. The `nsh` feature keeps the headless harness. `diff_parity`
   moves under `crates/turbogit-ui/tests` and its dev-dependencies move with it.
4. **No egui dependency survives.** `egui`, `egui_extras`, `egui_kittest`,
   `dify`, `eframe`, `rfd`, and the old crate are removed from the workspace
   manifests and `Cargo.lock`. `cargo tree -i egui` fails because no package
   named egui is reachable.
5. **One release path.** `release.yml` runs `nsh package --target
   {macos,windows,linux}` from `crates/turbogit-ui` on the three hosted
   desktops. macOS produces an ad hoc signed `.app`, zipped for the release
   asset; Windows a `.zip`; Linux the native directory and `.tar.gz` (runner,
   bundle, `.desktop`) built on Ubuntu 24.04, the GTK 4.14 and glibc 2.39
   floor. AppImage and Flatpak with a pinned runtime stay deferred to CI for
   older distributions.
6. **The perf harnesses lose their egui arm.** The N4 to N6 perf suites now
   measure the nsh head against the absolute 16.7 ms frame budget and the 1%
   idle-CPU budget. The egui baseline remains available on the `egui-final` tag;
   N7's release-bundle perf lane is the shipped-artifact check.

## Consequences

- One UI (`turbogit-ui`), one harness (`nsh_testing::Headless`), one release
  command (`nsh package`). `AGENTS.md`, `README.md`, `CONTEXT.md`,
  `docs/architecture.md`, and `docs/design-system-roles.md` describe only that
  arrangement.
- Every old egui suite, snapshot baseline, and harness test is gone with its
  crate. The nsh suites and the moved `diff_parity` carry the behavior.
- Rollback is `git checkout egui-final`; the tag is local and is not pushed.
- Named gaps at cutover: the macOS `.app` is ad hoc signed, not Developer ID
  signed or notarized, because notarization needs the operator's credentials;
  the Linux and Windows install lanes were waived by the operator, so no live
  bundle launch is claimed there; VoiceOver/Narrator and polished pixel probes
  remain the N6 gaps they were.

## Alternatives considered

- Keep the egui crate as a fallback — rejected: two UIs, two harnesses, and the
  frozen code would still need maintenance.
- Re-export the old types from the renamed crate — rejected: a compatibility
  shim preserves the dependency the deletion exists to remove.
- Ship the old binary tarball — rejected: it carries no runner, no runtime, and
  no `.desktop`, so it is not the installable app the migration promised.
- Sign and notarize locally — impossible without the operator's Developer ID
  certificate, so it is recorded as a gap rather than faked.
