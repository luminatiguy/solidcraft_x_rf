//! The MODEL tab's form tools: Extrusion, Blend, Revolve, Sweep, Swept Blend and their void
//! (cutting) versions, the work plane and Model Line.
//!
//! A form always starts from a sketch: choosing a tool opens a new sketch on the work plane, turns
//! the view to face it and puts the window into sketch mode, where the model and the other
//! sketches are shown faded and cannot be picked (only what is drawn in this sketch can). Finish
//! checks the sketch and either opens the next sketch the form needs (Blend's top, a Sweep's
//! profile across the path) or opens the feature's own window filled in with the sketches, to
//! set its sizes and press OK. Cancel drops the form and the sketches it made.

use serde_json::{Value, json};
use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::dialogs::Kind;

/// What a form tool builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Extrusion,
    Blend,
    Revolve,
    Sweep,
    SweptBlend,
    ModelLine,
}

impl Form {
    fn label(self, void: bool) -> &'static str {
        match (self, void) {
            (Form::Extrusion, false) => "Extrusion",
            (Form::Blend, false) => "Blend",
            (Form::Revolve, false) => "Revolve",
            (Form::Sweep, false) => "Sweep",
            (Form::SweptBlend, false) => "Swept Blend",
            (Form::Extrusion, true) => "Void Extrusion",
            (Form::Blend, true) => "Void Blend",
            (Form::Revolve, true) => "Void Revolve",
            (Form::Sweep, true) => "Void Sweep",
            (Form::SweptBlend, true) => "Void Swept Blend",
            (Form::ModelLine, _) => "Model Line",
        }
    }

    fn command(self, void: bool) -> &'static str {
        match (self, void) {
            (Form::Extrusion, false) => "form.extrusion",
            (Form::Blend, false) => "form.blend",
            (Form::Revolve, false) => "form.revolve",
            (Form::Sweep, false) => "form.sweep",
            (Form::SweptBlend, false) => "form.swept_blend",
            (Form::Extrusion, true) => "form.void.extrusion",
            (Form::Blend, true) => "form.void.blend",
            (Form::Revolve, true) => "form.void.revolve",
            (Form::Sweep, true) => "form.void.sweep",
            (Form::SweptBlend, true) => "form.void.swept_blend",
            (Form::ModelLine, _) => "model.line",
        }
    }
}

/// The form tool command `id` names, and whether it is a void.
pub fn parse(id: &str) -> Option<(Form, bool)> {
    Some(match id {
        "form.extrusion" => (Form::Extrusion, false),
        "form.blend" => (Form::Blend, false),
        "form.revolve" => (Form::Revolve, false),
        "form.sweep" => (Form::Sweep, false),
        "form.swept_blend" => (Form::SweptBlend, false),
        "form.void.extrusion" => (Form::Extrusion, true),
        "form.void.blend" => (Form::Blend, true),
        "form.void.revolve" => (Form::Revolve, true),
        "form.void.sweep" => (Form::Sweep, true),
        "form.void.swept_blend" => (Form::SweptBlend, true),
        "model.line" => (Form::ModelLine, false),
        _ => return None,
    })
}

/// The path of a Sweep or Swept Blend, drawn in its first sketch.
#[derive(Clone, Debug)]
struct PathInfo {
    sketch: u64,
    curves: Vec<String>,
    /// Profile planes across the path's start and end (square to it, facing along it).
    start: Plane,
    end: Plane,
}

#[derive(Clone, Debug)]
struct FormSession {
    form: Form,
    void: bool,
    /// The sketches made so far, in order; the last is the one open.
    sketches: Vec<u64>,
    path: Option<PathInfo>,
}

/// A feature window to fill once it is open.
#[derive(Clone, Debug)]
struct Fill {
    command: &'static str,
    inputs: Vec<(usize, Vec<Sel>)>,
    void: bool,
    /// Frames left to wait for the window.
    frames: u8,
}

#[derive(Debug, Default)]
pub struct State {
    /// The plane form tools and Model Line sketch on: an origin or construction plane, or a
    /// planar face. `None`: the XY plane.
    pub work_plane: Option<Sel>,
    /// The work plane's grid is shown (Show Work Plane).
    pub show_work_plane: bool,
    /// Set Work Plane waits for a plane or face to be picked.
    pub picking_work_plane: bool,
    session: Option<FormSession>,
    /// Finishing or cancelling a form's sketch ourselves: the Finish hook lets it through.
    leaving: bool,
    fill: Option<Fill>,
    /// Clear the selection next frame (what the click that set the work plane selected).
    clear_selection: bool,
}

