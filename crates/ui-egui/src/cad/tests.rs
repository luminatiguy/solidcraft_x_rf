//! The sketch tools after CADCraft's: their prompts take points, values and options in CADCraft's
//! order, and they leave plain geometry (no constraints).

use super::geo::Shape;
use super::{Input, feed, start_hook};
use crate::scenario::Harness;
use solidcore_record::Vec2;
use solidcraft_engine::geom::prelude::*;

fn sketching() -> Harness {
    let mut h = Harness::new();
    h.app.start("model.line");
    h.frames(2);
    assert!(h.app.session.active_sketch.is_some());
    assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("cad.line"), "a form's sketch starts with Line");
    h
}

fn pt(x: f64, y: f64) -> Input {
    Input::Point { p: Vec2::new(x, y), curve: None }
}

fn pick(id: &str, x: f64, y: f64) -> Input {
    Input::Point { p: Vec2::new(x, y), curve: Some(id.to_string()) }
}

fn tool(h: &mut Harness, id: &str) {
    assert!(start_hook(&mut h.app, id), "{id}");
}

fn ents(h: &Harness) -> Vec<super::geo::Ent> {
    super::ents(&h.app)
}

fn constraints(h: &Harness) -> usize {
    let sid = h.app.session.active_sketch.unwrap();
    let st = crate::open_meshing::session_world(&h.app.session);
    st.sketch(sid).unwrap().sketch.constraints.len()
}

fn lines(h: &Harness) -> Vec<(Vec2, Vec2)> {
    ents(h)
        .into_iter()
        .filter_map(|e| match e.shape {
            Shape::Line(a, b) => Some((a, b)),
            _ => None,
        })
        .collect()
}

fn id_of_line_near(h: &Harness, p: Vec2) -> String {
    ents(h).into_iter().filter(|e| matches!(e.shape, Shape::Line(..))).min_by(|a, b| a.shape.dist(p).total_cmp(&b.shape.dist(p))).unwrap().id
}

#[test]
fn line_draws_segment_by_segment_and_closes() {
    let mut h = sketching();
    for p in [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 5.0)] {
        feed(&mut h.app, p);
    }
    assert_eq!(lines(&h).len(), 2, "each click adds a line");
    feed(&mut h.app, Input::Keyword("Undo"));
    assert_eq!(lines(&h).len(), 1, "Undo takes the last one back");
    feed(&mut h.app, pt(10.0, 5.0));
    feed(&mut h.app, Input::Keyword("Close"));
    assert_eq!(lines(&h).len(), 3);
    assert!(h.app.tool.is_none(), "Close ends the tool");
    assert_eq!(constraints(&h), 0, "plain geometry");
}

#[test]
fn typed_values_are_points_and_distances() {
    let mut h = sketching();
    feed(&mut h.app, pt(0.0, 0.0));
    let prompt = h.app.cad.run.as_ref().unwrap().m.prompt(&h.app);
    let i = super::typed_input(&h.app, &prompt, "@10,0").unwrap();
    assert_eq!(i, pt(10.0, 0.0));
    let i = super::typed_input(&h.app, &prompt, "@5<90").unwrap();
    let Input::Point { p, .. } = i else { panic!() };
    assert!(p.dist(Vec2::new(0.0, 5.0)) < 1e-9);
    // Direct distance entry: toward the cursor.
    h.app.cad.cursor = Some(Vec2::new(3.0, 4.0));
    let Input::Point { p, .. } = super::typed_input(&h.app, &prompt, "10").unwrap() else { panic!() };
    assert!(p.dist(Vec2::new(6.0, 8.0)) < 1e-9);
}

