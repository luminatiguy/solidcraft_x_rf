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
    assert!(h.app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::Extrude { .. })));
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