const XY: &str = "XY";

fn status(app: &mut SolveApp, text: impl Into<String>, error: bool) {
    app.set_status(text.into(), error);
}

/// `start`'s hook: the form tools, the work plane commands and Model Line. True when `id` was one.
pub fn start_hook(app: &mut SolveApp, id: &str) -> bool {
    if id == "workplane.set" {
        set_work_plane(app);
        return true;
    }
    if id == "workplane.show" {
        app.forms.show_work_plane = !app.forms.show_work_plane;
        let text = if app.forms.show_work_plane {
            format!("Work plane shown: {}", work_plane_name(app))
        } else {
            "Work plane hidden".to_string()
        };
        status(app, text, false);
        return true;
    }
    // A sketch starts from a form tool (or Model Line). Surface, Sheet Metal and Weldments build
    // from sketches of their own and keep their New Sketch.
    if id == "sketch.create" && !matches!(app.ui.tab.as_str(), "SURFACE" | "SHEET METAL" | "WELDMENTS") {
        status(app, "A sketch starts from a modelling tool: choose Extrusion, Blend, Revolve, Sweep, Swept Blend or Model Line on the MODEL tab", true);
        return true;
    }
    // A form's own sketch is finished by Finish: a feature command started from it (its key)
    // would take the sketch past the form.
    if app.forms.session.is_some()
        && app.session.active_sketch.is_some()
        && matches!(id.split('.').next(), Some("solid" | "surface" | "sheet" | "plastic" | "frame"))
    {
        status(app, "Finish the form's sketch first (Finish on the SKETCH tab)", true);
        return true;
    }
    let Some((form, void)) = parse(id) else { return false };
    start_form(app, form, void);
    true
}

/// Is a form tool's session running for `id`, or Show Work Plane on (its button shows pressed)?
pub fn pressed(app: &SolveApp, id: &str) -> bool {
    if id == "workplane.show" {
        return app.forms.show_work_plane;
    }
    if id == "workplane.set" {
        return app.forms.picking_work_plane;
    }
    app.forms.session.as_ref().is_some_and(|s| s.form.command(s.void) == id)
}

// ---- Work plane ---------------------------------------------------------------------------

fn work_plane_name(app: &SolveApp) -> String {
    match &app.forms.work_plane {
        None => XY.to_string(),
        Some(Sel::Plane { name }) => name.clone(),
        Some(Sel::Face { body, .. }) => format!("face of {body}"),
        Some(_) => "?".into(),
    }
}

/// A selection that can be the work plane: an origin or construction plane, or a planar face.
fn plane_pick(app: &SolveApp, sel: &Sel) -> bool {
    match sel {
        Sel::Plane { .. } => true,
        Sel::Face { body, index, .. } => crate::dialogs::planar_face(&app.session, body, *index).is_some(),
        _ => false,
    }
}

fn set_work_plane(app: &mut SolveApp) {
    if app.session.active_sketch.is_some() {
        status(app, "Set Work Plane: finish the sketch first", true);
        return;
    }
    crate::dialogs::cancel(app);
    app.dialog = None;
    app.tool = None;
    if let [sel] = app.session.selection.as_slice()
        && plane_pick(app, sel)
    {
        let sel = sel.clone();
        adopt_work_plane(app, sel);
        return;
    }
    let _ = app.session.execute("select.clear", &json!({}));
    app.forms.picking_work_plane = true;
    status(app, "Set Work Plane: click an origin plane, a construction plane or any flat face of the model (Esc: keep the current one)", false);
}

fn adopt_work_plane(app: &mut SolveApp, sel: Sel) {
    app.forms.work_plane = Some(sel);
    app.forms.picking_work_plane = false;
    let _ = app.session.execute("select.clear", &json!({}));
    let text = format!("Work plane: {}. Extrusion, Blend, Revolve, Sweep and Model Line now sketch on it", work_plane_name(app));
    status(app, text, false);
}