#[test]
fn a_contour_left_open_cannot_be_finished() {
    let mut h = Harness::new();
    h.app.start("form.extrusion");
    h.frames(2);
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, pt(10.0, 0.0));
    feed(&mut h.app, pt(10.0, 10.0));
    feed(&mut h.app, Input::Enter);
    h.app.finish_sketch();
    h.frames(2);
    assert!(h.app.session.active_sketch.is_some(), "an open contour keeps the sketch open");
    assert_eq!(crate::forms::open_ends(&h.app, h.app.session.active_sketch.unwrap()).len(), 2);
    tool(&mut h, "cad.line");
    feed(&mut h.app, pt(10.0, 10.0));
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, Input::Enter);
    h.app.finish_sketch();
    h.frames(3);
    assert!(h.app.session.active_sketch.is_none());
    assert!(h.app.dialog.is_none(), "the form builds at once, its values in Properties");
    assert!(h.app.session.doc.features.iter().any(|f| matches!(f.kind, solidcraft_engine::doc::FeatureKind::Extrude { .. })));
}

#[test]
fn circle_takes_a_typed_radius_and_its_options() {
    let mut h = sketching();
    tool(&mut h, "cad.circle");
    feed(&mut h.app, pt(5.0, 5.0));
    feed(&mut h.app, Input::Number(3.0));
    tool(&mut h, "cad.circle");
    feed(&mut h.app, Input::Keyword("2P"));
    feed(&mut h.app, pt(20.0, 0.0));
    feed(&mut h.app, pt(30.0, 0.0));
    let circles: Vec<(Vec2, f64)> = ents(&h)
        .into_iter()
        .filter_map(|e| match e.shape {
            Shape::Circle(c, r) => Some((c, r)),
            _ => None,
        })
        .collect();
    assert_eq!(circles.len(), 2, "{circles:?}");
    assert!(circles.iter().any(|(c, r)| c.dist(Vec2::new(5.0, 5.0)) < 1e-6 && (r - 3.0).abs() < 1e-6));
    assert!(circles.iter().any(|(c, r)| c.dist(Vec2::new(25.0, 0.0)) < 1e-6 && (r - 5.0).abs() < 1e-6));
    assert_eq!(constraints(&h), 0);
}

#[test]
fn rectangle_with_fillets() {
    let mut h = sketching();
    tool(&mut h, "cad.rectang");
    feed(&mut h.app, Input::Keyword("Fillet"));
    feed(&mut h.app, Input::Number(2.0));
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, pt(20.0, 10.0));
    let e = ents(&h);
    assert_eq!(e.iter().filter(|e| matches!(e.shape, Shape::Line(..))).count(), 4);
    assert_eq!(e.iter().filter(|e| matches!(e.shape, Shape::Arc { .. })).count(), 4, "four rounded corners");
    assert_eq!(constraints(&h), 0);
    let sid = h.app.session.active_sketch.unwrap();
    assert!(crate::forms::open_ends(&h.app, sid).is_empty(), "a closed outline");
}

#[test]
fn polygon_by_sides_centre_and_radius() {
    let mut h = sketching();
    tool(&mut h, "cad.polygon");
    feed(&mut h.app, Input::Number(6.0));
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, Input::Keyword("Inscribed in circle"));
    feed(&mut h.app, Input::Number(10.0));
    let l = lines(&h);
    assert_eq!(l.len(), 6);
    for (a, b) in &l {
        assert!((a.len() - 10.0).abs() < 1e-6 && (b.len() - 10.0).abs() < 1e-6, "corners on the circle");
    }
    assert_eq!(constraints(&h), 0);
}

