//! The MODEL tab's form tools (`forms.rs`): each opens a sketch on the work plane, Finish leads to
//! the next sketch or builds the form at once, its values in Properties (no window).
use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{FeatureKind, Operation};
use solidcraft_engine::geom::prelude::*;

/// The form is built when its last sketch finishes: no window opens; wait for the model.
fn ok(h: &mut Harness) {
    h.frames(3);
    assert!(h.app.dialog.is_none(), "no feature window opens: {:?}", h.app.dialog.as_ref().map(|d| &d.kind));
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
    for id in ["form.extrusion", "form.blend", "form.revolve", "form.sweep", "form.swept_blend", "form.void.extrusion", "workplane.set", "model.line"]
    {
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
    ok(&mut h);
    assert_eq!(bodies(&h), 1);
    match last_kind(&h) {
        FeatureKind::Extrude { operation, .. } => assert_eq!(operation, Operation::NewBody),
        k => panic!("{k:?}"),
    }
    let body = h.app.session.model.state().bodies[0].name.clone();
    assert_eq!(body, "Extrusion 1", "a body is named after its tool");
    assert!(h.app.shell_ui.properties, "its values are in Properties");
}

#[test]
fn a_void_extrusion_cuts() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 40, "height": 20, "corner": [-20, -20, 0], "body_name": "Block"})).unwrap();
    start(&mut h, "form.void.extrusion");
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 5})).unwrap();
    finish(&mut h);
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
    ok(&mut h);
    assert_eq!(bodies(&h), 1);
    match last_kind(&h) {
        FeatureKind::Loft { sections, .. } => assert_eq!(sections.iter().map(|s| s.sketch.0).collect::<Vec<_>>(), [base, top]),
        k => panic!("{k:?}"),
    }
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
fn revolve_draws_its_axis_as_the_next_stage() {
    let mut h = Harness::new();
    start(&mut h, "form.revolve");
    let sketch = h.app.session.active_sketch.unwrap();
    assert!(crate::forms::finish_is_next(&h.app), "the profile's Finish is Next");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [10, 0], "p1": [20, 30]})).unwrap();
    finish(&mut h);
    // The axis stage: the same sketch, a line only, and Finish is the last.
    assert_eq!(h.app.session.active_sketch, Some(sketch));
    assert!(!crate::forms::finish_is_next(&h.app));
    assert!(crate::forms::blocked_in_form(&h.app, "cad.circle").is_some(), "the axis is a line only");
    finish(&mut h);
    assert_eq!(h.app.session.active_sketch, Some(sketch), "no axis yet: the sketch stays open");
    // A plain line becomes the axis (a centerline), not a profile edge.
    h.app.run("sketch.line", json!({"points": [[0, -5], [0, 40]], "ids": ["ax"]})).unwrap();
    finish(&mut h);
    ok(&mut h);
    assert!(matches!(last_kind(&h), FeatureKind::Revolve { .. }));
    assert_eq!(bodies(&h), 1);
    // Edited again: Profile, then Axis (Next on the first).
    let body = h.app.session.model.state().bodies[0].name.clone();
    assert!(crate::objects::edit_object_sketch(&mut h.app, &body, None));
    h.frames(2);
    assert!(crate::forms::finish_is_next(&h.app));
    finish(&mut h);
    assert_eq!(h.app.session.active_sketch, Some(sketch), "the axis stage, same sketch");
    assert!(!crate::forms::finish_is_next(&h.app));
    finish(&mut h);
    assert!(h.app.session.active_sketch.is_none(), "{:?}", h.app.status);
    assert_eq!(bodies(&h), 1);
}

