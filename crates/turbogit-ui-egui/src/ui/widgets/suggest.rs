use egui::widgets::text_edit::TextEditState;
use egui::{
    Align, Color32, CornerRadius, EventFilter, Id, Layout, Popup, PopupCloseBehavior, Pos2, Rect,
    Response, Sense, TextEdit, Ui, Vec2, WidgetInfo, WidgetType,
    text::{CCursor, CCursorRange},
};

use crate::theme::{CONTROL_RADIUS, GROUP_ROW_HEIGHT, Palette, TYPE_CHIP, TYPE_CONTROL, data_font};

use super::menu::menu_surface;

/// One row of a completion list. `text` is the row's whole meaning and its
/// identity, so arming and clicking are both answered by the string, never by an
/// index into a table that is rebuilt every frame.
pub struct Candidate<'a> {
    pub text: &'a str,
    /// The quiet right-hand mark: a count, a date, whatever the caller can say
    /// in one word.
    pub note: Option<&'a str>,
}

pub struct CompletionFieldProps<'a> {
    /// The field's egui id. The caller salts it per invocation, which is what
    /// keeps a previous field's caret, undo history and selection from becoming
    /// this one's.
    pub id: Id,
    /// The accessible name, painted nowhere.
    pub label: &'a str,
    pub text: &'a mut String,
    /// The field's own width, which the list copies. An `egui::Window` sizes itself
    /// from its content, so a field that claims the available width inflates the
    /// window it is in and leaves the rest of the dialog laid out against a rect it
    /// no longer has.
    pub width: f32,
    /// Ranked already, and already capped. Empty, or `live_text: None`, and no
    /// list is painted. The caller maps its own candidate type in here: a general
    /// widget does not get to know what a branch family is.
    pub candidates: &'a [Candidate<'a>],
    /// The text the list is answering, `None` while the buffer is unclaimed.
    pub live_text: Option<&'a str>,
    pub armed: Option<&'a str>,
    /// `true` exactly while the list is on screen: `Tab` belongs to the field
    /// then, and to egui's focus ring otherwise.
    pub tab_is_mine: bool,
    /// Select the whole buffer on the frame this field first takes focus. A
    /// pre-filled dialog field says yes; a search box does not.
    pub select_on_focus: bool,
}

pub struct CompletionReply {
    pub response: Response,
    /// The row the pointer took, by its text.
    pub clicked: Option<String>,
    /// The painted list's own rect, `None` while it is closed. The widget owns the
    /// anchor arithmetic, so this is the only way a caller can hit-test the float.
    pub list_rect: Option<Rect>,
    pub edited: bool,
    pub gained_focus: bool,
}

/// The text the field is answering: the buffer while it holds focus with the
/// caret, or a partial selection, inside it; `None` while the buffer is
/// unclaimed, which is unfocused or selected whole for the first keystroke to
/// replace. Derived from the field's own cursor, so there is no "has the reader
/// typed yet" flag to go stale.
pub fn claimed_text(ctx: &egui::Context, id: Id, buffer: &str) -> Option<String> {
    if buffer.is_empty() || !ctx.memory(|mem| mem.has_focus(id)) {
        return None;
    }
    (!selected_whole(ctx, id, buffer)).then(|| buffer.to_owned())
}

/// Whether the field's stored range is the whole buffer, which is the shape of a
/// pre-fill waiting to be replaced rather than a prefix being written.
fn selected_whole(ctx: &egui::Context, id: Id, buffer: &str) -> bool {
    TextEditState::load(ctx, id)
        .and_then(|state| state.cursor.char_range())
        .is_some_and(|range| {
            let [min, max] = range.sorted_cursors();
            min == CCursor::new(0) && max == CCursor::new(buffer.chars().count())
        })
}

fn selection_is_owed(ctx: &egui::Context, id: Id) -> bool {
    ctx.data(|data| data.get_temp::<bool>(pending_id(id)))
        .unwrap_or(false)
}