#[test]
fn move_copy_and_rotate_the_selection() {
    let mut h = sketching();
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, pt(10.0, 0.0));
    feed(&mut h.app, Input::Enter);
    let id = id_of_line_near(&h, Vec2::new(5.0, 0.0));
    tool(&mut h, "cad.move");
    feed(&mut h.app, pick(&id, 5.0, 0.0));
    feed(&mut h.app, Input::Enter);
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, pt(0.0, 5.0));
    let l = lines(&h);
    assert_eq!(l.len(), 1);
    assert!(l[0].0.dist(Vec2::new(0.0, 5.0)) < 1e-6 && l[0].1.dist(Vec2::new(10.0, 5.0)) < 1e-6, "{l:?}");
    tool(&mut h, "cad.copy");
    feed(&mut h.app, pick(&id, 5.0, 5.0));
    feed(&mut h.app, Input::Enter);
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, pt(0.0, 10.0));
    feed(&mut h.app, pt(0.0, 20.0));
    feed(&mut h.app, Input::Enter);
    assert_eq!(lines(&h).len(), 3, "two copies");
    tool(&mut h, "cad.rotate");
    feed(&mut h.app, pick(&id, 5.0, 5.0));
    feed(&mut h.app, Input::Enter);
    feed(&mut h.app, pt(0.0, 5.0));
    feed(&mut h.app, Input::Number(90.0));
    let l = lines(&h);
    assert!(l.iter().any(|(a, b)| a.dist(Vec2::new(0.0, 5.0)) < 1e-6 && b.dist(Vec2::new(0.0, 15.0)) < 1e-6), "{l:?}");
    assert_eq!(constraints(&h), 0);
}

#[test]
fn trim_fillet_and_offset() {
    let mut h = sketching();
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, pt(20.0, 0.0));
    feed(&mut h.app, Input::Enter);
    tool(&mut h, "cad.line");
    feed(&mut h.app, pt(10.0, -5.0));
    feed(&mut h.app, pt(10.0, 5.0));
    feed(&mut h.app, Input::Enter);
    // Trim the right half of the horizontal line at the vertical one.
    let horizontal = id_of_line_near(&h, Vec2::new(18.0, 0.0));
    tool(&mut h, "cad.trim");
    feed(&mut h.app, pick(&horizontal, 18.0, 0.0));
    feed(&mut h.app, Input::Enter);
    let l = lines(&h);
    assert!(
        l.iter().any(|(a, b)| (a.dist(Vec2::new(0.0, 0.0)) < 1e-6 && b.dist(Vec2::new(10.0, 0.0)) < 1e-6)
            || (b.dist(Vec2::new(0.0, 0.0)) < 1e-6 && a.dist(Vec2::new(10.0, 0.0)) < 1e-6)),
        "{l:?}"
    );
    assert!(!l.iter().any(|(a, b)| a.x > 10.5 || b.x > 10.5), "the right piece is gone: {l:?}");
    // Offset the vertical line 3 to the right.
    let vertical = id_of_line_near(&h, Vec2::new(10.0, 4.0));
    tool(&mut h, "cad.offset");
    feed(&mut h.app, Input::Number(3.0));
    feed(&mut h.app, pick(&vertical, 10.0, 4.0));
    feed(&mut h.app, pt(15.0, 0.0));
    feed(&mut h.app, Input::Enter);
    assert!(lines(&h).iter().any(|(a, b)| (a.x - 13.0).abs() < 1e-6 && (b.x - 13.0).abs() < 1e-6), "{:?}", lines(&h));
    assert_eq!(constraints(&h), 0, "the offset keeps no dimension or tie");
}

