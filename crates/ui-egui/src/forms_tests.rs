//! The MODEL tab's form tools (`forms.rs`): each opens a sketch on the work plane, Finish leads to
//! the next sketch or the feature window filled in, and OK builds the form.
use crate::dialogs::Kind;
use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{FeatureKind, Operation};
use solidcraft_engine::geom::prelude::*;

fn ok(h: &mut Harness) {
    h.frames(3);
    h.step(&json!({"widget":"OK"})).unwrap();
    for _ in 0..300 {
        if h.app.dialog.is_none() {
            break;
        }
        h.frames(1);
    }
    assert!(h.app.dialog.is_none(), "{:?} {:?} {:?}", h.app.preview.error, h.app.dialog.as_ref().map(|d| (&d.error, &d.inputs)), h.app.session.log);
}

fn start(h: &mut Harness, id: &str) {
    h.app.start(id);
    h.frames(2);
    assert!(h.app.session.active_sketch.is_some(), "{id} opens a sketch: {:?}", h.app.status);
    assert_eq!(h.app.ui.tab, "SKETCH");
}

fn finish(h: &mut Harness) {
    h.app.finish_sketch();
    h.frames(3);
}

fn bodies(h: &Harness) -> usize {
    h.app.session.model.state().bodies.len()
}

fn last_kind(h: &Harness) -> FeatureKind {
    h.app.session.doc.features.last().unwrap().kind.clone()
}

#[test]
fn the_model_tab_has_the_form_tools() {
    let ids: Vec<&str> = crate::workspace::SECTIONS.iter().filter(|s| s.tab == "MODEL").flat_map(|s| s.ids.iter().copied()).collect();
    for id in ["form.extrusion", "form.blend", "form.revolve", "form.sweep", "form.swept_blend", "form.void.extrusion", "workplane.set", "model.line"] {
        assert!(ids.contains(&id), "{id}");
        assert!(solidcraft_engine::find_command(id).is_some(), "{id} is registered");
    }
    assert!(!ids.contains(&"sketch.create"), "a sketch starts only from a form tool");
}

#[test]
fn extrusion_sketches_then_extrudes_its_loops() {
    let mut h = Harness::new();
    start(&mut h, "form.extrusion");
    // Finish with nothing drawn is refused: the sketch stays open.
    finish(&mut h);
    assert!(h.app.session.active_sketch.is_some(), "an empty sketch is not finished");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [20, 10], "radius": 5})).unwrap();
    finish(&mut h);
    assert!(h.app.session.active_sketch.is_none());
    let d = h.app.dialog.as_ref().expect("Extrude opened");
    assert!(matches!(d.kind, Kind::Extrude { .. }), "{:?}", d.kind);
    assert_eq!(d.inputs[0].items.len(), 1, "the circle inside the rectangle is a hole: {:?}", d.inputs[0].items);
    ok(&mut h);
    assert_eq!(bodies(&h), 1);
}

#[test]
fn a_void_extrusion_cuts() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 40, "height": 20, "corner": [-20, -20, 0], "body_name": "Block"})).unwrap();
    start(&mut h, "form.void.extrusion");
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 5})).unwrap();
    finish(&mut h);
    let d = h.app.dialog.as_ref().expect("Extrude opened");
    assert!(matches!(d.kind, Kind::Extrude { operation: 2, .. }), "{:?}", d.kind);
    let k = h.app.dialog.as_mut().unwrap();
    if let Kind::Extrude { distance, direction, .. } = &mut k.kind {
        *distance = "30 mm".into();
        *direction = 0;
    }
    ok(&mut h);
    match last_kind(&h) {
        FeatureKind::Extrude { operation, .. } => assert_eq!(operation, Operation::Cut),
        k => panic!("{k:?}"),
    }
}

#[test]
fn blend_takes_a_base_and_a_top_sketch() {
    let mut h = Harness::new();
    start(&mut h, "form.blend");
    let base = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [-10, -10], "p1": [10, 10]})).unwrap();
    finish(&mut h);
    let top = h.app.session.active_sketch.expect("the top sketch opened");
    assert_ne!(base, top);
    let st = crate::open_meshing::session_world(&h.app.session);
    let (pb, pt) = (st.sketch(base).unwrap().plane, st.sketch(top).unwrap().plane);
    assert!((pt.origin - pb.origin).dot(pb.normal()) > 1.0, "the top is above the base");
    drop(st);
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 6})).unwrap();
    finish(&mut h);
    let d = h.app.dialog.as_ref().expect("Loft opened");
    assert!(matches!(d.kind, Kind::Loft { .. }), "{:?}", d.kind);
    assert_eq!(d.inputs[0].items, vec![Sel::Profile { sketch: base, index: 0 }, Sel::Profile { sketch: top, index: 0 }]);
    ok(&mut h);
    assert_eq!(bodies(&h), 1);
}

#[test]
fn cancelling_a_blend_removes_its_base() {
    let mut h = Harness::new();
    start(&mut h, "form.blend");
    let base = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 6})).unwrap();
    finish(&mut h);
    assert!(h.app.session.active_sketch.is_some());
    h.app.cancel_sketch();
    h.frames(2);
    assert!(h.app.session.active_sketch.is_none());
    assert!(h.app.session.doc.feature(base).is_none(), "the base sketch went with the form");
}

