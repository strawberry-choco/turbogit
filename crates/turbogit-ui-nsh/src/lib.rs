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

use nsh_core::element::{AnyValue, Children, PropError, PropSpec, Props, Slot, TsTypes};
use nsh_core::prelude::*;

use crate::bridge::AppHandle;
use crate::screens::gallery::kitchen_sink;
use crate::ui::toast::{ToastShared, toast_layer};

/// The app's UI set (`nsh.json` `"rust"`): what a TSX entry may import from
/// `turbogit/native`.
#[native_module("turbogit/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// The chrome tokens of `theme.json`, the file the manifest names. Rust
/// roots (`cargo run`, the headless harness) mount them on the `Root`
/// themselves: a Rust-only launch reads no generated manifest.
pub fn tokens() -> nsh_core::element::Tokens {
    nsh_base::tokens_of(Some(include_str!("../theme.json"))).expect("theme.json parses")
}

/// The application's root component. `nsh run`, `nsh package` and
/// `cargo run` mount it with no props: the gallery, over a live state with
/// no project.
#[component]
pub fn App() -> impl IntoElement {
    let handle = AppHandle::new(turbogit_app::state::AppState::launch(None));
    root_shell(handle)
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

/// The headless root: the gallery over the harness's prepared state, so
/// `Headless` can mount it and press its controls. Implemented by hand:
/// `#[component]` pairs with `#[derive(Props)]`, and these props cross no
/// JS boundary.
pub struct TestRoot;

impl Component for TestRoot {
    type Props = TestRootProps;
    type Event = NoEvent;

    fn view(props: Self::Props, _cx: Scope<NoEvent>) -> impl IntoElement {
        root_shell(props.handle)
    }
}

/// The shell both roots share: the pump interval at the poll cadence, the
/// gallery, and the toast layer at the window's edge. One interval per
/// mounted root.
fn root_shell(handle: AppHandle) -> impl IntoElement {
    let pump = handle.clone();
    set_interval(Duration::from_millis(120), move || {
        pump.pump();
    });

    let toasts = ToastShared::new();
    let shown_toasts = signal(Vec::<(u64, SharedString)>::new());
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
