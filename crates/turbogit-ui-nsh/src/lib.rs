//! The nsh presentation crate: the bridge, the token layers, the styled
//! controls, and the app root. The chrome roles live in `theme.json`; the
//! data roles in [`theme`]; the controls in [`ui`]; the screens in
//! [`screens`], one per surface, filled phase by phase.

pub mod bridge;
pub mod screens;
pub mod test_ids;
pub mod theme;
pub mod ui;

use std::time::Duration;

use nsh_base::theme;
use nsh_core::element::{AnyValue, Children, PropError, PropSpec, Props, Slot, TsTypes};
use nsh_core::prelude::*;

use crate::bridge::AppHandle;
use crate::screens::feedback::{banner_layer, confirm_layer};
use crate::screens::gallery::kitchen_sink;
use crate::screens::shell::{TabSwitcher, shell_screen, status_bar};
use crate::screens::welcome::welcome_screen;
use crate::ui::toast::{ToastShared, toast_layer};

/// The app's UI set (`nsh.json` `"rust"`): what a TSX entry may import from
/// `turbogit/native`.
#[native_module("turbogit/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
    registry.register::<AppRoot>();
}

/// The chrome tokens of `theme.json`, the file the manifest names. Rust
/// roots (`cargo run`, the headless harness) mount them on the `Root`
/// themselves: a Rust-only launch reads no generated manifest.
pub fn tokens() -> nsh_core::element::Tokens {
    nsh_base::tokens_of(Some(include_str!("../theme.json"))).expect("theme.json parses")
}

/// The application's root component. `nsh run`, `nsh package` and
/// `cargo run` mount it with no props: the app over a live state with no
/// project.
#[component]
pub fn App() -> impl IntoElement {
    // `TURBOGIT_CONFIG` points the recents store at a scratch directory, so a
    // lane can drive Welcome's recent rows without touching the user's own.
    let config = std::env::var_os("TURBOGIT_CONFIG").map(std::path::PathBuf::from);
    let handle = AppHandle::new(turbogit_app::state::AppState::launch_in(None, config));
    app_shell(handle)
}

/// The props of [`TestRoot`]: the handle whose state the harness prepared.
/// `AppHandle` is Rust-only (it wraps an `Rc<RefCell<AppState>>`), so the
/// props are hand-implemented: no JS surface mounts this component, the
/// harness's `Root::new` closure is its only constructor.
#[derive(Clone)]
pub struct TestRootProps {
    pub handle: AppHandle,
}

impl PartialEq for TestRootProps {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl Props for TestRootProps {
    fn spec(_: &mut TsTypes) -> Vec<PropSpec> {
        Vec::new()
    }

    fn initial() -> Self {
        // The instance always starts here (OnceInstance calls it on the
        // engine thread, where `signal` is legal); the harness's `Root::new`
        // closure then replaces the handle with its prepared state through
        // `update_from`.
        TestRootProps {
            handle: AppHandle::new(turbogit_app::state::AppState::launch(None)),
        }
    }

    fn set(&mut self, name: &str, _: Option<AnyValue>) -> Result<bool, PropError> {
        Err(PropError(format!("unknown prop `{name}`")))
    }

    fn set_slot(&mut self, _: &str, _: Option<Slot>) -> bool {
        false
    }

    fn set_children(&mut self, _: Children) -> bool {
        false
    }
}

/// The headless root the gallery suites mount: the gallery over the
/// harness's prepared state, so `Headless` can mount it and press its
/// controls. Implemented by hand: `#[component]` pairs with
/// `#[derive(Props)]`, and these props cross no JS boundary.
pub struct TestRoot;

impl Component for TestRoot {
    type Props = TestRootProps;
    type Event = NoEvent;

    fn view(props: Self::Props, _cx: Scope<NoEvent>) -> impl IntoElement {
        gallery_shell(props.handle)
    }
}

/// The props of [`AppRoot`], the harness-mountable twin of [`App`]: the
/// same hand-implemented shape [`TestRootProps`] keeps.
#[derive(Clone)]
pub struct AppRootProps {
    pub handle: AppHandle,
}

impl PartialEq for AppRootProps {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl Props for AppRootProps {
    fn spec(_: &mut TsTypes) -> Vec<PropSpec> {
        Vec::new()
    }

    fn initial() -> Self {
        AppRootProps {
            handle: AppHandle::new(turbogit_app::state::AppState::launch(None)),
        }
    }