/// A single-line field with a completion list anchored under it. Owns the anchor
/// arithmetic, the layering over the enclosing `Window`, the row geometry and
/// ink, the first-paint focus take and the select-all, and the focus lock that
/// keeps Escape in the field and `Tab` in it only while the list is on screen.
/// Owns no policy about what a candidate means, which is the caller's, and reads
/// no application state.
pub fn completion_field(ui: &mut Ui, props: CompletionFieldProps<'_>) -> CompletionReply {
    let ctx = ui.ctx().clone();
    let id = props.id;
    let showing = props.live_text.is_some() && !props.candidates.is_empty();

    let mut gained_focus = false;
    let first_paint = !ctx
        .data(|data| data.get_temp::<bool>(first_paint_id(id)))
        .unwrap_or(false);
    if first_paint {
        ctx.data_mut(|data| data.insert_temp(first_paint_id(id), true));
        // The whole-buffer selection is owed until the first keystroke consumes it,
        // and it has to be rewritten every frame until then. Two of egui's defaults
        // bite: `CCursorRange::two(min, max)` puts `primary` at `max`, and a
        // `TextEdit` that does not hold focus collapses the stored range to
        // `one(primary)` — so a selection written once is a caret at the end by the
        // next unfocused frame. `Memory::end_pass`'s dead-mans-switch also drops the
        // focus asked for on a frame the widget had not painted on, which is what a
        // dialog opened from the Branches popup does, so focus is re-asked too.
        ctx.data_mut(|data| {
            data.insert_temp(
                pending_id(id),
                props.select_on_focus && !props.text.is_empty(),
            )
        });
        ctx.memory_mut(|mem| mem.request_focus(id));
        gained_focus = true;
    }
    if selection_is_owed(&ctx, id) {
        select_whole_buffer(&ctx, id, props.text);
        // Only while nothing at all holds focus, so `Tab` to the checkbox still
        // moves focus: the case this covers is the dialog that opened with no focus
        // owner because the surface that had it vanished with it.
        if ctx.memory(|mem| mem.focused().is_none()) {
            ctx.memory_mut(|mem| mem.request_focus(id));
        }
    }

    let response = ui.add(
        TextEdit::singleline(props.text)
            .id(id)
            .return_key(None)
            .desired_width(props.width),
    );
    gained_focus |= response.gained_focus();
    let edited = response.changed();
    // Owed no longer once the keystroke that replaced the pre-fill has come, or once
    // the reader has put the caret elsewhere: a click into the field or an arrow key
    // means the buffer is theirs, not a placeholder.
    if edited || (response.has_focus() && !selected_whole(&ctx, id, props.text)) {
        ctx.data_mut(|data| data.insert_temp(pending_id(id), false));
    }
    // A buffer the field did not edit itself was replaced by its owner — an accepted
    // row — so the stored caret indexes a text that no longer exists. Drop it at the
    // end, where the reader's next character goes.
    if !edited
        && ctx
            .data(|data| data.get_temp::<String>(text_id(id)))
            .is_some_and(|seen| seen != *props.text)
    {
        set_caret_at_end(&ctx, id, props.text);
        ctx.data_mut(|data| data.insert_temp(pending_id(id), false));
    }
    ctx.data_mut(|data| data.insert_temp(text_id(id), props.text.clone()));
    response
        .widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, ui.is_enabled(), props.label));

    // Installed after the add, because `TextEdit` writes its own filter every
    // frame it holds focus, `tick_focus` reads the filter stored during the
    // *previous* pass, and the last writer of a pass wins.
    ctx.memory_mut(|mem| {
        mem.set_focus_lock_filter(
            id,
            EventFilter {
                tab: props.tab_is_mine,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            },
        );
    });

    let mut clicked = None;
    let mut list_rect = None;
    if showing {
        let frame = menu_surface(ui);
        let width = response.rect.width();
        let rows = props.candidates;
        let armed = props.armed;
        if let Some(inside) = Popup::from_response(&response)
            .open(true)
            .close_behavior(PopupCloseBehavior::IgnoreClicks)
            .frame(frame)
            .width(width)
            .layout(Layout::top_down(Align::Min))
            .show(|ui| {
                let mut picked = None;
                for candidate in rows {
                    if completion_row(ui, candidate, armed == Some(candidate.text)).clicked() {
                        picked = Some(candidate.text.to_owned());
                    }
                }
                picked
            })
        {
            list_rect = Some(inside.response.rect);
            clicked = inside.inner;
        }
    }

    CompletionReply {
        response,
        clicked,
        list_rect,
        edited,
        gained_focus,
    }
}

fn completion_row(ui: &mut Ui, candidate: &Candidate<'_>, armed: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), GROUP_ROW_HEIGHT),
        Sense::click(),
    );

    let fill = if armed {
        Palette::SURFACE_3
    } else if response.hovered() {
        Palette::SURFACE_2
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }

    let text = ui.painter().layout_no_wrap(
        candidate.text.to_owned(),
        data_font(TYPE_CONTROL),
        Palette::INK,
    );
    ui.painter().galley(
        Pos2::new(rect.left() + 8.0, rect.center().y - text.size().y / 2.0),
        text,
        Color32::WHITE,
    );
    if let Some(note) = candidate.note {
        let count =
            ui.painter()
                .layout_no_wrap(note.to_owned(), data_font(TYPE_CHIP), Palette::T_MUTED);
        ui.painter().galley(
            Pos2::new(
                rect.right() - 8.0 - count.size().x,
                rect.center().y - count.size().y / 2.0,
            ),
            count,
            Color32::WHITE,
        );
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, candidate.text));
    response
}

fn select_whole_buffer(ctx: &egui::Context, id: Id, buffer: &str) {
    write_cursor(
        ctx,
        id,
        CCursorRange::two(CCursor::new(0), CCursor::new(buffer.chars().count())),
    );
}

fn set_caret_at_end(ctx: &egui::Context, id: Id, buffer: &str) {
    write_cursor(
        ctx,
        id,
        CCursorRange::one(CCursor::new(buffer.chars().count())),
    );
}

fn write_cursor(ctx: &egui::Context, id: Id, range: CCursorRange) {
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(range));
    TextEdit::store_state(ctx, id, state);
}

fn first_paint_id(id: Id) -> Id {
    id.with("tg_completion_field_first_paint")
}

fn pending_id(id: Id) -> Id {
    id.with("tg_completion_field_selection_owed")
}

fn text_id(id: Id) -> Id {
    id.with("tg_completion_field_text")
}