/// The work plane in the world, where Show Work Plane draws its grid (none while a sketch is
/// open: the sketch has its own).
pub fn shown_work_plane(app: &SolveApp) -> Option<Plane> {
    if !app.forms.show_work_plane || app.session.active_sketch.is_some() {
        return None;
    }
    work_plane_world(app)
}

fn work_plane_world(app: &SolveApp) -> Option<Plane> {
    match app.forms.work_plane.as_ref() {
        None => crate::viewport::named_plane(app, XY),
        Some(Sel::Plane { name }) => crate::viewport::named_plane(app, name),
        Some(Sel::Face { body, index, .. }) => {
            let (centre, normal) = crate::dialogs::planar_face(&app.session, body, *index)?;
            Plane::from_normal(centre, normal)
        }
        Some(_) => None,
    }
}

// ---- Sketch mode isolation ----------------------------------------------------------------

/// Tools that reach out of the sketch for model geometry keep it pickable.
const REFERENCE_TOOLS: &[&str] = &[
    "sketch.project",
    "sketch.intersect",
    "sketch.include_3d",
    "sketch.project_to_surface",
    "sketch.intersection_curve",
    "sketch.spun_profile",
    "sketch.isoparametric_curve",
    "sketch.fit_curves_to_section",
];

/// While a sketch is open, everything but what is drawn in it is inactive: the model's faces,
/// edges and vertices, the other sketches, planes and axes cannot be picked or snapped to.
/// Projecting (and windows that pick model geometry) still reach the model.
pub fn isolating(app: &SolveApp) -> bool {
    if app.session.active_sketch.is_none() {
        return false;
    }
    if app.tool.as_ref().is_some_and(|t| REFERENCE_TOOLS.contains(&t.cmd)) {
        return false;
    }
    if let Some(d) = &app.dialog {
        if matches!(d.kind, Kind::SketchProject { .. } | Kind::SketchIntersect { .. }) {
            return false;
        }
        let model = crate::selection::FACES | crate::selection::EDGES | crate::selection::VERTICES | crate::selection::BODIES;
        if d.active_input().is_some_and(|i| i.accept & model != 0) {
            return false;
        }
    }
    true
}

/// Is hit `h` part of the open sketch (what stays pickable while [`isolating`])?
pub fn own_hit(app: &SolveApp, h: &crate::viewport::Hit) -> bool {
    use crate::viewport::Hit;
    let active = app.session.active_sketch;
    match h {
        Hit::SketchPoint { sketch, .. } | Hit::SketchMidpoint { sketch, .. } | Hit::SketchCurve { sketch, .. } | Hit::Profile { sketch, .. } => {
            Some(*sketch) == active
        }
        _ => false,
    }
}

/// The model and the other sketches are drawn faded while a sketch is open (halftone).
pub fn halftone(app: &SolveApp) -> bool {
    app.session.active_sketch.is_some()
}

// ---- Starting a form ----------------------------------------------------------------------

fn start_form(app: &mut SolveApp, form: Form, void: bool) {
    app.forms.picking_work_plane = false;
    app.forms.fill = None;
    if app.session.active_sketch.is_some() {
        if app.forms.session.is_some() {
            abort(app, true);
        } else {
            app.finish_sketch();
        }
        if app.session.active_sketch.is_some() {
            status(app, format!("{}: finish or cancel the open sketch first", form.label(void)), true);
            return;
        }
    }
    crate::dialogs::cancel(app);
    app.dialog = None;
    app.tool = None;
    let _ = app.session.execute("select.clear", &json!({}));
    app.forms.session = Some(FormSession { form, void, sketches: Vec::new(), path: None });
    let plane = app.forms.work_plane.clone().unwrap_or_else(|| Sel::Plane { name: XY.into() });
    let opened = open_on(app, &plane).or_else(|why| {
        if app.forms.work_plane.is_some() {
            // The face the work plane was on is gone (the model changed): back to XY.
            app.forms.work_plane = None;
            let text = format!("The work plane is no longer there ({why}); sketching on XY");
            status(app, text, true);
            crate::dialogs::start_sketch(app, &Sel::Plane { name: XY.into() })
        } else {
            Err(why)
        }
    });
    if let Err(why) = opened {
        app.forms.session = None;
        status(app, format!("{}: {why}", form.label(void)), true);
        return;
    }
    opened_sketch(app, true);
    let hint = match form {
        Form::Extrusion => "draw one or more closed loops (a loop inside another makes a hole), then Finish",
        Form::Blend => "draw the base loop, then Finish; the top loop comes next",
        Form::Revolve => "draw closed loops and the axis as a Centerline, then Finish",
        Form::Sweep => "draw the path (lines and arcs end to end), then Finish; the profile comes next",
        Form::SweptBlend => "draw the path (lines and arcs end to end), then Finish; a profile at each end comes next",
        Form::ModelLine => "draw lines on the work plane, then Finish",
    };
    let text = format!("{}: {hint}", form.label(void));
    status(app, text, false);
}

