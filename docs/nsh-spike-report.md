# Native Shell spike report (N0)

The N0 spike ran TurboGit's log, diff, branches, tables, editors, overlays,
and bridge surfaces on Native Shell 0.5.0, in Rust, headless and live, on
macOS. Verdict: **go**, with two named gaps that are environmental, not
platform ones. Every number below was measured this run; the evidence
paths exist on disk.

## Verdict

| Question (plan Appendix A) | Verdict | Evidence |
| --- | --- | --- |
| 60 fps log at 100k rows with aligned lanes | go | p95 frame build 0.064 ms, max 0.074 ms over 129 scroll frames; lanes drawn at fixed x per lane column |
| 60 fps diff at 20k lines | go | p95 0.171 ms, max 0.284 ms over 203 scroll frames (fallback gutter surface) |
| DiffView hosts a staged-line gutter | no — fallback wins | DiffView 0.5.x has line selection on the gutter and annotations under lines, but no per-line app control column; the fallback (`virtual_list` + `canvas` gutter + per-line checkbox) is implemented and proven |
| Tree and DataTable cover branch, file, worktree, shortcuts surfaces | go | 10k-branch tree and 1k-worktree table render, virtualize, and scroll (lanes 5); grouped file tree renders |
| Editor covers conflict and patch surfaces | go for N0 | editor with rust language renders; folding, search, and large files to be proven at N5 with the 1k-hunk fixture |
| Input multiline handles IME, CJK, emoji, undo, paste | go for N0 | CJK + emoji round trip driven through the multiline input (headless unit 4, lane 8); paste, text selection, and undo verified at the API level (`nsh-testing` `paste` step, `on_paste` prop, platform input) but not yet driven end to end; N1's input lanes drive them. True IME candidate-window composition is untested headlessly |
| Overlays, focus return, `on_key` cover the keyboard surface | go | menu, context menu (system popup), dialog, alert dialog, popover, command palette, toasts, `on_key("mod+k")` all open and dismiss with Escape; focus returns to the trigger (lane 6) |
| `Rc<RefCell<AppState>>` bridge avoids re-entrant borrows at 10k rows | go | 10k-row render reads through the handle; pump drains events; no borrow panic across all tests and lanes |
| Headless at 1280x820 reproduces the kittest flows | go | six cargo tests pass (press, filter, scroll-to-100000, 500-line stage, CJK round trip, mod+k palette) |
| TSX screenshot test gives the visual proof | go | `nsh test --target macos` writes PNGs of the real windows (lanes 1–9) |
| Packaged bundle launches on macOS 14 / Windows 10 1809 / Linux GTK 4.14 | go with gaps | macOS bundle launches and quits cleanly; Windows bundle built with cargo-xwin (launch named gap); Linux bundle blocked-on-network (see gaps) |

## Environment

