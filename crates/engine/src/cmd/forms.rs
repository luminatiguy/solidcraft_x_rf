//! Form tools of the MODEL tab (Extrusion, Blend, Revolve, Sweep, Swept Blend, their void
//! versions), the work plane and Model Line. They are interactive: the window starts a sketch on
//! the work plane and builds the feature from it when the sketch is finished. Run here (over the
//! control channel or a script) they only say so: the features themselves are `solid.extrude`,
//! `solid.loft`, `solid.revolve` and `solid.sweep`, and a sketch is `sketch.create`.

use serde_json::{Value, json};
use solidcore_record::{Plane, Vec3};
use solidcraft_doc::{Direction, Expr, FeatureKind, Mat, PlaneRef};
use solidcraft_geom::prelude::*;

use super::CommandSpec;
use crate::params::{bad, vec3};
use crate::{Result, Session};

fn interactive(cmd: &'static str, instead: &'static str) -> Result<Value> {
    Err(bad(cmd, format!("interactive only: it opens a sketch on the work plane in the window; run {instead} instead")))
}

fn extrusion(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.extrusion", "sketch.create and solid.extrude")
}
fn blend(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.blend", "sketch.create twice and solid.loft")
}
fn revolve(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.revolve", "sketch.create and solid.revolve")
}
fn sweep(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.sweep", "sketch.create twice and solid.sweep")
}
fn swept_blend(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.swept_blend", "sketch.create three times and solid.loft")
}
fn void_extrusion(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.void.extrusion", "solid.extrude with operation cut")
}
fn void_blend(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.void.blend", "solid.loft with operation cut")
}
fn void_revolve(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.void.revolve", "solid.revolve with operation cut")
}
fn void_sweep(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.void.sweep", "solid.sweep with operation cut")
}
fn void_swept_blend(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.void.swept_blend", "solid.loft with operation cut")
}
fn set_work_plane(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("workplane.set", "sketch.create with the plane")
}
fn show_work_plane(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("workplane.show", "view.grid")
}
fn reference_plane(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("workplane.reference", "construct.plane.three_points")
}
fn model_line(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("model.line", "sketch.create and sketch.line")
}

fn hatch_pattern(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("hatch.pattern", "hatch.apply")
}
fn show_references(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("workplane.show_references", "construct.plane.list")
}
fn window_setting(_: &mut Session, _: &Value) -> Result<Value> {
    Err(bad("view", "a window setting: the ribbon button toggles it; over the control channel use ui.set"))
}

fn objects(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("obj", "solid.move, solid.mirror or solid.combine")
}

/// The rigid motion `form.transform` asks for, as a matrix (`m[i]` is the image of axis `i`,
/// `m[3]` the translation) and whether it mirrors.
fn motion(cmd: &str, p: &Value) -> Result<(Mat, bool)> {
    let mut m: Mat = solidcraft_doc::IDENTITY;
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite() && v.len() < 1e9;
    if let Some(t) = p.get("translate") {
        let t = vec3(t).filter(|v| finite(*v)).ok_or_else(|| bad(cmd, "`translate` must be [x, y, z]"))?;
        m[3] = [t.x, t.y, t.z, 1.0];
        return Ok((m, false));
    }
    if let Some(r) = p.get("rotate") {
        let o = r.get("origin").and_then(vec3).filter(|v| finite(*v)).ok_or_else(|| bad(cmd, "`rotate.origin` must be [x, y, z]"))?;
        let k = r.get("axis").and_then(vec3).and_then(|v| v.normalized()).ok_or_else(|| bad(cmd, "`rotate.axis` must be a direction"))?;
        let deg = r
            .get("angle")
            .and_then(Value::as_f64)
            .filter(|a| a.is_finite() && a.abs() <= 1e6)
            .ok_or_else(|| bad(cmd, "`rotate.angle` must be degrees"))?;
        let (sn, cs) = deg.to_radians().sin_cos();
        let turn = |v: Vec3| v * cs + k.cross(v) * sn + k * (k.dot(v) * (1.0 - cs));
        for (i, e) in [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)].into_iter().enumerate() {
            let c = turn(e);
            if let Some(col) = m.get_mut(i) {
                *col = [c.x, c.y, c.z, 0.0];
            }
        }
        let t = o - turn(o);
        m[3] = [t.x, t.y, t.z, 1.0];
        return Ok((m, false));
    }
    if let Some(r) = p.get("mirror") {
        let o = r.get("origin").and_then(vec3).filter(|v| finite(*v)).ok_or_else(|| bad(cmd, "`mirror.origin` must be [x, y, z]"))?;
        let n = r.get("normal").and_then(vec3).and_then(|v| v.normalized()).ok_or_else(|| bad(cmd, "`mirror.normal` must be a direction"))?;
        for (i, e) in [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)].into_iter().enumerate() {
            let c = e - n * (2.0 * e.dot(n));
            if let Some(col) = m.get_mut(i) {
                *col = [c.x, c.y, c.z, 0.0];
            }
        }
        let t = n * (2.0 * o.dot(n));
        m[3] = [t.x, t.y, t.z, 1.0];
        return Ok((m, true));
    }
    Err(bad(cmd, "give `translate`, `rotate` or `mirror`"))
}