/// Open the form's first sketch on the work plane `sel`. On a face, the face's edges are not
/// projected into it: only what is drawn is the user's.
fn open_on(app: &mut SolveApp, sel: &Sel) -> Result<(), String> {
    let Sel::Face { point, .. } = sel else { return crate::dialogs::start_sketch(app, sel) };
    let plane = crate::sketch_attachment_ui::face_plane(app, sel).ok_or("the face no longer exists")?;
    let params = json!({"plane": plane, "activate_part": app.preferences.sketch_face_activates_part, "project_edges": false});
    let before = app.settled_cam();
    app.run("sketch.create", params).map_err(|e| crate::friendly_error("sketch.create", &e))?;
    if app.look_at_opened_sketch(before)
        && let Some(mut to) = app.cam_anim.as_ref().map(|a| a.to)
    {
        // Look at the picked point, not the face plane's origin (a corner of the face).
        to.target = *point;
        app.animate_to(to);
    }
    Ok(())
}

/// A sketch of the form opened: it faces the view (whatever Preferences say about opening
/// sketches), joins the session, and the Line tool starts.
fn opened_sketch(app: &mut SolveApp, look: bool) {
    if look && !app.preferences.sketch_open_look_at {
        crate::dialogs::look_at_sketch(app);
    }
    if let (Some(id), Some(s)) = (app.session.active_sketch, app.forms.session.as_mut()) {
        s.sketches.push(id);
    }
    app.ui.tab = "SKETCH".into();
    app.start("sketch.line");
}

fn plane_json(p: &Plane) -> Value {
    let v = |a: Vec3| json!([a.x, a.y, a.z]);
    json!({"origin": v(p.origin), "x_dir": v(p.x), "y_dir": v(p.y)})
}

/// Open the form's next sketch on `params` (`sketch.create`'s), the view centred on `focus` (the
/// base loop over a Blend's top, the path's end for a profile), so what came before shows.
fn open_next(app: &mut SolveApp, params: Value, focus: Option<Vec3>) -> Result<(), String> {
    let before = app.settled_cam();
    app.run("sketch.create", params).map_err(|e| crate::friendly_error("sketch.create", &e))?;
    let turned = app.look_at_opened_sketch(before) || {
        crate::dialogs::look_at_sketch(app);
        true
    };
    if turned
        && let Some(at) = focus
        && let Some(mut to) = app.cam_anim.as_ref().map(|a| a.to)
    {
        to.target = at;
        app.animate_to(to);
    }
    opened_sketch(app, false);
    Ok(())
}

/// The middle of what is drawn in sketch `sketch`, in the world.
fn drawn_centre(app: &SolveApp, sketch: u64) -> Option<Vec3> {
    let st = crate::open_meshing::session_world(&app.session);
    let ss = st.sketch(sketch)?;
    let (mut sum, mut n) = (Vec2::ZERO, 0.0);
    for i in drawn_curves(ss) {
        for p in ss.sketch.polyline(i) {
            sum = sum + p;
            n += 1.0;
        }
    }
    (n > 0.0).then(|| ss.plane.to_world(sum * (1.0 / n)))
}

// ---- Finishing --------------------------------------------------------------------------

/// `finish_sketch`'s hook. True when the form took it: the sketch was checked and either the
/// form's next sketch or its feature window opened, or (a refusal) the sketch stays open.
pub fn intercept_finish(app: &mut SolveApp) -> bool {
    if app.forms.leaving {
        return false;
    }
    let Some(session) = app.forms.session.clone() else { return false };
    let Some(active) = app.session.active_sketch else { return false };
    if session.sketches.last() != Some(&active) {
        // Another sketch (opened some other way): not the form's.
        app.forms.session = None;
        return false;
    }
    let label = session.form.label(session.void);
    if let Err(why) = step(app, session, active) {
        status(app, format!("{label}: {why}"), true);
    }
    true
}