#[test]
fn fillet_rounds_a_corner() {
    let mut h = sketching();
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, pt(20.0, 0.0));
    feed(&mut h.app, Input::Enter);
    tool(&mut h, "cad.line");
    feed(&mut h.app, pt(20.0, 0.0));
    feed(&mut h.app, pt(20.0, 20.0));
    feed(&mut h.app, Input::Enter);
    let a = id_of_line_near(&h, Vec2::new(10.0, 0.0));
    let b = id_of_line_near(&h, Vec2::new(20.0, 10.0));
    tool(&mut h, "cad.fillet");
    feed(&mut h.app, Input::Keyword("Radius"));
    feed(&mut h.app, Input::Number(4.0));
    feed(&mut h.app, pick(&a, 10.0, 0.0));
    feed(&mut h.app, pick(&b, 20.0, 10.0));
    let arcs: Vec<f64> = ents(&h)
        .into_iter()
        .filter_map(|e| match e.shape {
            Shape::Arc { r, .. } => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(arcs.len(), 1, "{:?}", ents(&h));
    assert!((arcs[0] - 4.0).abs() < 1e-6);
    assert_eq!(constraints(&h), 0);
}

#[test]
fn arrays_and_mirror() {
    let mut h = sketching();
    tool(&mut h, "cad.circle");
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, Input::Number(1.0));
    let id = ents(&h)[0].id.clone();
    tool(&mut h, "cad.arrayrect");
    feed(&mut h.app, pick(&id, 1.0, 0.0));
    feed(&mut h.app, Input::Enter);
    for v in [3.0, 2.0, 5.0, 4.0] {
        feed(&mut h.app, Input::Number(v));
    }
    assert_eq!(ents(&h).len(), 6, "3 columns × 2 rows");
    tool(&mut h, "cad.mirror");
    feed(&mut h.app, pick(&id, 1.0, 0.0));
    feed(&mut h.app, Input::Enter);
    feed(&mut h.app, pt(-5.0, -10.0));
    feed(&mut h.app, pt(-5.0, 10.0));
    feed(&mut h.app, Input::Keyword("No"));
    let n = ents(&h);
    assert_eq!(n.len(), 7, "{n:?}");
    assert!(n.iter().any(|e| matches!(e.shape, Shape::Circle(c, _) if c.dist(Vec2::new(-10.0, 0.0)) < 1e-6)), "{n:?}");
}

#[test]
fn snaps_find_ends_middles_centres_and_crossings() {
    use super::snap;
    let own = vec![
        super::geo::Ent { id: "a".into(), shape: Shape::Line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)), construction: false },
        super::geo::Ent { id: "b".into(), shape: Shape::Line(Vec2::new(4.0, -5.0), Vec2::new(4.0, 5.0)), construction: false },
        super::geo::Ent { id: "c".into(), shape: Shape::Circle(Vec2::new(30.0, 0.0), 5.0), construction: false },
    ];
    let hit = |p: Vec2| snap::find(&own, &[], p, 0.5, None, false);
    assert_eq!(hit(Vec2::new(9.8, 0.1)).map(|h| (h.kind, h.p)), Some((snap::END, Vec2::new(10.0, 0.0))));
    assert_eq!(hit(Vec2::new(5.2, 0.1)).map(|h| h.kind), Some(snap::MID));
    assert_eq!(hit(Vec2::new(4.1, 0.1)).map(|h| (h.kind, h.p)), Some((snap::INT, Vec2::new(4.0, 0.0))));
    assert_eq!(hit(Vec2::new(30.2, -0.1)).map(|h| h.kind), Some(snap::CEN));
    assert_eq!(hit(Vec2::new(35.1, 0.2)).map(|h| h.kind), Some(snap::QUA));
    // Model edges count too.
    let model = vec![Shape::Line(Vec2::new(50.0, 0.0), Vec2::new(60.0, 0.0))];
    assert_eq!(snap::find(&own, &model, Vec2::new(59.9, 0.1), 0.5, None, false).map(|h| h.kind), Some(snap::END));
}

#[test]
fn dynamic_input_reads_distance_and_angle_from_the_last_point() {
    use super::{Accept, Prompt, Settings, dyn_entry, dyn_submit};
    let s = Settings::default();
    let p = Prompt::new("Specify next point", Accept { point: true, ..Default::default() }).base(Some(Vec2::new(10.0, 0.0)));
    let e = dyn_entry(&s, &p, "40").unwrap();
    assert!(e.polar && !e.first_locked && e.relative);
    // Locked distance: the cursor is held 40 from the base, toward where it points.
    let e = dyn_entry(&s, &p, "40<").unwrap();
    let held = e.apply(Vec2::new(10.0, 100.0));
    assert!((held.dist(Vec2::new(10.0, 0.0)) - 40.0).abs() < 1e-9);
    assert_eq!(dyn_submit(&s, &p, "40<90", Vec2::ZERO), "@40<90");
    assert_eq!(dyn_submit(&s, &p, "40", Vec2::ZERO), "40");
    assert_eq!(dyn_submit(&s, &p, "#5,5", Vec2::ZERO), "5,5");
    // The open angle box takes the cursor's angle.
    assert_eq!(dyn_submit(&s, &p, "40<", Vec2::new(10.0, 50.0)), "@40<90");
    // The first point: X and Y, absolute.
    let first = Prompt::new("Specify first point", Accept { point: true, ..Default::default() });
    let e = dyn_entry(&s, &first, "3,").unwrap();
    assert!(!e.polar && !e.relative);
    assert_eq!(dyn_submit(&s, &first, "3,4", Vec2::ZERO), "3,4");
}