/// A Blend edited again goes through both its sketches: Next on the base opens the top.
#[test]
fn a_blend_edited_again_goes_through_both_sketches() {
    let mut h = Harness::new();
    start(&mut h, "form.blend");
    let base = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 20]})).unwrap();
    assert!(crate::forms::finish_is_next(&h.app));
    finish(&mut h);
    let top = h.app.session.active_sketch.unwrap();
    assert!(!crate::forms::finish_is_next(&h.app));
    h.app.run("sketch.circle.center", json!({"center": [10, 10], "radius": 5})).unwrap();
    finish(&mut h);
    ok(&mut h);
    let body = h.app.session.model.state().bodies[0].name.clone();
    assert!(crate::objects::edit_object_sketch(&mut h.app, &body, None));
    h.frames(2);
    assert_eq!(h.app.session.active_sketch, Some(base));
    assert!(crate::forms::finish_is_next(&h.app));
    finish(&mut h);
    assert_eq!(h.app.session.active_sketch, Some(top), "Next opened the top");
    // The top redrawn: a bigger circle; Finish rebuilds the blend from it.
    let st = crate::open_meshing::session_world(&h.app.session);
    let ids: Vec<String> = st.sketch(top).unwrap().sketch.curves.iter().map(|c| c.id.clone()).collect();
    drop(st);
    h.app.run("sketch.delete", json!({"entities": ids})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [10, 10], "radius": 8})).unwrap();
    finish(&mut h);
    assert!(h.app.session.active_sketch.is_none(), "{:?}", h.app.status);
    assert!(h.app.forms.chain.is_none());
    let f = h.app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Loft { .. })).map(|f| f.id).unwrap();
    assert!(h.app.session.model.result(f).and_then(|r| r.error.clone()).is_none(), "the blend follows its new top");
}

/// A reference plane's line can be changed until Finish: it is not made on the first line.
#[test]
fn a_reference_plane_waits_for_finish() {
    let mut h = Harness::new();
    h.app.start("workplane.reference");
    h.frames(5);
    let sk = h.app.session.active_sketch.expect("a sketch");
    h.app.run("sketch.line", json!({"points": [[0, 0], [10, 0]]})).unwrap();
    h.frames(5);
    assert_eq!(h.app.session.active_sketch, Some(sk), "still open after the line");
    finish(&mut h);
    assert!(h.app.session.active_sketch.is_none());
    assert!(solidcraft_engine::view::construction_planes(&h.app.session).iter().any(|(_, n, _)| crate::reference_planes::is_reference(n)));
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
    ok(&mut h);
    assert!(matches!(last_kind(&h), FeatureKind::Sweep { .. }));
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
    assert_eq!(
        h.app.session.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Sketch { .. })).count(),
        2,
        "the void's sketch went when Sweep started"
    );
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
    ok(&mut h);
    assert!(matches!(last_kind(&h), FeatureKind::Loft { .. }));
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

#[test]
fn an_edited_extrusion_sketch_rebuilds_the_object() {
    let mut h = Harness::new();
    start(&mut h, "form.extrusion");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    finish(&mut h);
    ok(&mut h);
    assert_eq!(bodies(&h), 1);
    let body = crate::open_meshing::session_world(&h.app.session).bodies.first().map(|b| b.name.clone()).unwrap();
    let area = |h: &Harness| {
        let st = crate::open_meshing::session_world(&h.app.session);
        let m = st.bodies.first().unwrap().mesh();
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for p in &m.positions {
            lo = lo.min(p.x);
            hi = hi.max(p.x);
        }
        hi - lo
    };
    assert!((area(&h) - 10.0).abs() < 1e-6);
    // Double-click: its sketch opens; a loop drawn around the old one, the old one deleted.
    crate::objects::edit_object(&mut h.app, &body);
    h.frames(2);
    assert!(h.app.session.active_sketch.is_some());
    h.app.run("sketch.rectangle.two_point", json!({"p0": [-10, -10], "p1": [20, 20]})).unwrap();
    finish(&mut h);
    h.frames(3);
    // The depth window opens; OK keeps it.
    if h.app.dialog.is_some() {
        ok(&mut h);
    }
    h.frames(2);
    // The outer loop takes over: the inner one is now a hole and the object is 30 wide.
    assert!((area(&h) - 30.0).abs() < 1e-6, "{}", area(&h));
}

