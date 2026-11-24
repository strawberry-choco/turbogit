//! The card: a surface, not a box. A region is carried by its fill and its
//! padding; the interior is divided by spacing and at most one hairline.

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::theme as data;

/// The props of [`card`].
pub struct CardProps {
    /// The card's body.
    pub children: Vec<AnyElement>,
    /// An optional header line above the body.
    pub header: Option<SharedString>,
    pub test_id: Option<SharedString>,
}

impl CardProps {
    pub fn new() -> Self {
        CardProps {
            children: Vec::new(),
            header: None,
            test_id: None,
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_element());
        self
    }

    pub fn header(mut self, header: impl Into<SharedString>) -> Self {
        self.header = Some(header.into());
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

impl Default for CardProps {
    fn default() -> Self {
        Self::new()
    }
}

/// A card: the card surface, the card radius, the panel padding. No stroke:
/// a stroke means the region floats (ADR-0027 R2), and the surface tone plus
/// the air already carry a card.
pub fn card(props: CardProps) -> impl IntoElement {
    let t = theme();
    let mut column = view()
        .flex_col()
        .gap(px(data::SPACING_X))
        .rounded(px(data::CARD_RADIUS))
        .bg(t.card)
        .p(px(data::PANEL_PADDING));
    if let Some(header) = props.header.clone() {
        column = column.child(
            text(move || header.clone())
                .text_size(px(data::TYPE_DETAIL_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.card_foreground),
        );
    }
    for child in props.children {
        column = column.child(child);
    }
    match props.test_id.clone() {
        Some(id) => view().test_id(&id).child(column),
        None => view().child(column),
    }
}
