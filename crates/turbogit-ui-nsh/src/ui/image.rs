//! The decoded-image control the image diff pane mounts: an RGBA buffer the
//! state layer already decoded, its caption, or the binary caption when no
//! pixels exist. The buffer is held, never re-encoded, so a pane redraw
//! costs no decode.

use std::sync::Arc;

use nsh_base::theme;
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;

use crate::theme as data;

/// The props of [`image_pane`].
pub struct ImagePaneProps {
    /// The pixel width, when the buffer decoded.
    pub width: u32,
    /// The pixel height, when the buffer decoded.
    pub height: u32,
    /// Straight RGBA rows, `width * height * 4` bytes.
    pub rgba: Option<Arc<[u8]>>,
    /// The caption under the image (`1920×1080 · 5 B`).
    pub caption: SharedString,
    /// The tallest the image may lay out, in points.
    pub max_height: f32,
    pub test_id: Option<SharedString>,
}

impl ImagePaneProps {
    pub fn new(caption: impl Into<SharedString>, max_height: f32) -> Self {
        ImagePaneProps {
            width: 0,
            height: 0,
            rgba: None,
            caption: caption.into(),
            max_height,
            test_id: None,
        }
    }

    pub fn pixels(mut self, width: u32, height: u32, rgba: Arc<[u8]>) -> Self {
        self.width = width;
        self.height = height;
        self.rgba = Some(rgba);
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

/// A decoded image over its caption, fitted under `max_height`. No pixels:
/// the caption alone, which is what a side that refused to decode shows.
pub fn image_pane(props: ImagePaneProps) -> impl IntoElement {
    let t = theme();
    let caption = props.caption.clone();
    let max_height = props.max_height;
    let rgba = props.rgba.clone();
    let (width, height) = (props.width, props.height);
    let mut body = view().flex_col().items_center().gap(px(data::SPACING_Y));
    if let Some(bytes) = rgba {
        body = body.child(
            image(ImageSrc::rgba(width, height, bytes))
                .max_h(px(max_height))
                .max_w(relative(1.0)),
        );
    }
    body = body.child(
        text(move || caption.clone())
            .text_size(px(data::TYPE_CONTROL))
            .text_color(t.muted_foreground),
    );
    let mut pane = view()
        .flex_1()
        .items_center()
        .justify_center()
        .p(px(data::PANEL_PADDING))
        .bg(t.card)
        .child(body);
    if let Some(id) = props.test_id {
        pane = pane.test_id(&id);
    }
    pane
}
