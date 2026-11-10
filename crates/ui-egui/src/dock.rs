//! Docked panels limit the model's space instead of lying over it: the Browser and a left
//! History at the left, the right column (Properties, the sketch's panel, a feature's form) at
//! the right, a bottom History along the bottom. The scene is drawn only between them, so the
//! view cube and the navigation bar stay clear of every panel.
//!
//! The right column is one dock: the sketch's panel above Properties, never over each other,
//! its width dragged at its left edge. Properties can float instead (its header's button) and
//! dock again the same way.

use egui::{Rect, pos2};

use crate::SolveApp;
use crate::theme::Tokens;

/// The narrowest and widest the right column is dragged to.
pub const MIN_WIDTH: f32 = 220.0;
pub const MAX_WIDTH: f32 = 640.0;

/// The gap between the docks and the model's space.
fn gap() -> f32 {
    crate::shell::SHELL_GAP
}

/// Is anything docked in the right column this frame?
pub fn right_column_used(app: &SolveApp) -> bool {
    if !app.preferences.panels_limit_view || crate::render_workspace::active(app) || crate::drawing_ws::active(app) {
        return false;
    }
    (app.shell_ui.properties && app.preferences.properties_docked) || app.session.active_sketch.is_some()
}

/// The right column's width.
pub fn column_width(app: &SolveApp) -> f32 {
    let w = app.preferences.right_column_width;
    if w.is_finite() { w.clamp(MIN_WIDTH, MAX_WIDTH) } else { 300.0 }
}

/// The right column within `full` (the area below the ribbon): its full height between the
/// ribbon's cards and the bottom History or status bar.
pub fn right_column(app: &SolveApp, full: Rect) -> Rect {
    let mut top = full.top();
    for card in &app.shell.cards {
        top = top.max(card.bottom());
    }
    let mut bottom = full.bottom();
    if app.ui.show_timeline
        && !app.preferences.timeline_left
        && let Some(h) = app.shell.history
    {
        bottom = bottom.min(h.top());
    }
    if let Some(s) = app.shell.status_bar {
        bottom = bottom.min(s.top());
    }
    let right = full.right() - gap();
    let left = (right - column_width(app)).max(full.left());
    Rect::from_min_max(pos2(left, top + gap()), pos2(right, (bottom - gap()).max(top + gap())))
}

/// The model's space in `full`: without the docked panels (measured last frame).
pub fn model_rect(app: &SolveApp, full: Rect) -> Rect {
    if !app.preferences.panels_limit_view || crate::render_workspace::active(app) || crate::drawing_ws::active(app) {
        return full;
    }
    let mut r = full;
    let g = gap();
    if app.ui.show_browser
        && !app.shell.browser_hidden
        && let Some(b) = app.shell.browser
        && b.is_positive()
        && b.width() < full.width() * 0.6
    {
        r.min.x = r.min.x.max(b.right() + g * 0.5);
    }
    if let Some(h) = app.shell.history.filter(|h| h.is_positive() && app.ui.show_timeline) {
        if app.preferences.timeline_left {
            if h.width() < full.width() * 0.6 {
                r.min.x = r.min.x.max(h.right() + g * 0.5);
            }
        } else if h.height() < full.height() * 0.6 {
            r.max.y = r.max.y.min(h.top() - g * 0.5);
        }
    }
    if right_column_used(app) {
        r.max.x = r.max.x.min(right_column(app, full).left() - g * 0.5);
    }
    if r.width() < 200.0 || r.height() < 150.0 { full } else { r }
}

/// The handle on the right column's left edge: drag to widen or narrow it; a double click puts
/// it back to its default width.
pub fn width_handle(app: &mut SolveApp, ctx: &egui::Context, column: Rect) {
    let handle = Rect::from_min_max(pos2(column.left() - 4.0, column.top()), pos2(column.left() + 3.0, column.bottom()));
    let id = egui::Id::new("sc_right_column_width");
    let host = crate::ui_kit::Container::new(ctx, crate::ui_kit::Surface::Panel);
    let shown = host
        .area(id)
        .fixed_pos(handle.min)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| ui.allocate_exact_size(handle.size(), egui::Sense::click_and_drag()).1);
    let r = shown.inner;
    let ui = &shown.response;
    if r.hovered() || r.dragged() {
        ui.ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        let t = Tokens::get();
        ui.ctx
            .layer_painter(egui::LayerId::new(egui::Order::Foreground, id))
            .line_segment([pos2(column.left() - 1.0, column.top()), pos2(column.left() - 1.0, column.bottom())], egui::Stroke::new(2.0, t.accent));
    }
    crate::scenario::publish_handle("dock:right:width", handle.center());
    if r.dragged() {
        let w = column_width(app) - r.drag_delta().x;
        app.preferences.right_column_width = w.clamp(MIN_WIDTH, MAX_WIDTH);
    }
    if r.double_clicked() {
        app.preferences.right_column_width = 300.0;
    }
}

/// A drag handle along the vertical line `x` from `top` to `bottom`: how far it was dragged
/// this frame (points, rightwards), and whether it was double-clicked (back to the default).
pub fn edge_handle(ctx: &egui::Context, id: &str, x: f32, top: f32, bottom: f32) -> (f32, bool) {
    let handle = Rect::from_min_max(pos2(x - 3.5, top), pos2(x + 3.5, bottom.max(top + 1.0)));
    let id = egui::Id::new(id);
    let host = crate::ui_kit::Container::new(ctx, crate::ui_kit::Surface::Panel);
    let shown = host
        .area(id)
        .fixed_pos(handle.min)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| ui.allocate_exact_size(handle.size(), egui::Sense::click_and_drag()).1);
    let r = shown.inner;
    if r.hovered() || r.dragged() {
        ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, id.with("line")))
            .line_segment([pos2(x, top), pos2(x, bottom)], egui::Stroke::new(2.0, Tokens::get().accent));
    }
    (if r.dragged() { r.drag_delta().x } else { 0.0 }, r.double_clicked())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A docked browser and Properties take their room out of the model's space; floating
    /// Properties (or the preference off) does not.
    #[test]
    fn docked_panels_limit_the_model_space() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        let full = Rect::from_min_max(pos2(0.0, 100.0), pos2(1600.0, 1000.0));
        app.ui.show_browser = true;
        app.shell.browser = Some(Rect::from_min_max(pos2(7.0, 107.0), pos2(257.0, 600.0)));
        app.shell_ui.properties = true;
        app.preferences.properties_docked = true;
        app.preferences.panels_limit_view = true;
        app.ui.show_timeline = false;
        let r = model_rect(&app, full);
        assert!(r.left() >= 257.0, "{r:?}");
        assert!(r.right() <= right_column(&app, full).left(), "{r:?}");
        app.preferences.properties_docked = false;
        assert!((model_rect(&app, full).right() - 1600.0).abs() < 1e-3);
        app.preferences.panels_limit_view = false;
        assert_eq!(model_rect(&app, full), full);
    }
}
