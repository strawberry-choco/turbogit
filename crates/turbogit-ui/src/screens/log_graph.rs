//! The graph painter: one row's lane strip, the spike's painter pattern. The
//! lane rails run the row's full height at fixed x per lane column, so every
//! lane meets every row exactly; the commit wears its lane's color, a merge
//! a ring, a plain commit a filled dot.

use nsh_core::prelude::*;

use crate::theme as data;

/// One lane column's width; the strip draws [`MAX_LANES`] rails.
pub const LANE_WIDTH: f32 = 14.0;
/// The lane columns the strip draws before they repeat.
pub const MAX_LANES: usize = 4;
/// The row height the strip paints within.
pub const ROW_HEIGHT: f32 = 24.0;
/// The strip's width: the lanes it draws.
pub const STRIP_WIDTH: f32 = LANE_WIDTH * MAX_LANES as f32;

/// The muted rail the idle lane columns carry, a step under any lane color.
const RAIL: AdaptiveColor = data::GRAPH_RAIL;

/// The lane column a lane index paints in.
pub fn column_of(lane: usize) -> f32 {
    LANE_WIDTH * (lane % MAX_LANES) as f32 + LANE_WIDTH / 2.0
}

/// One row's strip: the rails, then the commit's dot (a ring on a merge).
pub fn lane_strip(lane: Option<usize>, merge: bool) -> Painter {
    let is_dark = dark();
    let rail = if is_dark { RAIL.dark } else { RAIL.light };
    let mut painter = Painter::new();
    for column in 0..MAX_LANES {
        let x = LANE_WIDTH * column as f32 + LANE_WIDTH / 2.0 - 1.0;
        painter = painter.fill_color(rail).fill_rect(x, 0.0, 2.0, ROW_HEIGHT);
    }
    let adaptive = match lane {
        Some(lane) => data::lane(lane),
        None => data::GRAPH_MUTED,
    };
    let lane_color = if dark() {
        adaptive.dark
    } else {
        adaptive.light
    };
    let column = column_of(lane.unwrap_or(0));
    if merge {
        painter
            .stroke_color(lane_color)
            .arc(column, ROW_HEIGHT / 2.0, 3.0, 0.0, std::f32::consts::TAU)
            .stroke()
    } else {
        painter
            .fill_color(lane_color)
            .arc(column, ROW_HEIGHT / 2.0, 4.0, 0.0, std::f32::consts::TAU)
            .fill()
    }
}