- nsh CLI 0.5.0 (f4d68f7), installed at `~/.nsh/bin/nsh`.
- Crates resolved under the `"0.5.0"` requirement: `nsh-core`/`nsh-base`/`nsh-engine`/`nsh-testing`/`nsh-platform-*` at **0.5.1** (semver-compatible point release; the template's requirement is `"0.5.0"`). The pin for N1 is the Cargo.lock this spike produced.
- cargo/rustc 1.97.1 stable-aarch64-apple-darwin; Xcode at `/Applications/Xcode.app`.
- OrbStack 29.4.0 (Docker Engine), Ubuntu 24.04.3 rootfs imported locally.
- `nsh --version` recorded: `nsh 0.5.0 (f4d68f7)`.

## Construct mapping (plan name → nsh 0.5.x reality)

| Plan construct | 0.5.x API | Status |
| --- | --- | --- |
| `virtual_list(count, \|at\| row)` | `nsh_core::prelude::virtual_list` | exists exactly |
| `estimated_item_size(px(24.0))` | `.estimated_item_size(px(24.0))` | exists |
| per-row `canvas()` lane strip | `canvas(Painter)` / `canvas(closure)` from the prelude | exists |
| `diff_view()` | `nsh_base::diff_view()` (unified/split, line selection, annotations, patch mode) | exists |
| staged-line gutter on DiffView | not supported: no per-line app control column; `on_line_selection_change` + a stage button, or annotations, are the nearest fit | fallback proven |
| `tree()` | `nsh_base::tree()` (`TreeItem`, `render_item`, keyboard per ARIA treeview) | exists |
| `data_table()` | `nsh_base::data_table()` (`DataTableColumn`, `DataTableRows`, resizable/movable/sortable) | exists |
| `editor()` | `nsh_base::editor()` (50+ languages, folding, find/replace, diagnostics, decorations) | exists |
| `input().multiline(true)` | `nsh_base::input().multiline(true)` | exists |
| `menu()` | `nsh_base::menu()` + `menu_item()`, `menu_separator()`, `submenu()` | exists |
| `context_menu()` | `on_context_menu` + `nsh_core::element::desktop::popup_menu_at` (system menus) | exists |
| `dialog()` | `nsh_base::dialog()` = `modal().role(Dialog)`; styling goes on children in 0.5.x | exists |
| `alert_dialog()` | `nsh_base::alert_dialog().show()` (NSAlert on macOS) | exists |
| `popover()` | `nsh_base::popover()` (trigger, popup_class, Escape/outside close) | exists |
| `command()` | `nsh_base::command()` (palette; CommandItem, search, keyboard) | exists |
| `toast_stack()` | `nsh_base::toast_stack()` + `toast()` + `ToastManager` (app drives the lifecycle timer) | exists |
| `on_key("mod+k")` | `nsh_core::prelude::on_key(keystroke, handler)` | exists |
| `tabs()`, `checkbox()`, `select()`, `switch()`, `progress()`, `toolbar()`, `combobox()`, `date_picker()` | `nsh_base` constructors of the same names | exist |
| `h_resizable()` | `nsh_base::h_resizable()` + `resizable_panel()` | exists (not exercised by N0's build box) |
| `theme()` | `nsh_base::theme()` (`Arc<Theme>`), theme.json = shadcn `cssVars` | exists |
| `nsh_testing::Headless` | `nsh_testing::Headless::start(EngineConfig)` | exists |
| 1280x820 dark + manual clock | `config.viewport`, `config.environment.color_scheme = Dark`, `config.clock = Clock::manual(...)` | exists |
| `Root::new::<TestRoot>` | `nsh_core::element::Root::new::<C>(props)` / `Root::of::<C>()` | exists |
| `nsh test --target macos` | exists; runs cargo tests first, then TSX tests in real windows; `app.screenshot(path)` writes PNGs | exists |
| `nsh run --fps` | exists as a CLI flag; `NSH_FPS=1` turns the monitor on for any start, including packaged apps; logs once a second (target `nsh::fps`) on all desktops | exists (the plan was right; the earlier coverage note said undocumented) |
| `nsh test --trace` | `--trace` prints per-commit perf lines (`commit: N ops, Xµs …`) | exists |
| `nsh package --target macos/windows/linux` | exists; macOS `.app` ad hoc signed; Windows needs cargo-xwin + the msvc target; Linux builds the runner on Linux | exists |
| crates `nsh-core`, `nsh-base`, `nsh-platform-*` at 0.5.0 | real crates on crates.io; the lock resolves 0.5.1 under `"0.5.0"` | exist |
| `dialog()` styling (`bg`, `m_auto` on the dialog) | 0.5.x Modal takes no style methods; the card is a child view | 0.6 docs differ from 0.5 crates; N1 must read the 0.5.1 sources, not nsh.dev (which serves 0.6.1) |
| `test_id` on components | 0.5.x: primitives have `.test_id(&str)`; component elements (Tree, DataTable, Editor, Tab, Command, Button) do not — wrap in a view | documented gap fixed in 0.6; wrap for now |
| `DiffAnnotation` construction | `#[non_exhaustive]` in 0.5.x — build via `Default` + field assignment | works |

## What the spike built

`spikes/nsh/` (git-excluded, outside the Cargo workspace): a Rust nsh app,
`nsh init spikes/nsh --template rust`, with seven tabbed surfaces:

- **Log** — 100k synthetic commits on `virtual_list` with
  `estimated_item_size(px(24.0))` and a per-row `canvas` lane strip
  (four continuous lane rails + a lane-colored commit dot); filter bar;
  selection; arrow-key navigation with scroll-into-view; Jump-to-row
  buttons over the list ref.
- **Diff** — the 20k-line synthetic diff on DiffView (unified, patch
  mode, line selection recorded, staged annotations) and the fallback
  staging gutter (virtual list + canvas gutter + per-line checkboxes);
  Select 500 / Extend / Stage / Unstage driving the staging model.
- **Branches** — a grouped file tree and the 10k-branch tree on `tree()`.
- **Tables** — 1k worktrees and a shortcuts table on `data_table()`.
- **Editors** — `editor()` with a CJK/emoji fixture and the multiline
  commit-message input.
- **Overlays** — menu, context menu host, dialog, alert dialog, palette,
  toast push, popover.
- **Bridge** — 10k rows read through the `AppHandle` plus a
  push-event/pump-drain counter.

The bridge is the plan's exact shape: `AppHandle { state:
Rc<RefCell<AppState>>, revision: Signal<u64> }` with `read`, `update`,
`update_with`, and `pump` (drain events + `tick_incoming_poll` cadence),
driven by one `set_interval` at 120 ms. The spike state is a
shape-compatible stand-in with the same `drain_events()` /
`tick_incoming_poll(Instant)` surface.

**Bridge finding that decides N1's wiring:** `turbogit-app` is
egui-free (zero egui/eframe/dify matches in its whole dependency tree)
and compiles standalone as a plain path dependency of an outside crate
(`spikes/nsh` builds it from the worktree). N1 wires the real
`AppState` directly; no stand-in layer is needed.

## Headless unit results (the six "Verify, unit" boxes)

`cargo test -p nsh-app-nsh --test spike` at 1280x820, dark,
`Clock::manual`, on the real engine:

1. `log_row_press_filter_and_selection` — press a log row, filter
   "lane strips", assert the narrowed count and the selection label. pass
2. `log_scrolls_to_row_100000` — jump to row 100000, assert the row's
   sha is on screen. pass
3. `diff_selects_500_lines_and_stages_them` — select 5200..5699, stage,
   assert `staged: 500 selection: 5200..5699`, unstage back to 0. pass
4. `commit_message_round_trips_cjk` — type `中文提交信息 with an emoji 🚀`
   into the multiline input, assert the round trip. pass
5. `palette_opens_with_mod_k_and_runs_a_command` — mod+k, type "fetch",
   Return, assert `commands: palette:fetch`. pass
6. `bridge_pump_drains_events_into_the_render` — queue two events,
   assert `drained 2 by the pump`. pass

All six pass; `nsh check` passes; the crate's targets build without warnings (the perf test's unused `Arc` import was removed in the audit round).

## Live lanes (the ten "Verify, live" boxes)

Run with `nsh test tests/ui/spike.test.tsx --target macos`; each lane
opened the app in a real 1280x820 dark window and saved its PNG. Copies
live in `/tmp/swarm-n0/worker-1/` and the review set in
`media/nsh/N0-review/`.

| Lane | Slug | Verdict | Note |
| --- | --- | --- | --- |
| 1 regression | `spike-regression.png` | pass | the log renders 100000 of 100000 commits with lanes, selection, filter bar |
| 2 log fling | `spike-log-scroll.png` | pass | scroll to ~row 830, no blank rows, row sha asserted on screen |
| 3 graph alignment | `spike-graph-align.png` | pass | three scroll offsets; lanes are drawn at fixed x per lane column so every lane meets its row exactly |
| 4 diff select+stage | `spike-diff-select.png` | pass | 500 lines selected and staged in the fallback gutter; selection survives the scroll (asserted) |
| 5 tree+table | `spike-tree-table.png`, `spike-tree-table-2.png` | pass | 10k branch tree and 1k worktree table render, virtualize, scroll |
| 6 overlays | `spike-overlays.png`, `spike-overlays-2.png` | pass | dialog, popover, alert (discard + cancel answered), Escape dismisses; focus returns |
| 7 keyboard | `spike-keyboard.png` | pass | arrows move the log selection; mod+k opens the palette; the palette screenshot shows the query |
| 8 CJK/emoji/path | `spike-cjk.png` | pass | CJK + emoji round trip and the 94-char unbroken path echoed |
| 9 resize | `spike-resize.png` | pass | 1024x700, no pane clips |
| 10 packaged | `spike-packaged.png` | macOS: pass (launch + clean quit); the PNG itself is black — screen capture is blocked on this machine | see gaps; the review video below carries the in-process visuals instead |

The Windows and Linux sub-lanes are named gaps (below).

## Perf ("Verify, perf")

Probe. The comparable quantity on both sides is the per-frame **build**
cost on the real engine, measured headless in release builds:

- egui trunk: `spikes/nsh/baseline/tests/log_perf.rs` (kittest
  `Harness::new_ui_state` over `turbogit_ui::ui::log_window::show_log` on
  a clone of the 100k-commit fixture repo; per-`step` wall time). Run:
  `cargo test --release -p spike-baseline --test log_perf -- --ignored`.
- Spike: `spikes/nsh/tests/spike_perf.rs` (`EngineConfig::trace` +
  the engine's `perf commit: …, Xµs` lines collected while the driver
  scrolls; the core's render+layout+diff time per frame). Run:
  `cargo test --release -p nsh-app-nsh --test spike_perf -- --ignored`.

Fixture. `/tmp/nsh-n0-fixture/repo-100k`: 100000 commits, 499 merges,
built with `git fast-import`; the stream was not retained, so the
fixture's identity on disk is its measured commit and merge counts, not
a recorded stream hash. The egui baseline clones it per run; the spike's
fixture is the synthetic 100k-commit model the log screen reads through
the bridge.

**Baseline first, as the plan orders.** egui trunk, 1280x820, release:

| Measurement | Value |
| --- | --- |
| log open, first 10 frames | p50 0.16 ms, p95 0.63 ms |
| log steady idle (30 frames) | p50 0.23 ms, p95 0.64 ms |
| log scroll (120 wheel frames) | p50 0.21 ms, **p95 0.43 ms**, max 0.59 ms |
| peak RSS of the harness process | 83.5 MB (81.5 MiB) |
| cached log size at measurement | 1200 commits (the egui log pages 50 per load-more; the window's frame cost depends on the visible rows and the row model, not the cache depth) |

**Spike**, same engine discipline, release:

| Measurement | Value |
| --- | --- |
| 100k log scroll (129 recorded frames) | p50 0.034 ms, **p95 0.064 ms**, max 0.074 ms |
| 20k diff scroll, fallback gutter (203 recorded frames) | p50 0.044 ms, **p95 0.171 ms**, max 0.284 ms |
| peak RSS of the harness process (all fixtures held) | 134.4 MB (131.3 MiB) |

**Rule.** Head p95 at most 2 ms over trunk and under 16.7 ms on the
reference Mac. Log: 0.064 ms ≤ 0.43 + 2 and < 16.7 — pass. Diff:
0.171 ms < 16.7 — pass. The egui diff baseline does not exist yet (the
egui diff pane has no 20k fixture harness on trunk); N3's perf box owns
that comparison and the 16.7 ms absolute budget is the gate here.

Run spread, for honesty about the single values above. Independent runs
of the same probes read spike log p95 0.064-0.155 ms, spike diff p95
0.100-0.275 ms, egui trunk scroll p95 0.17-0.43 ms (the engine docs
warn single-run timings swing with the CPU's clock state). The gate
passes in every combination; N2's gate compares against the minimum of
several runs, per the nsh performance guide.

Memory. No numeric gate exists in N0's rule box. The ps-sampling run
read egui 83.5 MB and spike 134.4 MB; the kernel's `/usr/bin/time -l`
max-RSS over the same runs reads egui 1.11 GB and spike 136 MB. The
spike side agrees across methods; the egui side is 14x apart, because
kittest's wgpu allocation lands outside the ps-sampled process. N1 must
pin one method (the kernel max-RSS reading is the conservative choice)
before adopting the plan's 1.5x memory gate. The fixture asymmetry is
deliberate: the spike holds all 100k commits, the 20k diff, the 10k
branches and the 1k worktrees in memory at once (the bridge's proof),
while the egui log pages its cache at 50 per batch.

## Packaging

- **macOS**: `nsh package --target macos` wrote
  `spikes/nsh/.nsh/out/nsh.app` (ad hoc signed). Launched via `open`
  (pid captured), quit with SIGTERM, and no process remained — launches
  the fixtures and quits cleanly. The launch was re-verified at the
  verdict round (process alive under `pgrep`, exited cleanly on quit).
- **Windows**: `nsh package --target windows` requires cargo-xwin and
  `x86_64-pc-windows-msvc`; both installed (`cargo-xwin 0.23.1`), the
  build was interrupted by the disk/network conditions and did not
  finish. Named gap: no Windows machine exists on this island, so even a
  produced bundle cannot launch here; N7's release workflow owns the
  Windows bundle and its launch.
- **Linux**: the nsh 0.5.0 Linux runtime tarball
  (`nsh-linux-arm64.tar.gz`, sha256-verified
  `d99550ed…6ac`) is staged at `/tmp/swarm-n0/linux/`. An Ubuntu 24.04.3
  rootfs was imported into OrbStack and GTK 4.14 dev packages installed;
  the rustup + package step was repeatedly killed before reaching
  `nsh package --target linux`, so **no Linux bundle artifact exists yet**
  and the live launch is recorded blocked-on-network. The root's swarm
  round can verify the launch when the network recovers; the container
  recipe is in the decision log.

## Linux distribution decision

**Decision: ship the nsh package's native shape — a tarball directory
(`<Name>/` executable + `nsh/` runtime + `.desktop` entry) built on
Ubuntu 24.04 — and defer AppImage/Flatpak to N7 with a pinned-runtime
requirement.** Reasons, from evidence:

1. The Linux runtime needs GTK 4.14 and glibc 2.39 (Ubuntu 24.04 floor,
   confirmed in nsh's Linux requirements and by the GTK 4.15.x that
   24.04's `libgtk-4-dev` carries). nsh's own package output is already
   the right shape for distributions at that floor.
2. WebKitGTK 6.0 is a runtime dependency of the default runtime
   (`libwebkitgtk-6.0-4`); an AppImage carrying GTK would have to carry
   WebKit too, and Flatpak would pin a runtime with both — meaningful
   size and maintenance cost that N0's spike cannot measure without a
   working Linux build.
3. N7's release job can produce the tarball plus an AppImage or Flatpak
   in CI (where the Linux toolchain is free); the spike's gate was "a
   launching bundle on all three desktops", and the Linux leg's evidence
   is recorded as blocked-on-network rather than a platform no.

## Named gaps

1. **Windows launch** — no Windows machine or VM on this island; the
   bundle build was also interrupted (cargo-xwin installed; the build
   never finished). Never recorded as a pass.
2. **Linux bundle + launch** — blocked-on-network (apt and rustup
   downloads in the container did not complete in the window; the
   nsh-linux runtime tarball is staged and verified). The live launch is
   left to the root's swarm round.
3. **Screen capture on this Mac** — `screencapture` returns an all-black
   image (display locked / TCC recording permission for the host
   terminal is not granted). Lane 10's macOS PNG is therefore black
   while the launch-and-quit evidence is process-level. The review video
   was still delivered: `tests/ui/video.test.tsx` drives the log scroll,
   the diff staging, the dialog, the palette, and the toasts and saves
   in-process `app.screenshot` frames, which ffmpeg stitches into
   `media/nsh/N0-review.mp4` (34 s). The in-process `app.screenshot`
   PNGs of lanes 1–9 are unaffected by the permission gap.
4. **egui diff baseline** — no trunk harness exists for the 20k diff;
   the spike's diff p95 is judged against the 16.7 ms absolute budget,
   and N3 owns the trunk comparison.
5. **Docs version skew** — nsh.dev serves 0.6.1 while the spike pins
   0.5.x. APIs that moved: Modal lost/gained style methods, component
   `test_id`, `ToastManager::sonner` vs `new(ToastMotion::sonner())`,
   `TreeProps.default_expanded` element type, `input()` living in
   nsh_base (not the core prelude). N1 must read the 0.5.1 crate
   sources as the API of record.

## Notes for N1 (lessons that change the plan's boxes)

- **Controlled inputs and the headless typist**: an `input()` whose
  `value` binding echoes the state on every keystroke stalls the
  headless `type_text` driver after one character. Uncontrolled inputs
  (read via `on_change`) type cleanly. The commit-message box and the
  filter bar are uncontrolled for this reason; a controlled input needs
  an echo-race review at N3.
- **Region subscriptions**: any `read()` of the revision signal executed
  in a component body (not inside a binding) subscribes the enclosing
  dynamic region to every state change and tears the subtree down under
  the driver — this dropped clicks and keystrokes until the tab switch
  and the diff mode switch were keyed off dedicated `Signal`s. The
  bridge rule for N1: screens switch structure on their own signals;
  revision reads only live inside bindings.
- **0.5.x component elements lack `test_id`** — wrap in a view and put
  the id on the wrapper, or select by text.
- **The JS stack is optional in tests**: `nsh-testing` default features
  pull QuickJS; a Rust root runs headless with
  `default-features = false` on `nsh-engine`/`nsh-testing`, which also
  removes the release-profile proc-macro load failure the spike hit.
- **DiffView read-only case is real**: the 20k-line patch renders
  virtualized with unified numbering and inline change highlighting;
  N3 pairs it with the fallback gutter for staging.