#[test]
fn snap_modes_turn_kinds_off() {
    use super::snap;
    let own = vec![super::geo::Ent { id: "a".into(), shape: Shape::Line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)), construction: false }];
    let at = Vec2::new(10.05, 0.0);
    assert_eq!(snap::find_modes(&own, &[], at, 0.5, None, snap::DEFAULT_MODES).map(|h| h.kind), Some(snap::END));
    assert_eq!(snap::find_modes(&own, &[], at, 0.5, None, 1 << snap::MID).map(|h| h.kind), None);
}

fn body_box(h: &Harness, name: &str) -> (solidcore_record::Vec3, solidcore_record::Vec3) {
    let st = crate::open_meshing::session_world(&h.app.session);
    let b = st.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("{name}"));
    let m = b.mesh();
    let mut lo = solidcore_record::Vec3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut hi = solidcore_record::Vec3::new(f64::MIN, f64::MIN, f64::MIN);
    for p in &m.positions {
        lo = solidcore_record::Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = solidcore_record::Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    (lo, hi)
}

#[test]
fn design_move_copy_rotate_and_mirror_whole_objects() {
    let mut h = Harness::new();
    h.app.run("solid.box", serde_json::json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let name = super::objects::selected_bodies(&h.app)
        .first()
        .cloned()
        .or_else(|| crate::open_meshing::session_world(&h.app.session).bodies.first().map(|b| b.name.clone()))
        .unwrap();
    let (lo0, _) = body_box(&h, &name);
    // Move: picked as an object, then base point and second point on the work plane (XY).
    tool(&mut h, "obj.move");
    assert!(h.app.cad.objects);
    feed(&mut h.app, Input::Object(name.clone()));
    feed(&mut h.app, Input::Enter);
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, pt(20.0, 0.0));
    assert!(h.app.cad.run.is_none(), "move ends");
    let (lo, _) = body_box(&h, &name);
    assert!((lo.x - lo0.x - 20.0).abs() < 1e-6, "{lo:?} {lo0:?}");
    // Rotate by 90° about the box's corner it now has at (lo.x, lo.y).
    let _ = h.app.session.execute("select.set", &serde_json::json!({"items": [{"body": {"name": name}}]}));
    tool(&mut h, "obj.rotate");
    let corner = (lo.x, lo.y);
    if h.app.cad.run.as_ref().is_some_and(|r| r.m.prompt(&h.app).accept.select) {
        feed(&mut h.app, Input::Object(name.clone()));
        feed(&mut h.app, Input::Enter);
    }
    feed(&mut h.app, pt(corner.0, corner.1));
    feed(&mut h.app, Input::Number(90.0));
    let (lo2, hi2) = body_box(&h, &name);
    // Turned a quarter about the corner: it now spans x from corner−10 to corner.
    assert!((hi2.x - corner.0).abs() < 1e-6 && (lo2.x - (corner.0 - 10.0)).abs() < 1e-6, "{lo2:?} {hi2:?} about {corner:?}");
    assert!((lo2.y - corner.1).abs() < 1e-6, "{lo2:?}");
    // Mirror across the line x = 0, keeping the source: a second body appears.
    let before = crate::open_meshing::session_world(&h.app.session).bodies.len();
    tool(&mut h, "obj.mirror");
    if h.app.cad.run.as_ref().is_some_and(|r| r.m.prompt(&h.app).accept.select) {
        feed(&mut h.app, Input::Object(name.clone()));
        feed(&mut h.app, Input::Enter);
    }
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, pt(0.0, 10.0));
    feed(&mut h.app, Input::Enter);
    assert_eq!(crate::open_meshing::session_world(&h.app.session).bodies.len(), before + 1);
}

