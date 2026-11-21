# 0035 — Migrate the presentation layer to Native Shell (nsh) 0.5.0

Status: accepted (platform choice proven by the N0 spike, pending the
operator's redesign review at N1)

## Context

TurboGit's presentation layer is egui (`crates/turbogit-ui`) with kittest
snapshots as the visual contract. The program wants natively rendered
controls and native accessibility on macOS, Windows, and Linux, one
shared `AppState`, and no abstraction between the UI and the app
(`docs/nsh-migration-plan.md`). Native Shell (nsh) 0.5.0 is the chosen
candidate: apps written entirely in Rust on a reactive engine that
renders with each platform's own UI (AppKit, Win32/Direct2D, GTK 4), with
headless testing on the real engine and a TSX screenshot lane for visual
proof.

The N0 spike (`docs/nsh-spike-report.md`) had to prove the platform
before any porting: the 100k log on a virtualized list with per-row
canvas lanes, the 20k diff with a staged-line gutter, trees, tables, the
editor and IME, the overlays, the `Rc<RefCell<AppState>>` bridge, the
headless harness, screenshot tests, packaging on three desktops, and the
frame-budget gate against the egui trunk baseline.

## Decision

1. **The platform is Native Shell 0.5.0, in Rust, with no QuickJS.**
   The spike's app is `nsh init --template rust`: components are Rust
   functions binding signals to typed styles; no JavaScript VM ships.
   This matches the plan's rejected-alternatives table (TSX rejected by
   the operator; Rust keeps typed access to `AppState` and one toolchain).
2. **The pin is the 0.5.x lock the spike produced.** The templates
   require `"0.5.0"` and the lock resolves `nsh-core`, `nsh-base`,
   `nsh-engine`, `nsh-testing`, `nsh-platform-{macos,windows,linux}` at
   0.5.1. N1 copies that lock. The API of record is the 0.5.1 crate
   sources — nsh.dev serves 0.6.1 and its pages already differ (Modal
   styling, component `test_id`, `ToastManager::sonner`, where `input()`
   lives), so docs-only reading will mislead the port.
3. **The bridge is the plan's `AppHandle` shape, and it wires the real
   `turbogit-app` directly.** The spike proved `turbogit-app` is
   egui-free and compiles standalone as a path dependency of a crate
   outside the workspace. `AppState` stays single-threaded on the engine
   thread behind `Rc<RefCell<_>>` with one revision `Signal<u64>`;
   `read`/`update` keep borrows short; `pump()` drains events and ticks
   the poll cadence from one timer. No `Arc<Mutex>`; no executor
   injection.
4. **The staged-line gutter is the fallback, by measurement.**
   DiffView 0.5.x renders the read-only diff (unified or split, line
   selection, annotations) but has no per-line app control column, so
   staging uses the proven fallback: `virtual_list` rows with a `canvas`
   gutter and per-line checkboxes. DiffView carries the read-only pane;
   N3 pairs the two.
5. **The harness is `nsh_testing::Headless` at 1280x820 dark on a manual
   clock, with the JS stack off** (`default-features = false` on
   `nsh-engine`/`nsh-testing` — a Rust root needs no QuickJS, and the
   default features drag in the QuickJS proc-macro stack that fails to
   load under the release profile). Visual proof comes from
   `nsh test --target macos` files calling `app.screenshot`.
6. **Platform floors.** macOS 14+; Windows 10 1809+ with WebView2 for
   web views; Linux needs GTK 4.14 and glibc 2.39, i.e. Ubuntu 24.04 or
   later, plus WebKitGTK 6.0 where web views run. Linux distribution
   shape: nsh's native tarball directory built on Ubuntu 24.04 for that
   floor; AppImage or Flatpak with a pinned runtime is an N7 decision
   with CI's Linux toolchain.
7. **Two bridge rules the spike learned the hard way** (they are now
   design rules for every screen):
   - An `AppState` read in a component body (outside a binding)
     subscribes the enclosing region to every state change and rebuilds
     the subtree under the user; screens switch structure on their own
     `Signal`s and read the revision only inside bindings.
   - An input whose `value` binding echoes state on every keystroke
     races the engine's edit tracking and stalls the headless typist;
     inputs default to uncontrolled unless the app must rewrite text.

## Consequences

- N1 builds `crates/turbogit-ui-nsh` on the pinned lock, the bridge, the
  token layer from `theme.json`, and the harness feature in
  `test-support`; the plan's construct table needs two corrections it
  now has: DiffView cannot host the staging column (fallback wins), and
  `nsh run --fps` exists after all.
- The egui UI stays frozen and the old suites stay green until N7
  deletes them with their screens (migrate-callers-then-delete).
- Perf: the spike's numbers (log p95 0.064 ms, diff p95 0.171 ms, both
  under the 16.7 ms budget and the egui-trunk + 2 ms rule; egui trunk
  log p95 0.43 ms) become the baselines N2's gate compares against.
- The named gaps travel: Windows bundle and launch (N7), Linux bundle
  and launch (root swarm round, blocked-on-network at N0), screen
  capture permission on the review Mac (the operator grants it, or
  reviews from the in-process lane PNGs).

## Alternatives considered

- Stay on egui and restyle — rejected (plan, Appendix B): the goal is
  native controls and native accessibility on three desktops.
- GPUI directly — rejected: nsh already wraps it and stays available as
  a `--target gpui` fallback backend.
- Slint / Qt / Dioxus / Tauri — rejected at the goals review; each moves
  further from the language and runtime than nsh does.
- TSX for the UI — rejected by the operator; Rust keeps typed access to
  `AppState` and one toolchain.
- DiffView as the staging surface — measured, not assumed: it lacks the
  per-line control column, so the fallback is the decision.