#[test]
fn deleting_a_blend_body_takes_its_sketches() {
    let mut h = Harness::new();
    start(&mut h, "form.blend");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [-10, -10], "p1": [10, 10]})).unwrap();
    finish(&mut h);
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 6})).unwrap();
    finish(&mut h);
    ok(&mut h);
    assert_eq!(bodies(&h), 1);
    let body = h.app.session.model.state().bodies[0].name.clone();
    h.app.session.selection = vec![Sel::Body { name: body }];
    crate::delete::delete_selection(&mut h.app);
    h.frames(20);
    assert_eq!(bodies(&h), 0);
    let sketches = h.app.session.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Sketch { .. })).count();
    assert_eq!(sketches, 0, "the Blend's sketches went with it: {:?}", h.app.menu.confirm.as_ref().map(|c| &c.report));
}

#[test]
fn a_blend_shows_start_end_and_distance_and_takes_a_new_distance() {
    let mut h = Harness::new();
    start(&mut h, "form.blend");
    let base = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [-10, -10], "p1": [10, 10]})).unwrap();
    finish(&mut h);
    let top = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 6})).unwrap();
    finish(&mut h);
    ok(&mut h);
    let loft = h.app.session.doc.features.last().unwrap().id;
    let gap = |h: &Harness| {
        let st = crate::open_meshing::session_world(&h.app.session);
        let (b, t) = (st.sketch(base).unwrap().plane, st.sketch(top).unwrap().plane);
        (t.origin - b.origin).dot(b.normal())
    };
    let rows = crate::appshell::blend_rows_for_tests(&mut h.app, loft).unwrap();
    assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), ["Start", "End", "Distance"]);
    crate::appshell::set_blend_for_tests(&mut h.app, loft, 2, "35").unwrap();
    h.frames(5);
    assert!((gap(&h) - 35.0).abs() < 1e-6, "{}", gap(&h));
    crate::appshell::set_blend_for_tests(&mut h.app, loft, 0, "5 mm").unwrap();
    h.frames(5);
    assert!((gap(&h) - 30.0).abs() < 1e-6, "Start moves the base only: {}", gap(&h));
    assert_eq!(bodies(&h), 1, "the Blend rebuilt: {:?}", h.app.session.log.last());
}

#[test]
fn a_sweep_path_may_be_open_when_drawn_with_the_line_tool() {
    use crate::cad::Input;
    let mut h = Harness::new();
    start(&mut h, "form.sweep");
    let pt = |x: f64, y: f64| Input::Point { p: solidcore_record::Vec2::new(x, y), curve: None };
    for p in [pt(0.0, 0.0), pt(40.0, 0.0), pt(40.0, 30.0)] {
        crate::cad::feed(&mut h.app, p);
    }
    crate::cad::feed(&mut h.app, Input::Enter);
    finish(&mut h);
    let profile = h.app.session.active_sketch;
    assert!(profile.is_some(), "an open path goes on to the profile: {:?}", h.app.status);
    h.app.run("sketch.circle.center", json!({"center": [0, 0], "radius": 3})).unwrap();
    finish(&mut h);
    ok(&mut h);
    assert_eq!(bodies(&h), 1);
}

#[test]
fn the_reference_plane_tool_keeps_the_view_and_sketches_square_to_it() {
    let mut h = Harness::new();
    h.app.cam.set_view(solidcraft_engine::render::StandardView::Iso);
    h.app.cam_anim = None;
    let before = h.app.cam;
    h.app.start("workplane.reference");
    h.frames(10);
    let sk = h.app.session.active_sketch.expect("a sketch");
    assert_eq!(h.app.settled_cam().yaw, before.yaw);
    assert_eq!(h.app.settled_cam().pitch, before.pitch);
    let st = crate::open_meshing::session_world(&h.app.session);
    let n = st.sketch(sk).unwrap().plane.normal();
    assert!(n.dot(before.basis().2).abs() > 0.999, "square to the view: {n:?}");
}

