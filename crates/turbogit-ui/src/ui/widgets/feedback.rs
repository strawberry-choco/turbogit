//! Text-only feedback presenters shared by pages and keyed reads.

use egui::{Response, RichText, Ui};

use crate::theme::{Palette, TYPE_BODY, chrome_font};

/// Present one genuine one-line error in the shared error ink and body type.
///
/// This is intentionally only a text presenter. A page that has a separator,
/// retry action, or other context keeps that context and composes this beside
/// it; notes, alerts, banners, toasts, dialog error lists, and conflict
/// feedback have their own containers and do not use this helper.
pub fn inline_error(ui: &mut Ui, message: impl Into<String>) -> Response {
    ui.label(
        RichText::new(message.into())
            .font(chrome_font(TYPE_BODY))
            .color(Palette::STATE_ERROR),
    )
}

/// The two non-content answers a keyed read can present.
///
/// The text is already display-ready when it reaches this enum. The presenter
/// deliberately knows nothing about cache admission or `turbogit_app::Read`;
/// each surface owns its empty answer and supplies its own waiting text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyedReadPresentation<'a> {
    Waiting(&'a str),
    Failed(&'a str),
}

/// Render the shared waiting/failure answer for a keyed read.
///
/// Waiting keeps the familiar spinner-plus-body-label treatment. Failure uses
/// [`inline_error`] so a failed read cannot drift into a different error style.
pub fn keyed_read_presentation(ui: &mut Ui, presentation: KeyedReadPresentation<'_>) {
    match presentation {
        KeyedReadPresentation::Waiting(message) => {
            ui.spinner();
            ui.label(message);
        }
        KeyedReadPresentation::Failed(message) => {
            inline_error(ui, message);
        }
    }
}
