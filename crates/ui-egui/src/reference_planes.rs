//! Reference planes: drawn with two clicks in the view, square to it, so the view sees them
//! edge-on. They show only as a dashed line, and only in a view that sees them edge-on; they can
//! be the work plane, and a drawing never shows them. Show Work Plane outlines the work plane's
//! own edges (a face's boundary, a plane's line or frame) over its grid.

use egui::{Color32, Pos2, Stroke};
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::viewport::{Hit, Proj};

type V3 = solidcore_record::Vec3;

/// Reference planes are construction planes named so.
pub const PREFIX: &str = "Reference Plane";

/// Seen edge-on: the plane's normal is (almost) square to the line of sight.
const EDGE_ON: f64 = 0.03;

pub fn is_reference(name: &str) -> bool {
    name.starts_with(PREFIX)
}

/// The reference planes as (name, normal, origin, in-plane direction seen as the line).
fn planes(app: &SolveApp) -> Vec<(String, V3, V3, V3, V3)> {
    solidcraft_engine::view::construction_planes(&app.session)
        .into_iter()
        .filter(|(_, n, _)| is_reference(n) && app.session.construction_visible(n))
        .map(|(_, name, pl)| (name, pl.normal(), pl.origin, pl.x, pl.y))
        .collect()
}

fn view_dir(proj: &Proj, at: V3) -> V3 {
    let v = if proj.cam.fov > 0.0 { proj.cam.eye() - at } else { proj.cam.back() };
    let l = v.len();
    if l > 0.0 { v * (1.0 / l) } else { v }
}

/// The screen segment of each reference plane seen edge-on.
fn segments(app: &SolveApp, proj: &Proj) -> Vec<(String, Pos2, Pos2)> {
    let size = app.cam.half_height() * 6.0;
    planes(app)
        .into_iter()
        .filter_map(|(name, n, o, x, y)| {
            if n.dot(view_dir(proj, o)).abs() > EDGE_ON {
                return None;
            }
            // Of the plane's two directions, the one the view sees along its length.
            let along = if x.dot(view_dir(proj, o)).abs() < y.dot(view_dir(proj, o)).abs() { x } else { y };
            let a = proj.to_screen(o - along * size)?;
            let b = proj.to_screen(o + along * size)?;
            Some((name, a, b))
        })
        .collect()
}

fn dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// A reference plane under the pointer (seen edge-on, within a few pixels of its line).
pub fn hit(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<Hit> {
    let (name, _, _) = segments(app, proj).into_iter().find(|(_, a, b)| dist(pos, *a, *b) < 6.0)?;
    let (o, d) = proj.ray(pos);
    Some(Hit::Plane { name, point: o + d * 1.0 })
}

/// The dashed lines, lit when hovered or selected.
pub fn paint(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let t = crate::theme::Tokens::get();
    for (name, a, b) in segments(app, proj) {
        let selected = app.session.selection.iter().any(|s| matches!(s, Sel::Plane { name: n } if *n == name))
            || matches!(&app.forms.work_plane, Some(Sel::Plane { name: n }) if *n == name);
        let hovered = matches!(&app.viewport.hover, Some(Hit::Plane { name: n, .. }) if *n == name);
        let col = if hovered || selected { t.sel_edge } else { t.text_dim };
        let width = if hovered { 2.2 } else { 1.3 };
        painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(width, col), 10.0, 6.0));
    }
}

/// Make a reference plane through the line from `a` to `b`, square to the plane whose normal
/// is `n` (the sketch it was drawn in).
pub fn make(app: &mut SolveApp, a: V3, b: V3, n: V3) {
    let count = planes(app).len() + 1;
    let pt = |v: V3| json!({"point": [v.x, v.y, v.z]});
    let name = format!("{PREFIX} {count}");
    let c = a + n * 10.0;
    match app.run("construct.plane.three_points", json!({"a": pt(a), "b": pt(b), "c": pt(c), "name": name})) {
        Ok(_) => app.set_status(format!("{name} made: it shows as a dashed line in views that see it edge-on; Set Work Plane can pick it"), false),
        Err(e) => app.set_status(e, true),
    }
}

/// Show Work Plane: the work plane's own edges, lit over the grid.
pub fn paint_work_plane_edges(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if !app.forms.show_work_plane {
        return;
    }
    let accent = crate::theme::Tokens::get().accent;
    let glow = Stroke::new(6.0, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 70));
    let core = Stroke::new(2.0, accent);
    let mut lines: Vec<Vec<V3>> = Vec::new();
    match &app.forms.work_plane {
        Some(Sel::Face { body, index, .. }) => lines = crate::viewport::face_edges(app, body, *index),
        Some(Sel::Plane { name }) if !is_reference(name) => {
            let quads = crate::viewport::construction_quads(app).into_iter().map(|(n, _, q)| (n, q));
            let origin = crate::viewport::origin_planes(app).into_iter().map(|(n, _, q)| (n.to_string(), q));
            if let Some((_, q)) = origin.chain(quads).find(|(n, _)| n == name) {
                lines.push(vec![q[0], q[1], q[2], q[3], q[0]]);
            }
        }
        // A reference plane's line is lit by `paint`.
        Some(_) => {}
        None => {
            if let Some((_, q)) = crate::viewport::origin_planes(app).into_iter().find(|(n, _, _)| *n == "XY").map(|(n, _, q)| (n, q)) {
                lines.push(vec![q[0], q[1], q[2], q[3], q[0]]);
            }
        }
    }
    for line in lines {
        let pts: Vec<Pos2> = line.iter().filter_map(|p| proj.to_screen(*p)).collect();
        if pts.len() > 1 {
            painter.add(egui::Shape::line(pts.clone(), glow));
            painter.add(egui::Shape::line(pts, core));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_makes_a_plane_seen_edge_on_as_a_line() {
        let mut h = crate::scenario::Harness::new();
        h.app.cam.set_view(solidcraft_engine::render::StandardView::Top);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        h.app.viewport.rect = Some(rect);
        // Drawn in a sketch on XY from (-10, 0) to (10, 0): the plane is XZ-like, seen edge-on from the top.
        make(&mut h.app, V3::new(-10.0, 0.0, 0.0), V3::new(10.0, 0.0, 0.0), V3::new(0.0, 0.0, 1.0));
        assert_eq!(planes(&h.app).len(), 1);
        assert!(crate::viewport::construction_quads(&h.app).iter().all(|(n, _, _)| !is_reference(n)));
        let proj = crate::viewport::projection(&h.app, rect);
        let segs = segments(&h.app, &proj);
        assert_eq!(segs.len(), 1);
        let (_, a, b) = &segs[0];
        let mid = egui::pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        assert!(matches!(hit(&h.app, &proj, mid), Some(Hit::Plane { .. })));
        assert!(hit(&h.app, &proj, mid + egui::vec2(0.0, 100.0)).is_none());
        h.app.cam.set_view(solidcraft_engine::render::StandardView::Front);
        let proj = crate::viewport::projection(&h.app, rect);
        assert!(segments(&h.app, &proj).is_empty());
    }
}
