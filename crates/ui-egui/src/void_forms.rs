//! Show Void Forms: the shapes the void forms cut away, drawn as orange outlines over the model.
//! A double click on one edits its sketches, as a double click on a body does. A void is a cut,
//! so it has no body of its own: its shape is built on a scratch copy of the design with each
//! void made a new body instead, once per change of the design.

use std::sync::Arc;

use egui::{Color32, Pos2, Stroke};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::doc::{FeatureKind, Operation};

use crate::SolveApp;
use crate::viewport::Proj;

/// A void form: its feature, name, and its shape's edges in the world.
#[derive(Clone, Debug)]
pub struct Void {
    pub feature: u64,
    pub name: String,
    pub edges: Vec<Vec<Vec3>>,
}

thread_local! {
    static CACHE: std::cell::RefCell<Option<(usize, Arc<Vec<Void>>)>> = const { std::cell::RefCell::new(None) };
}

/// The most voids shown, and edges per void.
const MAX_VOIDS: usize = 500;
const MAX_EDGES: usize = 20_000;

/// The design's void forms: features that cut and come from a form tool.
fn void_features(app: &SolveApp) -> Vec<(u64, String)> {
    app.session
        .doc
        .features
        .iter()
        .filter(|f| !f.suppressed)
        .filter(|f| match &f.kind {
            FeatureKind::Extrude { operation, .. }
            | FeatureKind::Revolve { operation, .. }
            | FeatureKind::Loft { operation, .. }
            | FeatureKind::Sweep { operation, .. } => *operation == Operation::Cut,
            _ => false,
        })
        .take(MAX_VOIDS)
        .map(|f| (f.id, f.name.clone()))
        .collect()
}

/// The voids' shapes, built again when the design changes.
pub fn voids(app: &SolveApp) -> Arc<Vec<Void>> {
    let key = Arc::as_ptr(&app.session.doc) as usize;
    if let Some(found) = CACHE.with(|c| c.borrow().as_ref().filter(|k| k.0 == key).map(|k| k.1.clone())) {
        return found;
    }
    let features = void_features(app);
    let mut out = Vec::new();
    if !features.is_empty() {
        let cmds: Vec<(String, Value)> = features
            .iter()
            .map(|(id, _)| ("timeline.edit".to_string(), json!({"feature": id, "set": {"operation": "new_body", "targets": []}})))
            .collect();
        if let Ok(preview) = app.session.preview(&cmds) {
            for (id, name) in &features {
                let mut edges = Vec::new();
                for b in preview.after.bodies.iter().filter(|b| b.feature == *id) {
                    let (m, place) = b.mesh_in_place();
                    let moved = !solidcraft_engine::doc::is_identity(&place);
                    for e in m.edges.iter().take(MAX_EDGES) {
                        edges.push(e.iter().map(|p| if moved { solidcraft_engine::doc::apply_point(&place, *p) } else { *p }).collect());
                    }
                }
                if !edges.is_empty() {
                    out.push(Void { feature: *id, name: name.clone(), edges });
                }
            }
        }
    }
    let out = Arc::new(out);
    CACHE.with(|c| *c.borrow_mut() = Some((key, out.clone())));
    out
}

fn screen_edges(proj: &Proj, v: &Void) -> Vec<Vec<Pos2>> {
    v.edges.iter().map(|e| e.iter().filter_map(|p| proj.to_screen(*p)).collect()).collect()
}

fn dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// The void whose outline is under the pointer.
pub fn hit(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<(u64, String)> {
    if !app.forms.show_voids {
        return None;
    }
    let mut best: Option<(f32, &Void)> = None;
    let all = voids(app);
    for v in all.iter() {
        for e in screen_edges(proj, v) {
            for w in e.windows(2) {
                let d = dist(pos, w[0], w[1]);
                if d < 6.0 && best.is_none_or(|b| d < b.0) {
                    best = Some((d, v));
                }
            }
        }
    }
    best.map(|(_, v)| (v.feature, v.name.clone()))
}

/// The voids' outlines, dashed orange; the one under the pointer bolder.
pub fn paint(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if !app.forms.show_voids {
        return;
    }
    let hovered = painter.ctx().pointer_hover_pos().and_then(|p| hit(app, proj, p)).map(|h| h.0);
    let ink = Color32::from_rgb(0xf0, 0x8a, 0x24);
    for v in voids(app).iter() {
        let width = if hovered == Some(v.feature) { 2.4 } else { 1.4 };
        for e in screen_edges(proj, v) {
            if e.len() >= 2 {
                painter.extend(egui::Shape::dashed_line(&e, Stroke::new(width, ink), 6.0, 4.0));
            }
        }
    }
}

/// Edit a void's sketches (its double click).
pub fn edit(app: &mut SolveApp, feature: u64, name: &str) -> bool {
    if app.session.active_sketch.is_some() {
        app.finish_sketch();
        if app.session.active_sketch.is_some() {
            return false;
        }
    }
    if app.session.doc.feature(feature).is_some_and(|f| matches!(f.kind, FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. })) {
        let _ = app.run("timeline.set_profiles", json!({"feature": feature, "profiles": "all"}));
    }
    crate::forms::edit_chain(app, name, feature, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A void extrusion through a box shows as a shape of its own, found under the pointer.
    #[test]
    fn a_void_shows_its_shape() {
        let mut h = crate::scenario::Harness::new();
        let s = &mut h.app.session;
        let run = |s: &mut solidcraft_engine::Session, c: &str, p: Value| s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"));
        run(s, "sketch.create", json!({"plane": "XY", "name": "Box"}));
        run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 20]}));
        run(s, "sketch.finish", json!({}));
        run(s, "solid.extrude", json!({"sketch": "Box", "distance": 10, "operation": "new_body"}));
        run(s, "sketch.create", json!({"plane": "XY", "name": "Void Extrusion 1"}));
        run(s, "sketch.circle.center", json!({"center": [10, 10], "radius": 3}));
        run(s, "sketch.finish", json!({}));
        run(s, "solid.extrude", json!({"sketch": "Void Extrusion 1", "distance": 10, "operation": "cut", "name": "Void Extrusion 1"}));
        h.app.forms.show_voids = true;
        let v = voids(&h.app);
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].name, "Void Extrusion 1");
        assert!(!v[0].edges.is_empty());
    }
}
