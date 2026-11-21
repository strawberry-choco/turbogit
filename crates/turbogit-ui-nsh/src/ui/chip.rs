//! The chip: a compact fact marker. A chip is a filled, bounded fact
//! marker; a colored label is not one, and state is never a chip — the
//! repository-state map paints text and dots, chips stay neutral.

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::theme as data;

/// The chip's variant: which neutral fact it marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChipVariant {
    /// A plain fact: muted fill, ink.
    #[default]
    Neutral,
    /// A fact on the raised surface (a pane header's count).
    Raised,
    /// The selected fact: the accent band.
    Accent,
}

/// The props of [`chip`].
pub struct ChipProps {
    /// The chip's text: a count, a ref name, a scope.
    pub label: SharedString,
    pub variant: ChipVariant,
    pub disabled: bool,
    /// A chip is a fact marker, not a control: no press, no nested removal
    /// button. A bordered or removable chip is a different role with no
    /// shared equivalent.
    pub test_id: Option<SharedString>,
}

impl ChipProps {
    pub fn new(label: impl Into<SharedString>) -> Self {
        ChipProps {
            label: label.into(),
            variant: ChipVariant::default(),
            disabled: false,
            test_id: None,
        }
    }

    pub fn variant(mut self, variant: ChipVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

/// A chip: the compact radius, the chip type size, no stroke. A chip that
/// needs a border is a different role with no shared equivalent.
pub fn chip(props: ChipProps) -> impl IntoElement {
    let t = theme();
    let fill = match props.variant {
        ChipVariant::Neutral => t.muted,
        ChipVariant::Raised => t.card,
        ChipVariant::Accent => t.accent,
    };
    let ink = match props.variant {
        ChipVariant::Neutral => t.muted_foreground,
        ChipVariant::Raised => t.card_foreground,
        ChipVariant::Accent => t.accent_foreground,
    };
    let mut inner = view()
        .flex_row()
        .items_center()
        .rounded(px(data::CHIP_RADIUS))
        .px(px(data::DENSITY_DENSE_BUTTON_PAD_X))
        .py(px(data::DENSITY_DENSE_BUTTON_PAD_Y))
        .bg(fill)
        .text_color(ink)
        .text_size(px(data::TYPE_CHIP))
        .disabled_style(|s| s.opacity(0.45));
    if props.disabled {
        inner = inner.disabled(true);
    }
    let inner = inner.child(text(move || props.label.clone()));
    match props.test_id.clone() {
        Some(id) => view().test_id(&id).child(inner),
        None => view().child(inner),
    }
}
