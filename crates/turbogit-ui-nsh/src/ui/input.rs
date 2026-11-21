//! The input and the text area, over the core's Input element: the input is
//! uncontrolled (read through `on_change`) unless the app must rewrite text,
//! the headless typist stalls a value binding that echoes state on every
//! keystroke (the n0 spike's rule).

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

fn text_size_of(size: ControlSize) -> f32 {
    match size {
        ControlSize::Compact => data::TYPE_CONTROL,
        ControlSize::Regular => data::TYPE_BODY,
        ControlSize::Large => data::TYPE_DETAIL_TITLE,
    }
}

/// The input's frame: the input border, the muted placeholder ink, the ring
/// when the keyboard put the focus there.
fn frame(input: Input, size: ControlSize) -> Input {
    let t = theme();
    let (pad_x, pad_y) = match size {
        ControlSize::Compact => (
            data::DENSITY_DENSE_BUTTON_PAD_X,
            data::DENSITY_DENSE_BUTTON_PAD_Y,
        ),
        ControlSize::Regular => (data::BUTTON_PAD_X, data::BUTTON_PAD_Y),
        ControlSize::Large => (data::BUTTON_PAD_X + 2.0, data::BUTTON_PAD_Y + 2.0),
    };
    let height = match size {
        ControlSize::Compact => data::DENSITY_COMMAND_ROW_HEIGHT + 8.0,
        _ => data::FILE_ROW_HEIGHT,
    };
    let muted = t.muted_foreground;
    input
        .placeholder_color(move || if dark() { muted.dark } else { muted.light })
        .text_size(px(text_size_of(size)))
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

fn host_of(input: Input, test_id: Option<SharedString>, wrap_flex: bool) -> impl IntoElement {
    let host = if wrap_flex { view().flex_1() } else { view() };
    match test_id {
        Some(id) => host.test_id(&id).child(input),
        None => host.child(input),
    }
}

/// A single-line input. The nsh input is a primitive and takes a test id on
/// itself; the wrapper still names it, so callers need one shape.
pub fn input(props: InputProps) -> impl IntoElement {
    let t = theme();
    let size = props.size;
    let InputProps {
        default_value,
        placeholder,
        multiline: _,
        readonly,
        masked,
        disabled,
        on_change,
        on_submit,
        accessibility_label,
        size: _,
        test_id,
    } = props;
    let mut input = base_input()
        .default_value(default_value.to_string())
        .readonly(readonly)
        .masked(masked)
        .disabled(disabled)
        .flex_1()
        .text_color(t.foreground);
    if let Some(placeholder) = placeholder {
        input = input.placeholder(placeholder.to_string());
    }
    if let Some(a11y) = accessibility_label {
        input = input.label(a11y);
    }
    if let Some(on_change) = on_change {
        input = input.on_change(move |text: &str| on_change(text));
    }
    if let Some(on_submit) = on_submit {
        input = input.on_submit(move |text: &str| on_submit(text));
    }
    let input = frame(input, size);
    host_of(input, test_id, false)
}

/// A multiline text area: the commit message editor, a conflict's note. It
/// grows with the text; return adds a line.
pub fn text_area(props: InputProps) -> impl IntoElement {
    let t = theme();
    let size = props.size;
    let InputProps {
        default_value,
        placeholder,
        multiline: _,
        readonly,
        masked: _,
        disabled,
        on_change,
        on_submit,
        accessibility_label,
        size: _,
        test_id,
    } = props;
    let mut input = base_input()
        .default_value(default_value.to_string())
        .multiline(true)
        .readonly(readonly)
        .disabled(disabled)
        .flex_1()
        .min_h(px(data::GROUP_ROW_HEIGHT * 3.0))
        .text_color(t.foreground);
    if let Some(placeholder) = placeholder {
        input = input.placeholder(placeholder.to_string());
    }
    if let Some(a11y) = accessibility_label {
        input = input.label(a11y);
    }
    if let Some(on_change) = on_change {
        input = input.on_change(move |text: &str| on_change(text));
    }
    if let Some(on_submit) = on_submit {
        input = input.on_submit(move |text: &str| on_submit(text));
    }
    let input = frame(input, size);
    host_of(input, test_id, true)
}