    fn set(&mut self, name: &str, _: Option<AnyValue>) -> Result<bool, PropError> {
        Err(PropError(format!("unknown prop `{name}`")))
    }

    fn set_slot(&mut self, _: &str, _: Option<Slot>) -> bool {
        false
    }

    fn set_children(&mut self, _: Children) -> bool {
        false
    }
}

/// The harness-mountable app root: the same shell [`App`] paints, over the
/// harness's prepared state, so `Headless` can mount it and drive the
/// screens.
pub struct AppRoot;

impl Component for AppRoot {
    type Props = AppRootProps;
    type Event = NoEvent;

    fn view(props: Self::Props, _cx: Scope<NoEvent>) -> impl IntoElement {
        app_shell(props.handle)
    }
}

/// The app shell: the pump interval at the poll cadence, the screens the
/// state's route names, the feedback layers, and the toast layer at the
/// window's edge. One interval per mounted root.
pub fn app_shell(handle: AppHandle) -> impl IntoElement {
    let pump = handle.clone();
    set_interval(Duration::from_millis(120), move || {
        pump.pump();
    });

    let toasts = ToastShared::new();
    let shown_toasts = signal(Vec::<(u64, crate::ui::toast::ToastItem)>::new());
    let advance_toasts = toasts;
    let sync_toasts = shown_toasts;
    set_interval(Duration::from_millis(50), move || {
        if advance_toasts.advance() {
            sync_toasts.set(advance_toasts.visible(3));
        }
    });

    // The state layer's notices join the shared stack and clear: one toast
    // channel, whatever queued it.
    let watch = handle.clone();
    let push_toasts = toasts;
    let sync_on_push = shown_toasts;
    set_interval(Duration::from_millis(120), move || {
        let taken = watch.update_with(|s| s.ui.toast.take());
        if let Some(toast) = taken {
            push_toasts.push_kind(toast.kind, SharedString::from(toast.message));
            sync_on_push.set(push_toasts.visible(3));
        }
    });

    let welcome_handle = handle.clone();
    let welcome_cond = handle.clone();
    let shell_handle = handle.clone();
    let banner_handle = handle.clone();
    let confirm_handle = handle.clone();
    let diff_handle = handle.clone();
    let dialog_handle = handle.clone();
    let divergence_handle = handle.clone();
    let sync_handle = handle.clone();
    let t = theme();
    view()
        .flex_1()
        .flex_col()
        .bg(t.background)
        .child(
            when(
                move || welcome_cond.read(|s| s.show_welcome()),
                move || welcome_screen(welcome_handle.clone()),
            )
            .otherwise(move || {
                let tab = signal(shell_handle.peek(|s| s.ui.tab));
                shell_screen(
                    shell_handle.clone(),
                    TabSwitcher::new(shell_handle.clone(), tab),
                )
            }),
        )
        .child(banner_layer(banner_handle))
        .child(confirm_layer(confirm_handle))
        .child(crate::screens::diff_view::diff_layer(diff_handle))
        .child(crate::screens::branch_dialogs::dialog_layer(dialog_handle))
        .child(crate::screens::divergence::divergence_layer(
            divergence_handle,
        ))
        .child(crate::screens::sync::sync_layer(sync_handle))
        .child(status_bar(handle))
        .child(
            overlay()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .items_center()
                .p_4()
                .pointer_events(PointerEvents::None)
                .child(toast_layer(toasts, shown_toasts)),
        )
}

/// The gallery root's shell: the pump interval, the gallery, and the toast
/// layer, the N1 root the gallery suites still mount.
pub fn gallery_shell(handle: AppHandle) -> impl IntoElement {
    let pump = handle.clone();
    set_interval(Duration::from_millis(120), move || {
        pump.pump();
    });

    let toasts = ToastShared::new();
    let shown_toasts = signal(Vec::<(u64, crate::ui::toast::ToastItem)>::new());
    let advance_toasts = toasts;
    let sync_toasts = shown_toasts;
    set_interval(Duration::from_millis(50), move || {
        if advance_toasts.advance() {
            sync_toasts.set(advance_toasts.visible(3));
        }
    });

    let gallery_handle = handle;
    view()
        .flex_1()
        .flex_col()
        .child(kitchen_sink(gallery_handle, toasts))
        .child(
            overlay()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .items_center()
                .p_4()
                .pointer_events(PointerEvents::None)
                .child(toast_layer(toasts, shown_toasts)),
        )
}