/// `cancel_sketch`'s hook: cancelling a form's sketch drops the whole form, with the sketches it
/// made before this one.
pub fn intercept_cancel(app: &mut SolveApp) -> bool {
    if app.forms.leaving || app.forms.session.is_none() {
        return false;
    }
    abort(app, true);
    true
}

fn leave(app: &mut SolveApp, command: &str) {
    app.forms.leaving = true;
    app.leave_sketch(command);
    app.forms.leaving = false;
}

/// Drop the form: cancel its open sketch and, with `delete`, remove the sketches it made before.
fn abort(app: &mut SolveApp, delete: bool) {
    let Some(session) = app.forms.session.take() else { return };
    let open = app.session.active_sketch.filter(|id| session.sketches.last() == Some(id));
    if open.is_some() {
        leave(app, "sketch.cancel");
    }
    let earlier: Vec<u64> = session.sketches.iter().copied().filter(|id| Some(*id) != open && app.session.doc.feature(*id).is_some()).collect();
    if delete && !earlier.is_empty() {
        let _ = app.run("timeline.delete", json!({"features": earlier}));
    }
    status(app, format!("{} cancelled", session.form.label(session.void)), false);
}

/// The open sketch's closed regions a form takes: those inside an even number of others, so a
/// loop drawn inside another is a hole (as the form tools of architectural modellers treat it).
fn solid_profiles(app: &SolveApp, sketch: u64) -> Vec<usize> {
    let st = crate::open_meshing::session_world(&app.session);
    let Some(ss) = st.sketch(sketch) else { return Vec::new() };
    let key = |ids: &[String]| {
        let mut v = ids.to_vec();
        v.sort();
        v
    };
    let outers: Vec<Vec<String>> = ss.profiles.iter().map(|p| key(&p.outer_curves)).collect();
    // The profile whose hole loop is `i`'s outer loop: `i` sits directly inside it.
    let parent = |i: usize| -> Option<usize> {
        let outer = outers.get(i)?;
        ss.profiles.iter().enumerate().find(|(j, p)| *j != i && p.hole_curves.iter().any(|h| key(h) == *outer)).map(|(j, _)| j)
    };
    (0..ss.profiles.len())
        .filter(|&i| {
            let mut depth = 0;
            let mut at = i;
            while let Some(up) = parent(at) {
                depth += 1;
                at = up;
                if depth > ss.profiles.len() {
                    break;
                }
            }
            depth % 2 == 0
        })
        .collect()
}

fn profile_sels(sketch: u64, profiles: &[usize]) -> Vec<Sel> {
    profiles.iter().map(|&index| Sel::Profile { sketch, index }).collect()
}

/// Curves drawn in the sketch (not construction, centerlines, infinite or projected lines).
fn drawn_curves(ss: &solidcraft_engine::doc::SolvedSketch) -> Vec<usize> {
    ss.sketch
        .curves
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.construction && !c.centerline && !c.infinite && c.link.is_none())
        .map(|(i, _)| i)
        .collect()
}

