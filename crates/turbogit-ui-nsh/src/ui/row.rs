//! The row: one list-row shell and its state grammar. Selection is a
//! composite — the row fill plus the leading rail — and ink does not
//! invert: a selected row's text keeps the color an unselected row's has.

use std::rc::Rc;

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::theme as data;

/// A row's state: which fill and rail it wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowState {
    /// Resting: transparent.
    #[default]
    Default,
    /// Under the pointer: the shared interaction band.
    Hover,
    /// The list row: the selected fill plus the brand rail at the leading
    /// edge. No list row resolves to the current-ref band; that is a
    /// selection role of its own (the branch kit's).
    Selected,
}

/// The fill each state wears.
fn row_fill(state: RowState) -> AdaptiveColor {
    let t = theme();
    match state {
        RowState::Default => AdaptiveColor::TRANSPARENT,
        RowState::Hover => t.muted,
        RowState::Selected => t.accent,
    }
}

/// A row's context-menu handler.
type ContextMenuHandler = Box<dyn Fn(&ContextMenuEvent) + 'static>;

/// A row's press handler, with the event that carries the modifiers.
type PressEventHandler = Box<dyn Fn(&PressEvent) + 'static>;

/// The props of [`row`].
pub struct RowProps {
    /// The row's content: its cells, laid out by the caller.
    pub children: Vec<AnyElement>,
    /// The state read fresh on every render, so the app's signal drives
    /// the fill and the rail.
    pub state: Rc<dyn Fn() -> RowState>,
    /// Press target: rows that select on press take it; a fact row (a hash
    /// chip's row) takes none and senses hover only.
    pub on_press: Option<Box<dyn Fn() + 'static>>,
    /// The press listener with its event, for a row that reads the modifier
    /// keys or the click count (a changes-list selection gesture).
    pub on_press_event: Option<PressEventHandler>,
    /// The context menu the row opens where a secondary press lands.
    pub on_context_menu: Option<ContextMenuHandler>,
    /// What screen readers say, when the content does not.
    pub accessibility_label: Option<SharedString>,
    /// The a11y label read fresh on every render, when it names its row.
    pub accessibility_label_of: Option<Rc<dyn Fn() -> String>>,
    pub test_id: Option<SharedString>,
}

impl RowProps {
    pub fn new() -> Self {
        RowProps {
            children: Vec::new(),
            state: Rc::new(|| RowState::Default),
            on_press: None,
            on_press_event: None,
            on_context_menu: None,
            accessibility_label: None,
            accessibility_label_of: None,
            test_id: None,
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_element());
        self
    }

    pub fn state(mut self, state: RowState) -> Self {
        self.state = Rc::new(move || state);
        self
    }

    /// The state read fresh on every render.
    pub fn state_of(mut self, state: impl Fn() -> RowState + 'static) -> Self {
        self.state = Rc::new(state);
        self
    }

    pub fn on_press(mut self, on_press: impl Fn() + 'static) -> Self {
        self.on_press = Some(Box::new(on_press));
        self
    }

    /// The press listener with its event: the modifier keys and the click
    /// count a selection gesture reads.
    pub fn on_press_event(mut self, on_press: impl Fn(&PressEvent) + 'static) -> Self {
        self.on_press_event = Some(Box::new(on_press));
        self
    }

    pub fn on_context_menu(
        mut self,
        on_context_menu: impl Fn(&ContextMenuEvent) + 'static,
    ) -> Self {
        self.on_context_menu = Some(Box::new(on_context_menu));
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    /// The a11y label read fresh on every render.
    pub fn accessibility_label_of(mut self, label: impl Fn() -> String + 'static) -> Self {
        self.accessibility_label_of = Some(Rc::new(label));
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

impl Default for RowProps {
    fn default() -> Self {
        Self::new()
    }
}

/// A row: the fill from its state, the rail paint on a selected row. The
/// rail is paint, not layout — reserving its width as padding would shift
/// every row's text origin.
pub fn row(props: RowProps) -> View {
    let t = theme();
    let hover_fill = t.muted;
    let rail_fill = t.primary;
    let hover_state = props.state.clone();
    let rail_state = props.state.clone();
    let mut content = view()
        .flex_row()
        .items_center()
        .min_h(px(data::FILE_ROW_HEIGHT))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(move || row_fill((props.state)()))
        .hover(move |s| {
            if (hover_state)() == RowState::Default {
                s.bg(hover_fill)
            } else {
                s
            }
        });
    for child in props.children {
        content = content.child(child);
    }
    let pressable = props.on_press.is_some() || props.on_press_event.is_some();
    let mut row = view()
        .flex_col()
        .relative()
        .child(content)
        .child(dynamic(move || match (rail_state)() {
            RowState::Selected => view()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(data::RAIL_WIDTH))
                .bg(rail_fill)
                .into_element(),
            _ => view().into_element(),
        }));
    if pressable {
        // A row that takes a press is a native accessibility element (the
        // platform host materializes pressed roles, not bare views).
        row = row.role(AccessibilityRole::Button);
    }
    if let Some(on_press) = props.on_press {
        row = row.on_press(on_press);
    }
    if let Some(on_press_event) = props.on_press_event {
        row = row.on_press(move |event: &PressEvent| on_press_event(event));
    }
    if let Some(on_context_menu) = props.on_context_menu {
        row = row.on_context_menu(on_context_menu);
    }
    match props.accessibility_label_of {
        Some(read) => {
            row = row.label(move || SharedString::from(read()));
        }
        None => {
            if let Some(label) = props.accessibility_label.clone() {
                row = row.label(label);
            }
        }
    }
    if let Some(id) = props.test_id.clone() {
        row = row.test_id(&id);
    }
    row
}