/// An Extrusion body and its sketch are one object: the sketch is named after it, hidden, kept
/// out of the browser's Features, and deleted with it.
#[test]
fn an_extrusion_body_owns_its_sketch() {
    let mut h = Harness::new();
    start(&mut h, "form.extrusion");
    let sketch = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    finish(&mut h);
    ok(&mut h);
    let name = h.app.session.doc.feature(sketch).map(|f| f.name.clone()).unwrap();
    assert!(name.starts_with("Extrusion 1"), "{name}");
    assert!(!crate::viewport::visible_sketches(&h.app).contains(&sketch), "a form's sketch is hidden");
    let f = h.app.session.doc.feature(sketch).unwrap();
    assert!(!crate::browser::features::listed_feature(f), "not in Features (by default)");
    let body = h.app.session.model.state().bodies[0].name.clone();
    h.app.session.selection = vec![Sel::Body { name: body }];
    crate::delete::delete_selection(&mut h.app);
    h.frames(20);
    assert_eq!(bodies(&h), 0);
    assert!(h.app.session.doc.feature(sketch).is_none(), "the sketch went with the body");
}

/// Two extrusions, the second drawn on the first one's top face, stay two bodies: a form never
/// joins another body by touching it.
#[test]
fn a_form_on_another_bodys_face_stays_its_own_body() {
    let mut h = Harness::new();
    start(&mut h, "form.extrusion");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 20]})).unwrap();
    finish(&mut h);
    ok(&mut h);
    let state = h.app.session.model.state();
    let body = state.bodies.first().unwrap();
    let face = body.body.faces(1e-3).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99)).unwrap();
    let pick = Sel::Face { body: body.name.clone(), index: face.index, point: face.centroid };
    drop(state);
    h.app.forms.work_plane = Some(pick);
    start(&mut h, "form.extrusion");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [15, 15]})).unwrap();
    finish(&mut h);
    ok(&mut h);
    assert_eq!(bodies(&h), 2, "{:?}", h.app.session.log.last());
    // The first body edited again: the second neither moves nor joins it.
    let first = h.app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Extrude { .. })).map(|f| f.id).unwrap();
    let top_z = |h: &Harness| {
        let st = h.app.session.model.state();
        let b = st.bodies.iter().find(|b| b.name == "Extrusion 2").unwrap();
        let (m, place) = b.mesh_in_place();
        m.positions.iter().map(|p| solidcraft_engine::doc::apply_point(&place, *p).z).fold(f64::NEG_INFINITY, f64::max)
    };
    let before = top_z(&h);
    h.app.run("timeline.redefine", json!({"feature": first, "command": "solid.extrude", "params": {"distance": "25 mm"}})).unwrap();
    h.frames(5);
    assert_eq!(bodies(&h), 2, "{:?}", h.app.session.log.last());
    // The second stands on the first's top face: it rose with it.
    assert!((top_z(&h) - before - 15.0).abs() < 1e-6, "{} → {}", before, top_z(&h));
}

/// A joined object lists the objects it is made of, and each one is edited again without
/// unjoining: its sketch opens, Finish rebuilds the joined object.
#[test]
fn a_joined_object_lists_and_edits_its_objects() {
    let mut h = Harness::new();
    start(&mut h, "form.extrusion");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 20]})).unwrap();
    finish(&mut h);
    ok(&mut h);
    start(&mut h, "form.extrusion");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [30, 30]})).unwrap();
    finish(&mut h);
    ok(&mut h);
    h.app.run("solid.combine", json!({"target": "Extrusion 1", "tools": ["Extrusion 2"], "operation": "join"})).unwrap();
    h.frames(3);
    assert_eq!(bodies(&h), 1);
    let members = crate::objects::join_members(&h.app, "Extrusion 1");
    assert_eq!(members.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["Extrusion 1", "Extrusion 2"]);
    let (name, feature) = members[1].clone();
    assert!(crate::objects::edit_member(&mut h.app, &name, feature));
    h.frames(2);
    assert!(h.app.session.active_sketch.is_some(), "Extrusion 2's sketch is open");
    finish(&mut h);
    assert!(h.app.session.active_sketch.is_none(), "{:?}", h.app.status);
    assert_eq!(bodies(&h), 1, "still one joined object");
}