#[test]
fn revolve_needs_an_axis_line() {
    let mut h = Harness::new();
    start(&mut h, "form.revolve");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [10, 0], "p1": [20, 30]})).unwrap();
    finish(&mut h);
    assert!(h.app.session.active_sketch.is_some(), "no axis yet: the sketch stays open");
    h.app.run("sketch.line", json!({"points": [[0, -5], [0, 40]], "ids": ["ax"]})).unwrap();
    h.app.run("sketch.centerline", json!({"curves": ["ax"], "value": true})).unwrap();
    finish(&mut h);
    let d = h.app.dialog.as_ref().expect("Revolve opened");
    assert!(matches!(d.kind, Kind::Revolve { .. }), "{:?}", d.kind);
    assert_eq!(d.inputs[1].items.len(), 1, "the axis is filled in");
    ok(&mut h);
    assert_eq!(bodies(&h), 1);
}

#[test]
fn sweep_takes_a_path_then_a_profile_across_it() {
    let mut h = Harness::new();
    start(&mut h, "form.sweep");
    let path = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [50, 0]]})).unwrap();
    h.app.run("sketch.arc.three_point", json!({"start": [50, 0], "end": [80, 30], "through": [71.2, 8.8]})).unwrap();
    finish(&mut h);
    let profile = h.app.session.active_sketch.expect("the profile sketch opened");
    assert_ne!(path, profile);
    let st = crate::open_meshing::session_world(&h.app.session);
    let plane = st.sketch(profile).unwrap().plane;
    assert!(plane.normal().dot(solidcore_record::Vec3::X).abs() > 0.99, "square to the path's start: {plane:?}");
    drop(st);
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 3})).unwrap();
    finish(&mut h);
    let d = h.app.dialog.as_ref().expect("Sweep opened");
    assert!(matches!(d.kind, Kind::Sweep { .. }), "{:?}", d.kind);
    assert_eq!(d.inputs[1].items.len(), 2, "both path curves: {:?}", d.inputs[1].items);
    ok(&mut h);
    assert_eq!(bodies(&h), 1);
}

#[test]
fn sweep_takes_a_rectangle_profile_too() {
    let mut h = Harness::new();
    start(&mut h, "form.void.sweep");
    h.app.start("form.sweep");
    h.frames(2);
    h.app.run("sketch.line", json!({"points": [[0, 0], [50, 0], [50, 40]]})).unwrap();
    finish(&mut h);
    h.app.run("sketch.rectangle.two_point", json!({"p0": [-3, -2], "p1": [3, 2]})).unwrap();
    finish(&mut h);
    ok(&mut h);
    assert_eq!(bodies(&h), 1);
    assert_eq!(h.app.session.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Sketch { .. })).count(), 2, "the void's sketch went when Sweep started");
}

#[test]
fn swept_blend_takes_a_profile_at_each_end() {
    let mut h = Harness::new();
    start(&mut h, "form.swept_blend");
    h.app.run("sketch.line", json!({"points": [[0, 0], [60, 0]]})).unwrap();
    finish(&mut h);
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 6})).unwrap();
    finish(&mut h);
    let second = h.app.session.active_sketch.expect("the second profile sketch");
    let st = crate::open_meshing::session_world(&h.app.session);
    assert!((st.sketch(second).unwrap().plane.origin.x - 60.0).abs() < 1e-6, "at the path's end");
    drop(st);
    h.app.run("sketch.rectangle.two_point", json!({"p0": [-4, -4], "p1": [4, 4]})).unwrap();
    finish(&mut h);
    let d = h.app.dialog.as_ref().expect("Loft opened");
    assert!(matches!(d.kind, Kind::Loft { .. }), "{:?}", d.kind);
    ok(&mut h);
    assert_eq!(bodies(&h), 1);
}

#[test]
fn model_line_is_a_sketch_no_form_uses() {
    let mut h = Harness::new();
    start(&mut h, "model.line");
    h.app.run("sketch.line", json!({"points": [[0, 0], [10, 5]]})).unwrap();
    finish(&mut h);
    assert!(h.app.session.active_sketch.is_none() && h.app.dialog.is_none());
    assert!(matches!(last_kind(&h), FeatureKind::Sketch { .. }));
}

#[test]
fn a_face_set_as_the_work_plane_takes_the_next_sketch() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 40, "height": 20, "corner": [0, 0, 0], "body_name": "Block"})).unwrap();
    h.frames(2);
    // The block's top face, picked as the work plane.
    let state = h.app.session.model.state();
    let body = state.bodies.iter().find(|b| b.name == "Block").unwrap();
    let face = body.body.faces(1e-3).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99)).unwrap();
    let pick = Sel::Face { body: "Block".into(), index: face.index, point: face.centroid };
    drop(state);
    h.app.start("workplane.set");
    h.frames(1);
    assert!(h.app.forms.picking_work_plane);
    h.app.session.selection = vec![pick.clone()];
    h.frames(2);
    assert!(!h.app.forms.picking_work_plane);
    assert_eq!(h.app.forms.work_plane, Some(pick));
    start(&mut h, "form.extrusion");
    let st = crate::open_meshing::session_world(&h.app.session);
    let plane = st.sketch(h.app.session.active_sketch.unwrap()).unwrap().plane;
    assert!((plane.origin.z - 20.0).abs() < 1e-6 && plane.normal().z.abs() > 0.99, "{plane:?}");
    drop(st);
    // Sketch mode: the block can't be picked, only the sketch.
    assert!(crate::forms::isolating(&h.app));
    assert!(crate::forms::halftone(&h.app));
}
