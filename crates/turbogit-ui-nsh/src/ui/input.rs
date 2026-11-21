//! The input and the text area, over the core's Input element: the input is
//! uncontrolled (read through `on_change`) unless the app must rewrite text,
//! the headless typist stalls a value binding that echoes state on every
//! keystroke (the n0 spike's rule).

use std::rc::Rc;

use nsh_base::{input as base_input, theme};
use nsh_core::prelude::*;

use super::ControlSize;
use crate::theme as data;

/// The props of [`input`] and [`text_area`].
pub struct InputProps {
    /// The first text; the input keeps its own value from here on. A
    /// controlled `value` needs the echo-race review the bridge rule warns
    /// about, so the wrapper offers the uncontrolled form only.
    pub default_value: SharedString,
    /// Read at mount instead of `default_value`, so a remount picks up the
    /// text the app rewrote to.
    pub default_value_of: Option<Rc<dyn Fn() -> String>>,
    /// A generation the input remounts on: the app-rewrite path (the clone
    /// URL resets on success). Between bumps the input stays uncontrolled;
    /// the fresh mount takes `default_value_of`'s current reading.
    pub remount: Option<Rc<dyn Fn() -> u64>>,
    /// Shown while empty.
    pub placeholder: Option<SharedString>,
    /// Several lines that grow with the text.
    pub multiline: bool,
    /// Cannot be changed, but can be selected and copied.
    pub readonly: bool,
    /// Hides what is typed.
    pub masked: bool,
    /// No focus, no typing.
    pub disabled: bool,
    /// The text change handler, the new text.
    pub on_change: Option<super::OnChange<str>>,
    /// The return handler (single line).
    pub on_submit: Option<super::OnChange<str>>,
    /// What screen readers say.
    pub accessibility_label: Option<SharedString>,
    pub size: ControlSize,
    /// The id UI tests type into.
    pub test_id: Option<SharedString>,
}