/// The size of what is drawn in the sketch, its larger side (mm).
fn drawn_size(app: &SolveApp, sketch: u64) -> f64 {
    let st = crate::open_meshing::session_world(&app.session);
    let Some(ss) = st.sketch(sketch) else { return 0.0 };
    let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for i in drawn_curves(ss) {
        for p in ss.sketch.polyline(i) {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
    }
    if lo.x > hi.x { 0.0 } else { (hi.x - lo.x).max(hi.y - lo.y) }
}

/// The path drawn in sketch `sketch`: its curves in order, end to end, and the profile planes
/// across its two ends.
fn path_of(app: &SolveApp, sketch: u64) -> Result<PathInfo, String> {
    let st = crate::open_meshing::session_world(&app.session);
    let ss = st.sketch(sketch).ok_or("the sketch is gone")?;
    let mut pieces: Vec<(String, Vec<Vec2>)> = drawn_curves(ss)
        .into_iter()
        .filter_map(|i| {
            let pts = ss.sketch.polyline(i);
            (pts.len() >= 2).then(|| (ss.sketch.curves.get(i).map(|c| c.id.clone()).unwrap_or_default(), pts))
        })
        .collect();
    if pieces.is_empty() {
        return Err("draw the path first (lines, arcs or splines end to end)".into());
    }
    let size = drawn_size(app, sketch).max(1.0);
    let tol = (size * 1e-6).max(1e-6);
    let ends = |p: &Vec<Vec2>| (*p.first().unwrap_or(&Vec2::ZERO), *p.last().unwrap_or(&Vec2::ZERO));
    // Start at a free end (an open path); a closed path starts anywhere.
    let free = |k: usize, at: Vec2, pieces: &[(String, Vec<Vec2>)]| {
        !pieces.iter().enumerate().any(|(j, (_, q))| {
            let (a, b) = ends(q);
            j != k && (a.dist(at) < tol || b.dist(at) < tol)
        })
    };
    let first = (0..pieces.len())
        .find_map(|k| {
            let (a, b) = ends(&pieces[k].1);
            if free(k, a, &pieces) {
                Some((k, false))
            } else if free(k, b, &pieces) {
                Some((k, true))
            } else {
                None
            }
        })
        .unwrap_or((0, false));
    let (k, reverse) = first;
    let (id, mut pts) = pieces.remove(k);
    if reverse {
        pts.reverse();
    }
    let mut curves = vec![id];
    let mut chain = pts;
    while !pieces.is_empty() {
        let tip = *chain.last().unwrap_or(&Vec2::ZERO);
        let next = pieces.iter().position(|(_, q)| {
            let (a, b) = ends(q);
            a.dist(tip) < tol || b.dist(tip) < tol
        });
        let Some(n) = next else {
            return Err("the path must be one chain of curves meeting end to end".into());
        };
        let (id, mut q) = pieces.remove(n);
        if ends(&q).0.dist(tip) >= tol {
            q.reverse();
        }
        chain.extend(q.into_iter().skip(1));
        curves.push(id);
    }
    let plane = ss.plane;
    let across = |at: Vec2, along: Vec2| -> Option<Plane> {
        // Sketch x along the path sketch's normal, y across in its plane: a circle's seam (at
        // its x axis) then stays off the plane the path bends in, where the kernel refuses it.
        let n = plane.dir_to_world(along).normalized()?;
        let x = plane.normal();
        let y = n.cross(x);
        Plane::new(plane.to_world(at), x, y)
    };
    let (s0, s1) = (*chain.first().ok_or("empty path")?, *chain.get(1).ok_or("the path is too short")?);
    let (e1, e0) = (*chain.last().ok_or("empty path")?, *chain.get(chain.len().saturating_sub(2)).ok_or("the path is too short")?);
    let start = across(s0, s1 - s0).ok_or("the path's start has no direction")?;
    let end = across(e1, e1 - e0).ok_or("the path's end has no direction")?;
    Ok(PathInfo { sketch, curves, start, end })
}

/// The form's sketch `active` is finished: check it, then the next sketch or the feature.
fn step(app: &mut SolveApp, mut session: FormSession, active: u64) -> Result<(), String> {
    let stage = session.sketches.len();
    let closed = |app: &SolveApp| {
        let p = solid_profiles(app, active);
        if p.is_empty() { Err("the sketch needs a closed loop (draw one, or close the open one), then Finish".to_string()) } else { Ok(p) }
    };
    match (session.form, stage) {
        (Form::ModelLine, _) => {
            app.forms.session = None;
            leave(app, "sketch.finish");
            status(app, "Model Line: done", false);
        }
        (Form::Extrusion, _) => {
            let profiles = closed(app)?;
            app.forms.session = None;
            leave(app, "sketch.finish");
            open_feature(app, "solid.extrude", vec![(0, profile_sels(active, &profiles))], session.void);
        }
        (Form::Revolve, _) => {
            let profiles = closed(app)?;
            let axis = revolve_axis(app, active).ok_or("draw the axis line: SKETCH ▸ Centerline (or a construction line), then Finish")?;
            app.forms.session = None;
            leave(app, "sketch.finish");
            let axis = crate::selection::curve_pick(&axis, active, app.session.active_sketch);
            open_feature(app, "solid.revolve", vec![(0, profile_sels(active, &profiles)), (1, vec![axis])], session.void);
        }
        (Form::Blend, 1) => {
            closed(app)?;
            let depth = (drawn_size(app, active) / 5.0).round().max(2.0) * 5.0;
            let st = crate::open_meshing::session_world(&app.session);
            let base = st.sketch(active).map(|ss| ss.plane).ok_or("the sketch is gone")?;
            drop(st);
            leave(app, "sketch.finish");
            app.forms.session = Some(session);
            // The top: on the plane parallel to the base, `depth` along its normal.
            let top = base.offset(depth);
            let focus = drawn_centre(app, active).map(|c| c + base.normal() * depth);
            open_next(app, json!({"plane": plane_json(&top), "name": "Blend Top"}), focus).inspect_err(|_| abort(app, true))?;
            status(app, format!("Blend: draw the top loop ({depth} mm above the base), then Finish"), false);
        }
        (Form::Blend, _) => {
            let top = closed(app)?;
            let base = session.sketches.first().copied().ok_or("the base sketch is gone")?;
            let bottom = solid_profiles(app, base);
            app.forms.session = None;
            leave(app, "sketch.finish");
            let mut sections = profile_sels(base, &bottom);
            sections.extend(profile_sels(active, &top));
            open_feature(app, "solid.loft", vec![(0, sections)], session.void);
        }
        (Form::Sweep | Form::SweptBlend, 1) => {
            let path = path_of(app, active)?;
            let start = path.start;
            session.path = Some(path);
            leave(app, "sketch.finish");
            app.forms.session = Some(session.clone());
            open_next(app, json!({"plane": plane_json(&start), "name": "Profile"}), Some(start.origin)).inspect_err(|_| abort(app, true))?;
            let what = if session.form == Form::Sweep { "the profile" } else { "the first profile" };
            status(app, format!("{}: draw {what} (a closed loop across the path's start), then Finish", session.form.label(session.void)), false);
        }
        (Form::Sweep, _) => {
            let profiles = closed(app)?;
            let path = session.path.clone().ok_or("the path is gone")?;
            app.forms.session = None;
            leave(app, "sketch.finish");
            let curves = path.curves.iter().map(|id| crate::selection::curve_pick(id, path.sketch, app.session.active_sketch)).collect();
            open_feature(app, "solid.sweep", vec![(0, profile_sels(active, &profiles)), (1, curves)], session.void);
        }
        (Form::SweptBlend, 2) => {
            closed(app)?;
            let end = session.path.as_ref().map(|p| p.end).ok_or("the path is gone")?;
            leave(app, "sketch.finish");
            app.forms.session = Some(session.clone());
            open_next(app, json!({"plane": plane_json(&end), "name": "Profile 2"}), Some(end.origin)).inspect_err(|_| abort(app, true))?;
            status(app, format!("{}: draw the second profile (across the path's end), then Finish", session.form.label(session.void)), false);
        }
        (Form::SweptBlend, _) => {
            let second = closed(app)?;
            let first_sketch = *session.sketches.get(1).ok_or("the first profile is gone")?;
            let first = solid_profiles(app, first_sketch);
            app.forms.session = None;
            leave(app, "sketch.finish");
            let mut sections = profile_sels(first_sketch, &first);
            sections.extend(profile_sels(active, &second));
            open_feature(app, "solid.loft", vec![(0, sections)], session.void);
        }
    }
    Ok(())
}

/// The line a Revolve turns about: the sketch's centerline, else a straight construction line.
fn revolve_axis(app: &SolveApp, sketch: u64) -> Option<String> {
    let st = crate::open_meshing::session_world(&app.session);
    let ss = st.sketch(sketch)?;
    let straight = |i: usize| matches!(ss.sketch.shape(i), Some(solidcraft_engine::sketch::Shape::Line { .. }));
    let pick = |want: &dyn Fn(&solidcraft_engine::sketch::Curve) -> bool| {
        ss.sketch.curves.iter().enumerate().find(|(i, c)| want(c) && straight(*i) && c.link.is_none()).map(|(_, c)| c.id.clone())
    };
    pick(&|c| c.centerline).or_else(|| pick(&|c| c.construction && !c.infinite)).or_else(|| pick(&|c| c.infinite))
}

/// Open feature `command`'s window and fill its inputs; a void cuts.
fn open_feature(app: &mut SolveApp, command: &'static str, inputs: Vec<(usize, Vec<Sel>)>, void: bool) {
    app.forms.fill = Some(Fill { command, inputs, void, frames: 30 });
    if app.session.active_sketch.is_none() && app.deferred.is_none() {
        app.start(command);
        apply_fill(app);
    }
}

fn apply_fill(app: &mut SolveApp) -> bool {
    let Some(fill) = app.forms.fill.clone() else { return true };
    let Some(d) = app.dialog.as_mut() else { return false };
    let kind_ok = match fill.command {
        "solid.extrude" => matches!(d.kind, Kind::Extrude { .. }),
        "solid.revolve" => matches!(d.kind, Kind::Revolve { .. }),
        "solid.loft" => matches!(d.kind, Kind::Loft { .. }),
        "solid.sweep" => matches!(d.kind, Kind::Sweep { .. }),
        _ => false,
    };
    if !kind_ok {
        return false;
    }
    for (index, items) in &fill.inputs {
        if let Some(input) = d.inputs.get_mut(*index) {
            input.items = items.clone();
        }
    }
    d.active = d.inputs.iter().position(|i| i.items.is_empty()).unwrap_or(0);
    if fill.void {
        match &mut d.kind {
            Kind::Extrude { operation, auto_op, .. } => {
                *operation = 2;
                *auto_op = false;
            }
            Kind::Revolve { operation, .. } | Kind::Loft { operation, .. } | Kind::Sweep { operation, .. } => *operation = 2,
            _ => {}
        }
    }
    app.forms.fill = None;
    // The feature's window opened from the MODEL tab: the ribbon stays there.
    app.ui.tab = "MODEL".into();
    let text = if fill.void { "set the sizes, then OK cuts the void" } else { "set the sizes, then OK makes the form" };
    status(app, text, false);
    true
}

// ---- Every frame --------------------------------------------------------------------------

/// Once a frame: Set Work Plane's pick, the feature window waiting to be filled, and a form
/// whose sketch was closed some other way (undo, another document).
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if std::mem::take(&mut app.forms.clear_selection) {
        let _ = app.session.execute("select.clear", &json!({}));
    }
    if app.forms.picking_work_plane {
        let click = ctx.input(|i| if i.pointer.primary_clicked() { i.pointer.interact_pos() } else { None });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) || app.session.active_sketch.is_some() || app.dialog.is_some() {
            app.forms.picking_work_plane = false;
            status(app, "Set Work Plane cancelled", false);
        } else if let Some(pos) = click
            && let Some(rect) = app.viewport.rect.filter(|r| r.contains(pos))
        {
            // What is under the click, whatever the selection filter would make of it (a click
            // on a face selects its body by default).
            let proj = crate::viewport::projection(app, rect);
            let hits = crate::viewport::pick(app, &proj, pos);
            let found = hits.iter().find_map(|h| match h {
                crate::viewport::Hit::Plane { name, .. } => Some(Sel::Plane { name: name.clone() }),
                crate::viewport::Hit::Face { body, index, point } => {
                    Some(Sel::Face { body: body.clone(), index: *index, point: *point }).filter(|s| plane_pick(app, s))
                }
                _ => None,
            });
            match found {
                Some(sel) => {
                    adopt_work_plane(app, sel);
                    // The viewport selects what was clicked later this frame.
                    app.forms.clear_selection = true;
                }
                None if hits.iter().any(|h| matches!(h, crate::viewport::Hit::Face { .. })) => {
                    status(app, "Set Work Plane: that face is not flat; pick a plane or a flat face", true);
                }
                None => {}
            }
        } else if let Some(sel) = app.session.selection.iter().find(|s| plane_pick(app, s)).cloned() {
            // A plane picked in the browser.
            adopt_work_plane(app, sel);
        }
    }
    if let Some(mut fill) = app.forms.fill.clone() {
        if app.session.active_sketch.is_none() && app.deferred.is_none() && app.dialog.is_none() {
            app.start(fill.command);
        }
        if !apply_fill(app) {
            fill.frames = fill.frames.saturating_sub(1);
            app.forms.fill = (fill.frames > 0).then_some(fill);
        }
        ctx.request_repaint();
    }
    if !app.forms.leaving
        && let Some(session) = &app.forms.session
        && session.sketches.last().is_some_and(|id| app.session.active_sketch != Some(*id))
    {
        let label = session.form.label(session.void);
        app.forms.session = None;
        status(app, format!("{label}: its sketch was closed; the form was left unfinished"), false);
    }
}
