//! Text-only feedback presenters shared by pages and keyed reads, plus the
//! accent strip the feedback *containers* (banners, toasts) lead with.

use egui::{Color32, CornerRadius, Response, RichText, Ui, vec2};

use crate::theme::{MARK_RADIUS, Palette, TYPE_BODY, chrome_font};

/// The accent strip's spec geometry — a 3 px wide, 18 px tall bar. It is too
/// small to be a control or a chip, which is why it rounds at
/// [`crate::theme::MARK_RADIUS`].
const ACCENT_BAR_WIDTH: f32 = 3.0;
const ACCENT_BAR_HEIGHT: f32 = 18.0;

/// The narrow tinted strip that leads a feedback container — a banner, a
/// toast — down its leading edge.
///
/// This is the *general* form: it takes a colour the caller has already
/// resolved and owns no severity vocabulary of its own. Deciding that
/// "Error" or `ToastKind::Warning` means a particular token belongs to the
/// host, which already has the severity in hand; the shape of the strip
/// belongs here, so every host gets the same width, height, and rounding
/// and a banner and a toast can no longer drift apart.
pub fn accent_bar(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ACCENT_BAR_WIDTH, ACCENT_BAR_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, CornerRadius::same(MARK_RADIUS), color);
}

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