fn circle_arcs(h: &Harness) -> Vec<(Vec2, f64, f64, f64)> {
    ents(h)
        .into_iter()
        .filter_map(|e| match e.shape {
            Shape::Arc { c, r, a0, a1 } => Some((c, r, a0, a1)),
            _ => None,
        })
        .collect()
}

/// A circle crossed by two lines: Trim takes the piece clicked, left or right (#b07: it took the
/// same piece wherever the circle was picked).
#[test]
fn trim_takes_the_clicked_piece_of_a_circle() {
    for (side, keep_x) in [(10.0, -10.0), (-10.0, 10.0)] {
        let mut h = sketching();
        feed(&mut h.app, Input::Enter);
        tool(&mut h, "cad.circle");
        feed(&mut h.app, pt(0.0, 0.0));
        feed(&mut h.app, Input::Number(10.0));
        for x in [-5.0, 5.0] {
            tool(&mut h, "cad.line");
            feed(&mut h.app, pt(x, -20.0));
            feed(&mut h.app, pt(x, 20.0));
            feed(&mut h.app, Input::Enter);
        }
        let circle = ents(&h).into_iter().find(|e| matches!(e.shape, Shape::Circle(..))).unwrap().id;
        tool(&mut h, "cad.trim");
        feed(&mut h.app, pick(&circle, side, 0.0));
        feed(&mut h.app, Input::Enter);
        let arcs = circle_arcs(&h);
        assert_eq!(arcs.len(), 1, "{arcs:?}");
        let (c, r, a0, a1) = arcs[0];
        let pts = super::geo::arc_points(c, r, a0, a1);
        assert!(pts.iter().all(|p| p.x * side.signum() < 5.0 + 1e-6), "the clicked side is gone: {pts:?}");
        assert!(pts.iter().any(|p| (p.x - keep_x).abs() < 0.5), "the far side stays: {pts:?}");
    }
}

/// Trim with a fence: two points across several curves trim each where the fence crosses it.
#[test]
fn trim_fence_trims_every_crossed_curve() {
    let mut h = sketching();
    feed(&mut h.app, Input::Enter);
    // A long horizontal line and three verticals sticking out above it.
    tool(&mut h, "cad.line");
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, pt(30.0, 0.0));
    feed(&mut h.app, Input::Enter);
    for x in [5.0, 15.0, 25.0] {
        tool(&mut h, "cad.line");
        feed(&mut h.app, pt(x, -10.0));
        feed(&mut h.app, pt(x, 10.0));
        feed(&mut h.app, Input::Enter);
    }
    tool(&mut h, "cad.trim");
    // Empty space starts the fence; it crosses the three upper halves.
    feed(&mut h.app, pt(0.0, 5.0));
    feed(&mut h.app, pt(30.0, 5.0));
    feed(&mut h.app, Input::Enter);
    let l = lines(&h);
    assert!(!l.iter().any(|(a, b)| a.y > 1e-6 || b.y > 1e-6), "the parts above the base are gone: {l:?}");
    assert_eq!(l.iter().filter(|(a, b)| (a.x - b.x).abs() < 1e-9).count(), 3, "{l:?}");
}

/// A pick prompt (Trim) is not snapped: a click near a circle's quadrant stays where it is.
#[test]
fn a_pick_is_not_snapped() {
    assert!(super::Accept::PICK.pick && !super::Accept::POINT.pick);
}

