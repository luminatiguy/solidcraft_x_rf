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
    (app.shell_ui.properties && app.preferences.properties_docked && !app.preferences.properties_left) || app.session.active_sketch.is_some()
}

/// Properties docked at the left, below the Browser.
pub fn properties_left(app: &SolveApp) -> bool {
    app.preferences.panels_limit_view
        && app.preferences.properties_docked
        && app.preferences.properties_left
        && app.shell_ui.properties
        && !crate::render_workspace::active(app)
        && !crate::drawing_ws::active(app)
}

/// The lowest edge of the ribbon's and the design tabs' cards.
fn cards_bottom(app: &SolveApp, full: Rect) -> f32 {
    app.shell.cards.iter().map(|c| c.bottom()).fold(full.top(), f32::max)
}

/// Properties' place at the left: the Browser's width, from below the Browser (or the cards)
/// down to the bottom History or status bar.
pub fn left_properties(app: &SolveApp, full: Rect) -> Option<Rect> {
    if !properties_left(app) {
        return None;
    }
    let column = right_column(app, full);
    let left = full.left() + gap();
    let browser = app.shell.browser.filter(|b| app.ui.show_browser && !app.shell.browser_hidden && b.is_positive());
    let width = browser.map_or(column_width(app), |b| b.width()).clamp(MIN_WIDTH, MAX_WIDTH);
    let top = browser.map_or(column.top(), |b| b.bottom() + gap());
    let top = top.min(column.bottom() - 120.0).max(column.top());
    Some(Rect::from_min_max(pos2(left, top), pos2(left + width, column.bottom())))
}

/// The right column's width.
pub fn column_width(app: &SolveApp) -> f32 {
    let w = app.preferences.right_column_width;
    if w.is_finite() { w.clamp(MIN_WIDTH, MAX_WIDTH) } else { 300.0 }
}

/// The right column within `full` (the area below the ribbon): its full height between the
/// ribbon's cards and the bottom History or status bar.
pub fn right_column(app: &SolveApp, full: Rect) -> Rect {
    let top = cards_bottom(app, full);
    let mut bottom = full.bottom();
    if app.ui.show_timeline
        && !(app.preferences.timeline_left && !properties_left(app))
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
    // Below the ribbon and the design tabs: the docks and the model start under them.
    let below = cards_bottom(app, full);
    if below > r.min.y && below < full.bottom() - 150.0 {
        r.min.y = below + g * 0.5;
    }
    if let Some(p) = left_properties(app, full) {
        r.min.x = r.min.x.max(p.right() + g * 0.5);
    }
    if app.ui.show_browser
        && !app.shell.browser_hidden
        && let Some(b) = app.shell.browser
        && b.is_positive()
        && b.width() < full.width() * 0.6
    {
        r.min.x = r.min.x.max(b.right() + g * 0.5);
    }
    if let Some(h) = app.shell.history.filter(|h| h.is_positive() && app.ui.show_timeline) {
        if app.preferences.timeline_left && !properties_left(app) {
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

/// The small buttons in a docked panel's header, drawn (no font glyphs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HeaderIcon {
    Close,
    /// Undock: a window lifted off another.
    Float,
    /// Dock at the left: a frame with its left strip filled.
    DockLeft,
    /// Dock at the right: a frame with its right strip filled.
    DockRight,
}

/// A header button with a drawn icon, 20×20 points.
pub fn icon_button(ui: &mut egui::Ui, icon: HeaderIcon, tip: &str) -> egui::Response {
    let t = Tokens::get();
    let (rect, r) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
    let r = r.on_hover_text(tip);
    let p = ui.painter();
    if r.hovered() {
        p.rect_filled(rect, f32::from(t.radius_sm), t.accent_soft);
    }
    let ink = if r.hovered() { t.text } else { t.text_dim };
    let stroke = egui::Stroke::new(1.3, ink);
    let c = rect.center();
    let box_ = Rect::from_center_size(c, egui::vec2(11.0, 9.0));
    match icon {
        HeaderIcon::Close => {
            let d = 4.0;
            p.line_segment([pos2(c.x - d, c.y - d), pos2(c.x + d, c.y + d)], stroke);
            p.line_segment([pos2(c.x - d, c.y + d), pos2(c.x + d, c.y - d)], stroke);
        }
        HeaderIcon::Float => {
            let back = Rect::from_min_size(pos2(c.x - 3.0, c.y - 6.0), egui::vec2(9.0, 7.0));
            let front = Rect::from_min_size(pos2(c.x - 6.0, c.y - 2.0), egui::vec2(9.0, 7.0));
            p.rect_stroke(back, 1.0, stroke, egui::StrokeKind::Middle);
            p.rect_filled(front, 1.0, if r.hovered() { t.accent_soft } else { t.panel });
            p.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Middle);
        }
        HeaderIcon::DockLeft | HeaderIcon::DockRight => {
            p.rect_stroke(box_, 1.0, stroke, egui::StrokeKind::Middle);
            let strip = if icon == HeaderIcon::DockLeft {
                Rect::from_min_max(box_.min, pos2(box_.left() + 4.0, box_.bottom()))
            } else {
                Rect::from_min_max(pos2(box_.right() - 4.0, box_.top()), box_.max)
            };
            p.rect_filled(strip, 0.0, ink);
        }
    }
    r
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