/// A feature mirrored: its sketches' planes turn left-handed, so the profiles land mirrored, but
/// their normal points the other way. What goes along the normal (an extrusion's side and start
/// offset, a revolution's turn) goes the other way to keep the mirror image.
fn mirror_sense(kind: &mut FeatureKind) {
    let negate = |e: &mut Expr| *e = Expr::from(format!("-({})", e.trim()));
    match kind {
        FeatureKind::Extrude { extent, .. } => {
            extent.direction = match extent.direction {
                Direction::Positive => Direction::Negative,
                Direction::Negative => Direction::Positive,
                Direction::Symmetric => Direction::Symmetric,
            };
            if let Some(o) = extent.start_offset.as_mut() {
                negate(o);
            }
        }
        FeatureKind::Revolve { angle, .. } => negate(angle),
        _ => {}
    }
}

/// Move, turn, mirror or copy form bodies by moving what made them: each body's sketches go onto
/// planes of their own (the old plane moved), so the body is edited where it now is, and a copy
/// is a new form of its own, edited apart from the original.
fn transform(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "form.transform";
    let (m, mirrored) = motion(cmd, p)?;
    let copy = p.get("copy").and_then(Value::as_bool).unwrap_or(false);
    let bodies: Vec<String> = p
        .get("bodies")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty() && a.len() <= 1000)
        .and_then(|a| a.iter().map(|v| v.as_str().map(str::to_string)).collect())
        .ok_or_else(|| bad(cmd, "`bodies` must list 1…1000 body names"))?;
    let names: Vec<String> =
        p.get("names").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let state = s.model.state();
    // Each body's form feature and its sketches, with their planes in the world now.
    let mut plans: Vec<(u64, Vec<(u64, Plane)>)> = Vec::new();
    for name in &bodies {
        let body = state.bodies.iter().find(|b| &b.name == name).ok_or_else(|| bad(cmd, format!("no body `{name}`")))?;
        let f = s.doc.feature(body.feature).ok_or_else(|| bad(cmd, format!("`{name}` has no feature")))?;
        if !matches!(f.kind, FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. } | FeatureKind::Loft { .. } | FeatureKind::Sweep { .. }) {
            return Err(bad(cmd, format!("`{name}` is not a form (Extrusion, Blend, Revolve, Sweep)")));
        }
        let mut sketches = Vec::new();
        for id in f.kind.input_sketches() {
            let solved = state.sketch(id).ok_or_else(|| bad(cmd, format!("`{name}`'s sketch is not built")))?;
            if !sketches.iter().any(|(k, _)| *k == id) {
                sketches.push((id, solved.plane));
            }
        }
        if !plans.iter().any(|(k, _)| *k == body.feature) {
            plans.push((body.feature, sketches));
        }
    }
    drop(state);
    let mut made = Vec::new();
    for (k, (feature, sketches)) in plans.into_iter().enumerate() {
        let mut renamed: std::collections::BTreeMap<u64, u64> = std::collections::BTreeMap::new();
        for (id, world) in &sketches {
            let doc = s.doc_mut();
            let Some(f) = doc.feature(*id).cloned() else { continue };
            let mut kind = f.kind.clone();
            if let FeatureKind::Sketch { plane, .. } = &mut kind {
                // The plane it is on now, fixed: the moved copy no longer leans on a face.
                *plane = PlaneRef::Custom { plane: *world };
            }
            kind.transform_geometry(&m);
            if copy {
                let base = names.get(k).cloned().unwrap_or_else(|| f.name.clone());
                let suffix = f.name.split_once(" · ").map(|(_, r)| format!(" · {r}")).unwrap_or_default();
                let wanted = if names.get(k).is_some() { format!("{base}{suffix}") } else { base };
                let name = solidcraft_doc::copy_name(&wanted, |n| doc.features.iter().any(|x| x.name == n));
                let new = doc.add_feature(kind, Some(&name))?;
                renamed.insert(*id, new);
            } else if let Some(fm) = doc.feature_mut(*id) {
                fm.kind = kind;
            }
        }
        let doc = s.doc_mut();
        let Some(f) = doc.feature(feature).cloned() else { continue };
        let mut kind = f.kind.clone();
        kind.transform_geometry(&m);
        if mirrored {
            mirror_sense(&mut kind);
        }
        if copy {
            kind.remap_refs(&renamed, &std::collections::BTreeMap::new());
            let wanted = names.get(k).cloned().unwrap_or_else(|| f.name.clone());
            let name = solidcraft_doc::copy_name(&wanted, |n| doc.features.iter().any(|x| x.name == n));
            let new = doc.add_feature(kind, Some(&name))?;
            if let Some(nf) = doc.feature_mut(new) {
                nf.component = f.component;
                nf.body_names = names.get(k).map(|n| vec![n.clone()]).unwrap_or_default();
            }
            made.push(name);
        } else if let Some(fm) = doc.feature_mut(feature) {
            fm.kind = kind;
            made.push(fm.name.clone());
        }
    }
    s.refresh();
    let errors: Vec<String> =
        s.doc.features.iter().filter(|f| made.contains(&f.name)).filter_map(|f| s.model.result(f.id).and_then(|r| r.error.clone())).collect();
    if let Some(e) = errors.first() {
        return Err(bad(cmd, e.clone()));
    }
    Ok(json!({"features": made}))
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("form.transform", "Move Forms", transform).doc_only().params(
        "bodies: [names] (Extrusion, Blend, Revolve, Sweep bodies); one of translate: [x,y,z], \
         rotate: {origin, axis, angle (degrees)}, mirror: {origin, normal}; copy?: true for new forms; \
         names?: [body names for the copies]. The bodies' sketches move onto planes of their own, so \
         each body is edited where it now is",
    ),
    CommandSpec::new("obj.move", "Move", objects)
        .icon("cad:move")
        .noundo()
        .doc_only()
        .params("interactive: pick objects, a start point and an end point"),
    CommandSpec::new("obj.copy", "Copy", objects)
        .icon("cad:copy")
        .noundo()
        .doc_only()
        .params("interactive: as Move, keeping the objects where they were"),
    CommandSpec::new("obj.rotate", "Rotate", objects)
        .icon("cad:rotate")
        .noundo()
        .doc_only()
        .params("interactive: pick objects, an axis and an angle"),
    CommandSpec::new("obj.mirror", "Mirror", objects)
        .icon("cad:mirror")
        .noundo()
        .doc_only()
        .params("interactive: pick objects and a mirror plane or face"),
    CommandSpec::new("obj.join", "Join", objects)
        .icon("cad:join")
        .noundo()
        .doc_only()
        .params("interactive: join objects into one (objects stay separate until joined)"),
    CommandSpec::new("obj.unjoin", "Unjoin", objects)
        .icon("cad:explode")
        .noundo()
        .doc_only()
        .params("interactive: pick a joined object: its last join is undone"),
    CommandSpec::new("obj.cut", "Cut", objects).icon("cad:trim").noundo().doc_only().params("interactive: cut an object with others (they stay)"),
    CommandSpec::new("form.extrusion", "Extrusion", extrusion)
        .icon("extrude")
        .noundo()
        .doc_only()
        .params("interactive: sketch closed loops on the work plane, Finish extrudes them"),
    CommandSpec::new("form.blend", "Blend", blend)
        .icon("loft")
        .noundo()
        .doc_only()
        .params("interactive: sketch the base loop on the work plane, then the top loop on a parallel plane; Finish blends them"),
    CommandSpec::new("form.revolve", "Revolve", revolve)
        .icon("revolve")
        .noundo()
        .doc_only()
        .params("interactive: sketch closed loops and an axis line (centerline) on the work plane; Finish revolves them"),
    CommandSpec::new("form.sweep", "Sweep", sweep)
        .icon("sweep")
        .noundo()
        .doc_only()
        .params("interactive: sketch the path on the work plane, then the profile across its start; Finish sweeps it"),
    CommandSpec::new("form.swept_blend", "Swept Blend", swept_blend)
        .icon("pipe")
        .noundo()
        .doc_only()
        .params("interactive: sketch the path, then a profile across each end; Finish blends them along the path"),
    CommandSpec::new("form.void.extrusion", "Void Extrusion", void_extrusion)
        .icon("extrude")
        .noundo()
        .doc_only()
        .params("interactive: as form.extrusion, cutting"),
    CommandSpec::new("form.void.blend", "Void Blend", void_blend).icon("loft").noundo().doc_only().params("interactive: as form.blend, cutting"),
    CommandSpec::new("form.void.revolve", "Void Revolve", void_revolve)
        .icon("revolve")
        .noundo()
        .doc_only()
        .params("interactive: as form.revolve, cutting"),
    CommandSpec::new("form.void.sweep", "Void Sweep", void_sweep).icon("sweep").noundo().doc_only().params("interactive: as form.sweep, cutting"),
    CommandSpec::new("form.void.swept_blend", "Void Swept Blend", void_swept_blend)
        .icon("pipe")
        .noundo()
        .doc_only()
        .params("interactive: as form.swept_blend, cutting"),
    CommandSpec::new("workplane.set", "Set Work Plane", set_work_plane)
        .icon("plane")
        .noundo()
        .doc_only()
        .params("interactive: pick an origin or construction plane or any planar face; the form tools and Model Line sketch on it"),
    CommandSpec::new("workplane.reference", "Reference Plane", reference_plane)
        .icon("plane_perpendicular")
        .noundo()
        .doc_only()
        .params("interactive: two clicks draw a reference plane through them, square to the view; it shows as a dashed line where it is seen edge-on and can be the work plane"),
    CommandSpec::new("workplane.show", "Show Work Plane", show_work_plane)
        .icon("eye")
        .noundo()
        .doc_only()
        .params("interactive: show or hide the work plane's grid"),
    CommandSpec::new("hatch.pattern", "Hatch Pattern", hatch_pattern)
        .icon("hatch_pattern")
        .noundo()
        .doc_only()
        .params("interactive: choose a pattern, its scale and angle, then click faces to hatch them (Shift+click removes); hatch.apply is the programmatic form"),
    CommandSpec::new("workplane.show_voids", "Show Void Forms", show_references)
        .icon("eye")
        .noundo()
        .doc_only()
        .params("interactive: show the shapes the void forms cut away; double-click one to edit its sketches"),
    CommandSpec::new("workplane.show_references", "Show Reference Planes", show_references)
        .icon("eye")
        .noundo()
        .doc_only()
        .params("interactive: show every reference plane, as an outline where it is not seen edge-on"),
    CommandSpec::new("view.auto_ortho_faces", "Ortho on Faces", window_setting)
        .icon("camera")
        .noundo()
        .doc_only()
        .params("window setting: perspective while orbiting, orthographic when looking square at a face"),
    CommandSpec::new("view.tool_cursor", "Tool at Cursor", window_setting)
        .icon("select")
        .noundo()
        .doc_only()
        .params("window setting: show the running tool's icon at the cursor"),
    CommandSpec::new("view.agent_cursor", "Assistant Cursor", window_setting)
        .icon("select")
        .noundo()
        .doc_only()
        .params("window setting: show the assistant's cursor while it works"),
    CommandSpec::new("model.line", "Model Line", model_line)
        .icon("line")
        .noundo()
        .doc_only()
        .params("interactive: draw lines on the work plane (a sketch no form uses)"),
];

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
        s.execute(cmd, &params).unwrap_or_else(|e| panic!("{cmd}: {e}"))
    }

    /// An Extrusion 10×10×5 named `Extrusion 1`, its sketch on XY.
    fn form() -> Session {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Extrusion 1"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0.0, 0.0], "p1": [10.0, 10.0]}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "solid.extrude",
            json!({"sketch": "Extrusion 1", "distance": 5, "operation": "new_body", "body_name": "Extrusion 1", "name": "Extrusion 1"}),
        );
        s
    }

    /// The body's bounds, min and max corners.
    fn bounds(s: &Session, name: &str) -> ([f64; 3], [f64; 3]) {
        let st = s.model.state();
        let b = st.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no {name}"));
        let (m, place) = b.mesh_in_place();
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &m.positions {
            let p = solidcraft_doc::apply_point(&place, *p);
            for (i, v) in [p.x, p.y, p.z].into_iter().enumerate() {
                lo[i] = lo[i].min(v);
                hi[i] = hi[i].max(v);
            }
        }
        (lo, hi)
    }

    fn near(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    /// Moving a form moves its sketch onto a plane of its own: the body is where it was moved and
    /// its sketch, edited again, is there too.
    #[test]
    fn a_moved_form_keeps_its_sketch_with_it() {
        let mut s = form();
        run(&mut s, "form.transform", json!({"bodies": ["Extrusion 1"], "translate": [0, 0, 20]}));
        assert!(near(bounds(&s, "Extrusion 1").0, [0.0, 0.0, 20.0]), "{:?}", bounds(&s, "Extrusion 1"));
        let sketch = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. })).map(|f| f.id).unwrap();
        assert!((s.model.state().sketch(sketch).unwrap().plane.origin.z - 20.0).abs() < 1e-9);
        // Turned a quarter about Z through the origin.
        run(&mut s, "form.transform", json!({"bodies": ["Extrusion 1"], "rotate": {"origin": [0, 0, 0], "axis": [0, 0, 1], "angle": 90}}));
        let (lo, hi) = bounds(&s, "Extrusion 1");
        assert!(near(lo, [-10.0, 0.0, 20.0]) && near(hi, [0.0, 10.0, 25.0]), "{lo:?} {hi:?}");
        // Still editable: a longer extrusion grows from the moved sketch.
        let f = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Extrude { .. })).map(|f| f.id).unwrap();
        run(&mut s, "timeline.redefine", json!({"feature": f, "command": "solid.extrude", "params": {"distance": 8}}));
        assert!(near(bounds(&s, "Extrusion 1").1, [0.0, 10.0, 28.0]), "{:?}", bounds(&s, "Extrusion 1"));
    }

    /// A copy is a new form of its own, named as asked, with its own sketch; a mirrored copy is the
    /// mirror image (the extrusion goes the mirrored way).
    #[test]
    fn copies_and_mirrors_are_forms_of_their_own() {
        let mut s = form();
        run(&mut s, "form.transform", json!({"bodies": ["Extrusion 1"], "translate": [20, 0, 0], "copy": true, "names": ["Extrusion 2"]}));
        assert!(near(bounds(&s, "Extrusion 2").0, [20.0, 0.0, 0.0]));
        assert!(near(bounds(&s, "Extrusion 1").0, [0.0, 0.0, 0.0]), "the original stays");
        let sketches = s.doc.features.iter().filter(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. })).count();
        assert_eq!(sketches, 2, "the copy has its own sketch");
        // Mirrored in the plane z = -1: the copy lies below, from z = -7 to -2.
        run(
            &mut s,
            "form.transform",
            json!({"bodies": ["Extrusion 1"], "mirror": {"origin": [0, 0, -1], "normal": [0, 0, 1]}, "copy": true, "names": ["Extrusion 3"]}),
        );
        let (lo, hi) = bounds(&s, "Extrusion 3");
        assert!(near(lo, [0.0, 0.0, -7.0]) && near(hi, [10.0, 10.0, -2.0]), "{lo:?} {hi:?}");
        // A box made otherwise is not a form.
        assert!(s.execute("form.transform", &json!({"bodies": ["nope"], "translate": [1, 0, 0]})).is_err());
    }
}