impl InputProps {
    pub fn new() -> Self {
        InputProps {
            default_value: SharedString::default(),
            default_value_of: None,
            remount: None,
            placeholder: None,
            multiline: false,
            readonly: false,
            masked: false,
            disabled: false,
            on_change: None,
            on_submit: None,
            accessibility_label: None,
            size: ControlSize::default(),
            test_id: None,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn default_value(mut self, default_value: impl Into<SharedString>) -> Self {
        self.default_value = default_value.into();
        self
    }

    /// The mount-time text reader, paired with [`InputProps::remount`].
    pub fn default_value_of(mut self, read: impl Fn() -> String + 'static) -> Self {
        self.default_value_of = Some(Rc::new(read));
        self
    }

    /// The generation the input remounts on.
    pub fn remount(mut self, generation: impl Fn() -> u64 + 'static) -> Self {
        self.remount = Some(Rc::new(generation));
        self
    }

    pub fn multiline(mut self, multiline: bool) -> Self {
        self.multiline = multiline;
        self
    }

    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, on_change: impl Fn(&str) + 'static) -> Self {
        self.on_change = Some(Box::new(on_change));
        self
    }

    pub fn on_submit(mut self, on_submit: impl Fn(&str) + 'static) -> Self {
        self.on_submit = Some(Box::new(on_submit));
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

impl Default for InputProps {
    fn default() -> Self {
        Self::new()
    }
}

/// A text edit's handler.
type TextHandler = Rc<dyn Fn(&str)>;

/// The owned shape one input build reads. The handlers ride in `Rc`s so a
/// remount can rebuild from the same `Rc`s without moving them.
struct InputCore {
    default_value: SharedString,
    default_value_of: Option<Rc<dyn Fn() -> String>>,
    remount: Option<Rc<dyn Fn() -> u64>>,
    placeholder: Option<SharedString>,
    readonly: bool,
    masked: bool,
    disabled: bool,
    multiline: bool,
    on_change: Option<TextHandler>,
    on_submit: Option<TextHandler>,
    accessibility_label: Option<SharedString>,
    size: ControlSize,
}

impl InputCore {
    fn build(&self, generation: u64) -> Input {
        let t = theme();
        let (pad_x, pad_y) = match self.size {
            ControlSize::Compact => (
                data::DENSITY_DENSE_BUTTON_PAD_X,
                data::DENSITY_DENSE_BUTTON_PAD_Y,
            ),
            ControlSize::Regular => (data::BUTTON_PAD_X, data::BUTTON_PAD_Y),
            ControlSize::Large => (data::BUTTON_PAD_X + 2.0, data::BUTTON_PAD_Y + 2.0),
        };
        let height = match self.size {
            ControlSize::Compact => data::DENSITY_COMMAND_ROW_HEIGHT + 8.0,
            _ => data::FILE_ROW_HEIGHT,
        };
        let default_text = self
            .default_value_of
            .as_ref()
            .map(|read| read())
            .unwrap_or_else(|| self.default_value.to_string());
        let mut input = base_input()
            .default_value(default_text)
            .readonly(self.readonly)
            .masked(self.masked)
            .disabled(self.disabled)
            .multiline(self.multiline)
            .flex_1()
            .text_color(t.foreground);
        if self.multiline {
            input = input.min_h(px(data::GROUP_ROW_HEIGHT * 3.0));
        }
        if let Some(placeholder) = self.placeholder.clone() {
            input = input.placeholder(placeholder.to_string());
        }
        if let Some(a11y) = self.accessibility_label.clone() {
            input = input.label(a11y);
        }
        if let Some(on_change) = self.on_change.clone() {
            input = input.on_change(move |text: &str| on_change(text));
        }
        if let Some(on_submit) = self.on_submit.clone() {
            input = input.on_submit(move |text: &str| on_submit(text));
        }
        let muted = t.muted_foreground;
        input
            .placeholder_color(move || if dark() { muted.dark } else { muted.light })
            .text_size(px(text_size_of(self.size)))
            .key(format!("input:{generation}"))
            .flex_row()
            .items_center()
            .rounded(px(data::CONTROL_RADIUS))
            .px(px(pad_x))
            .py(px(pad_y))
            .h(px(height))
            .bg(t.background)
            .border(px(1.0))
            .border_color(t.input)
            .focus_visible(|s| s.ring(px(data::RAIL_WIDTH)).ring_color(t.ring))
            .disabled_style(|s| s.opacity(0.45))
    }
}

fn core_of(mut props: InputProps, multiline: bool) -> InputCore {
    InputCore {
        default_value: props.default_value.clone(),
        default_value_of: props.default_value_of.clone(),
        remount: props.remount.clone(),
        placeholder: props.placeholder.clone(),
        readonly: props.readonly,
        masked: props.masked,
        disabled: props.disabled,
        multiline,
        on_change: props
            .on_change
            .take()
            .map(|f| Rc::new(move |text: &str| f(text)) as Rc<dyn Fn(&str)>),
        on_submit: props
            .on_submit
            .take()
            .map(|f| Rc::new(move |text: &str| f(text)) as Rc<dyn Fn(&str)>),
        accessibility_label: props.accessibility_label.clone(),
        size: props.size,
    }
}

fn text_size_of(size: ControlSize) -> f32 {
    match size {
        ControlSize::Compact => data::TYPE_CONTROL,
        ControlSize::Regular => data::TYPE_BODY,
        ControlSize::Large => data::TYPE_DETAIL_TITLE,
    }
}

fn host_of(
    host: impl IntoElement,
    test_id: Option<SharedString>,
    wrap_flex: bool,
) -> impl IntoElement {
    let host = if wrap_flex {
        view().flex_1().child(host)
    } else {
        view().child(host)
    };
    match test_id {
        Some(id) => view().test_id(&id).child(host),
        None => view().child(host),
    }
}

/// A single-line input. The nsh input is a primitive and takes a test id on
/// itself; the wrapper still names it, so callers need one shape.
pub fn input(props: InputProps) -> impl IntoElement {
    let test_id = props.test_id.clone();
    let core = core_of(props, false);
    let host = match core.remount.clone() {
        Some(generation) => {
            let core = core;
            view().child(dynamic(move || core.build(generation())))
        }
        None => view().child(core.build(0)),
    };
    host_of(host, test_id, false)
}

/// A multiline text area: the commit message editor, a conflict's note. It
/// grows with the text; return adds a line.
pub fn text_area(props: InputProps) -> impl IntoElement {
    let test_id = props.test_id.clone();
    let core = core_of(props, true);
    let host = match core.remount.clone() {
        Some(generation) => {
            let core = core;
            view()
                .flex_1()
                .child(dynamic(move || core.build(generation())))
        }
        None => view().flex_1().child(core.build(0)),
    };
    host_of(host, test_id, true)
}