/// Rectangle's other corner: the value boxes are its Length and Height, not a distance and angle.
#[test]
fn rectangle_value_boxes_are_length_and_height() {
    let mut h = sketching();
    feed(&mut h.app, Input::Enter);
    tool(&mut h, "cad.rectang");
    feed(&mut h.app, pt(1.0, 1.0));
    let prompt = h.app.cad.run.as_ref().unwrap().m.prompt(&h.app);
    let mut s = h.app.cad.settings.clone();
    s.dyn_input = true;
    s.dyn_polar = true;
    s.dyn_relative = false;
    let e = super::dyn_entry(&s, &prompt, "").unwrap();
    assert!(!e.polar && e.relative && e.sides, "{e:?}");
    assert_eq!(e.live(Vec2::new(21.0, 11.0)), (20.0, 10.0));
}

/// MODIFY's Move outside a sketch moves the picked bodies and reference planes alike, by the
/// points picked in the world.
#[test]
fn object_move_takes_reference_planes_too() {
    use serde_json::json;
    let mut h = Harness::new();
    h.app.session.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let pt3 = |x: f64, y: f64, z: f64| json!({"point": [x, y, z]});
    h.app
        .session
        .execute(
            "construct.plane.three_points",
            &json!({"a": pt3(0.0, 0.0, 0.0), "b": pt3(1.0, 0.0, 0.0), "c": pt3(0.0, 0.0, 1.0), "name": "Reference Plane 1"}),
        )
        .unwrap();
    let body = crate::open_meshing::session_world(&h.app.session).bodies.first().unwrap().name.clone();
    let min_y = |h: &Harness| {
        let st = crate::open_meshing::session_world(&h.app.session);
        let b = st.bodies.iter().find(|b| b.name == body).unwrap();
        let (m, place) = b.mesh_in_place();
        m.positions.iter().map(|p| solidcraft_engine::doc::apply_point(&place, *p).y).fold(f64::INFINITY, f64::min)
    };
    let before = min_y(&h);
    h.app.session.execute("select.set", &json!({"items": [{"type": "body", "name": body}, {"type": "plane", "name": "Reference Plane 1"}]})).unwrap();
    tool(&mut h, "obj.move");
    feed(&mut h.app, pt(0.0, 0.0));
    feed(&mut h.app, pt(0.0, 5.0));
    let planes = solidcraft_engine::view::construction_planes(&h.app.session);
    let (_, _, pl) = planes.iter().find(|(_, n, _)| n == "Reference Plane 1").unwrap();
    assert!((pl.origin.y - 5.0).abs() < 1e-6, "the plane moved 5 along Y: {pl:?}");
    assert!((min_y(&h) - before - 5.0).abs() < 1e-6, "the body moved too: {} from {before}", min_y(&h));
}

/// A value typed as a formula: `=` then + - * / and brackets.
#[test]
fn values_take_formulas() {
    use super::geo::number;
    assert_eq!(number("=10+2*3"), Some(16.0));
    assert_eq!(number("=(1+2)/3"), Some(1.0));
    assert_eq!(number("=-4*-2.5"), Some(10.0));
    assert_eq!(number("=100/4-5"), Some(20.0));
    assert_eq!(number("=5/0"), None);
    assert_eq!(number("=2+"), None);
    assert_eq!(number(&format!("={}", "(".repeat(100))), None);
}

/// Tab while a tool runs is the value boxes' (taken from the frame's input before egui moves the
/// keyboard focus to a panel's widgets).
#[test]
fn tab_belongs_to_the_value_boxes() {
    let mut h = sketching();
    feed(&mut h.app, Input::Enter);
    tool(&mut h, "cad.line");
    let mut raw = egui::RawInput::default();
    raw.events.push(egui::Event::Key { key: egui::Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
    super::take_tabs(&mut h.app, &mut raw);
    assert!(raw.events.is_empty());
    assert_eq!(h.app.cad.tabs, 1);
}
