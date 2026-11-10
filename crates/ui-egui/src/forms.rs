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
    /// A reference plane: one line drawn in a sketch on the work plane.
    Reference,
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
            (Form::Reference, _) => "Reference Plane",
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
            (Form::Reference, _) => "workplane.reference",
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
        "workplane.reference" => (Form::Reference, false),
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
    /// The sketches made so far, in order; the last is the one being drawn.
    sketches: Vec<u64>,
    path: Option<PathInfo>,
    /// The object's name (`Extrusion 1`): its body's, its feature's, and its sketches' stem.
    name: String,
    /// The form's values, set in Properties while it is drawn (see [`Pending`]).
    pending: Pending,
}

/// What a form makes, set in Properties before it is made: an Extrusion's Start and End, a
/// Revolve's angle, a Blend's distance between its sketches.
#[derive(Clone, Debug)]
pub struct Pending {
    pub start: String,
    pub end: String,
    pub angle: String,
    /// The Blend's top sketch this far from the base (empty: sized from the base loop).
    pub distance: String,
}

impl Default for Pending {
    fn default() -> Self {
        Pending { start: "0 mm".into(), end: "10 mm".into(), angle: "360 deg".into(), distance: String::new() }
    }
}

/// A feature window to fill once it is open.
#[derive(Clone, Debug)]
struct Fill {
    command: &'static str,
    inputs: Vec<(usize, Vec<Sel>)>,
    void: bool,
    name: String,
    pending: Pending,
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
    /// Show Reference Planes: those not seen edge-on show as outlines.
    pub show_references: bool,
    /// Set Work Plane waits for a plane or face to be picked.
    pub picking_work_plane: bool,
    /// Orient to a plane (the view cube's menu) waits for a plane or flat face.
    pub picking_view_plane: bool,
    session: Option<FormSession>,
    /// Finishing or cancelling a form's sketch ourselves: the Finish hook lets it through.
    leaving: bool,
    fill: Option<Fill>,
    /// Where Finish found the contour open (sketch coordinates), marked until it is closed.
    open_ends: Vec<Vec2>,
    /// Clear the selection next frame (what the click that set the work plane selected).
    clear_selection: bool,
    /// An object's sketch opened by a double click: (sketch, the feature made from it, the
    /// sketch was seen open). Finishing it opens the feature's window (the depth, the angle).
    pub after_edit: Option<(u64, u64, bool)>,
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
    if let Some(reason) = blocked_in_form(app, id) {
        status(app, reason, true);
        return true;
    }
    if id == "workplane.show_references" {
        app.forms.show_references = !app.forms.show_references;
        let n = solidcraft_engine::view::construction_planes(&app.session)
            .into_iter()
            .filter(|(_, n, _)| crate::reference_planes::is_reference(n))
            .count();
        let text =
            if app.forms.show_references { format!("Reference planes shown ({n})") } else { "Reference planes shown only edge-on".to_string() };
        status(app, text, false);
        return true;
    }
    if id == "workplane.show" {
        app.forms.show_work_plane = !app.forms.show_work_plane;
        let text = if app.forms.show_work_plane { format!("Work plane shown: {}", work_plane_name(app)) } else { "Work plane hidden".to_string() };
        status(app, text, false);
        return true;
    }
    // A sketch starts from a form tool (or Model Line). Surface, Sheet Metal and Weldments build
    // from sketches of their own and keep their New Sketch.
    if id == "sketch.create" && !matches!(app.ui.tab.as_str(), "SURFACE" | "SHEET METAL" | "WELDMENTS") {
        status(
            app,
            "A sketch starts from a modelling tool: choose Extrusion, Blend, Revolve, Sweep, Swept Blend or Model Line on the MODEL tab",
            true,
        );
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
    if id == "workplane.show_references" {
        return app.forms.show_references;
    }
    if id == "workplane.set" {
        return app.forms.picking_work_plane;
    }
    if id == "workplane.reference" {
        return app.forms.session.as_ref().is_some_and(|s| s.form == Form::Reference);
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
    // The MODIFY tools pick their points on the work plane: its grid shows while one runs.
    if app.cad.objects && app.session.active_sketch.is_none() && app.cad.plane.is_some() {
        return app.cad.plane;
    }
    if !app.forms.show_work_plane || app.session.active_sketch.is_some() {
        return None;
    }
    work_plane_world(app)
}

pub(crate) fn work_plane_world(app: &SolveApp) -> Option<Plane> {
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

/// The view cube's Orient to a plane: the next plane or flat face clicked faces the view.
pub fn start_orient_plane(app: &mut SolveApp) {
    app.forms.picking_view_plane = true;
    app.forms.picking_work_plane = false;
    status(app, "Orient to a plane: click a plane or a flat face of the model (Esc: cancel)", false);
}

fn orient_to(app: &mut SolveApp, sel: Sel) {
    app.forms.picking_view_plane = false;
    let _ = app.session.execute("select.set", &json!({"items": [sel]}));
    app.start("view.normal_to");
    app.forms.clear_selection = true;
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
    let name = next_object_name(app, form.label(void));
    if form != Form::Reference {
        app.shell_ui.properties = true;
    }
    app.forms.session = Some(FormSession { form, void, sketches: Vec::new(), path: None, name, pending: Pending::default() });
    if form == Form::Reference {
        // A reference plane is drawn on a plane square to the view, through its centre: the
        // camera stays where it is, the grid faces the screen, and the plane made from the
        // line is seen edge-on from here.
        let cam = app.settled_cam();
        let (right, up, _) = cam.basis();
        let view = Plane::new(cam.target, right, up);
        let opened = match view {
            Some(p) => app.run("sketch.create", json!({"plane": plane_json(&p), "name": "Reference Plane sketch", "project_edges": false})),
            None => Err("the view has no direction".to_string()),
        };
        if let Err(why) = opened {
            app.forms.session = None;
            status(app, format!("Reference Plane: {why}"), true);
            return;
        }
        // Whatever opening a sketch would do to the camera, it stays.
        app.cam_anim = None;
        app.cam = cam;
        opened_sketch(app, false);
        status(app, "Reference Plane: draw one line (snaps work): the plane goes through it, square to the view", false);
        return;
    }
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
        Form::Reference => "draw one line (snaps work): the reference plane goes through it, square to the sketch",
    };
    let text = format!("{}: {hint}", form.label(void));
    status(app, text, false);
}

/// The work plane as `sketch.create` takes it, and its own normal (before any flip).
fn work_plane_ref(app: &SolveApp) -> Option<(Value, Vec3)> {
    match app.forms.work_plane.clone().unwrap_or(Sel::Plane { name: XY.into() }) {
        Sel::Plane { name } => {
            let plane = crate::viewport::named_plane(app, &name)?;
            Some((json!(name), plane.normal()))
        }
        // A face is taken where it is now, as a plane of its own: the form never depends on the
        // body it was drawn on (editing or deleting that body leaves the form as it is).
        Sel::Face { .. } => {
            let plane = work_plane_world(app)?;
            Some((plane_json(&plane), plane.normal()))
        }
        _ => None,
    }
}

/// Open the form's first sketch on the work plane `sel`. On a face, the face's edges are not
/// projected into it: only what is drawn is the user's.
fn open_on(app: &mut SolveApp, sel: &Sel) -> Result<(), String> {
    let Sel::Face { point, .. } = sel else { return crate::dialogs::start_sketch(app, sel) };
    // Where the face is now, as a plane of its own: the form does not hang on the body under it.
    let plane = work_plane_world(app).ok_or("the face no longer exists")?;
    let params = json!({"plane": plane_json(&plane), "project_edges": false});
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
    // The sketch is named after the object it makes: `Extrusion 1`, `Sweep 1 · Path`.
    if let (Some(id), Some(s)) = (app.session.active_sketch, app.forms.session.as_ref())
        && let Some(name) = sketch_name(s, s.sketches.len().saturating_sub(1))
    {
        let _ = app.session.execute("timeline.rename", &json!({"feature": id, "name": name}));
    }
    app.ui.tab = "SKETCH".into();
    app.forms.open_ends.clear();
    app.start("cad.line");
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
            sum += p;
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
        if session.sketches.contains(&active) {
            // An earlier sketch of the form, opened again from Properties: back to the one
            // being drawn.
            let label = session.form.label(session.void);
            if let Err(why) = resume(app, session, active) {
                status(app, format!("{label}: {why}"), true);
            }
            return true;
        }
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

/// Which tools a form's sketch refuses, and why: a reference plane's takes a line only.
pub fn blocked_in_form(app: &SolveApp, id: &str) -> Option<&'static str> {
    let s = app.forms.session.as_ref()?;
    (s.form == Form::Reference
        && app.session.active_sketch.is_some()
        && (id.starts_with("cad.") || id.starts_with("sketch."))
        && !matches!(id, "cad.line" | "sketch.finish" | "sketch.cancel"))
    .then_some("A reference plane is drawn with a line only")
}

/// The first line drawn in the reference plane's sketch, in the world, and the sketch's normal.
fn reference_line(app: &SolveApp, sketch: u64) -> Option<(Vec3, Vec3, Vec3)> {
    let st = crate::open_meshing::session_world(&app.session);
    let ss = st.sketch(sketch)?;
    for e in crate::cad::geo::ents(&ss.sketch) {
        if let crate::cad::geo::Shape::Line(a, b) = e.shape
            && a.dist(b) > 1e-9
        {
            return Some((ss.plane.to_world(a), ss.plane.to_world(b), ss.plane.normal()));
        }
    }
    None
}

/// `cancel_sketch`'s hook: cancelling a form's sketch drops the whole form, with the sketches it
/// made before this one.
pub fn intercept_cancel(app: &mut SolveApp) -> bool {
    app.forms.after_edit = None;
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

/// The ends of the sketch's drawn curves that touch no other drawn curve: where its contour is
/// open. Construction geometry and centerlines don't count.
pub fn open_ends(app: &SolveApp, sketch: u64) -> Vec<Vec2> {
    let st = crate::open_meshing::session_world(&app.session);
    let Some(ss) = st.sketch(sketch) else { return Vec::new() };
    let ents: Vec<crate::cad::geo::Ent> = crate::cad::geo::ents(&ss.sketch).into_iter().filter(|e| !e.construction).collect();
    let tol = 1e-4;
    let mut out = Vec::new();
    for (i, e) in ents.iter().enumerate() {
        for end in e.shape.ends() {
            let joined = ents.iter().enumerate().any(|(j, o)| j != i && o.shape.dist(end) <= tol);
            if !joined && !out.iter().any(|p: &Vec2| p.dist(end) <= tol) {
                out.push(end);
            }
        }
    }
    out
}

/// The open ends Finish found, as red rings while the sketch is open.
pub fn paint_open_ends(app: &SolveApp, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    let Some(sid) = app.session.active_sketch else { return };
    if app.forms.open_ends.is_empty() {
        return;
    }
    let st = crate::open_meshing::session_world(&app.session);
    let Some(ss) = st.sketch(sid) else { return };
    for p in &app.forms.open_ends {
        if let Some(sp) = proj.to_screen(ss.plane.to_world(*p)) {
            painter.circle_stroke(sp, 7.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(0xe5, 0x3e, 0x3e)));
            painter.circle_filled(sp, 2.5, egui::Color32::from_rgb(0xe5, 0x3e, 0x3e));
        }
    }
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

/// The outer loops (curve ids) of the regions a form takes from `sketch`.
pub(crate) fn profile_loops(app: &SolveApp, sketch: u64) -> Vec<Vec<String>> {
    let st = crate::open_meshing::session_world(&app.session);
    let Some(ss) = st.sketch(sketch) else { return Vec::new() };
    solid_profiles(app, sketch).into_iter().filter_map(|i| ss.profiles.get(i).map(|p| p.outer_curves.clone())).collect()
}

fn profile_sels(sketch: u64, profiles: &[usize]) -> Vec<Sel> {
    profiles.iter().map(|&index| Sel::Profile { sketch, index }).collect()
}

/// Curves drawn in the sketch (not construction, centerlines, infinite or projected lines).
fn drawn_curves(ss: &solidcraft_engine::doc::SolvedSketch) -> Vec<usize> {
    ss.sketch.curves.iter().enumerate().filter(|(_, c)| !c.construction && !c.centerline && !c.infinite && c.link.is_none()).map(|(i, _)| i).collect()
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
    let closed = |app: &mut SolveApp| {
        let ends = open_ends(app, active);
        if !ends.is_empty() {
            let n = ends.len();
            app.forms.open_ends = ends;
            return Err(format!("the contour is not closed: {n} open end{} (marked red); join them, then Finish", if n == 1 { "" } else { "s" }));
        }
        app.forms.open_ends.clear();
        let p = solid_profiles(app, active);
        if p.is_empty() { Err("the sketch needs a closed loop (draw one, or close the open one), then Finish".to_string()) } else { Ok(p) }
    };
    match (session.form, stage) {
        (Form::Reference, _) => {
            let (a, b, n) = reference_line(app, active).ok_or("draw a line for the reference plane, then Finish")?;
            app.forms.session = None;
            leave(app, "sketch.cancel");
            crate::reference_planes::make(app, a, b, n);
        }
        (Form::ModelLine, _) => {
            app.forms.session = None;
            leave(app, "sketch.finish");
            status(app, "Model Line: done", false);
        }
        (Form::Extrusion, _) => {
            let profiles = closed(app)?;
            app.forms.session = None;
            leave(app, "sketch.finish");
            open_feature(app, "solid.extrude", vec![(0, profile_sels(active, &profiles))], &session);
        }
        (Form::Revolve, _) => {
            let profiles = closed(app)?;
            let axis = revolve_axis(app, active).ok_or("draw the axis line: SKETCH ▸ Centerline (or a construction line), then Finish")?;
            app.forms.session = None;
            leave(app, "sketch.finish");
            let axis = crate::selection::curve_pick(&axis, active, app.session.active_sketch);
            open_feature(app, "solid.revolve", vec![(0, profile_sels(active, &profiles)), (1, vec![axis])], &session);
        }
        (Form::Blend, 1) => {
            closed(app)?;
            let depth = match plain_mm(&session.pending.distance) {
                Some(d) if d > 0.0 => d,
                _ => (drawn_size(app, active) / 5.0).round().max(2.0) * 5.0,
            };
            session.pending.distance = format!("{depth} mm");
            let st = crate::open_meshing::session_world(&app.session);
            let base = st.sketch(active).map(|ss| ss.plane).ok_or("the sketch is gone")?;
            drop(st);
            leave(app, "sketch.finish");
            app.forms.session = Some(session);
            // The top: on the work plane offset by `depth` toward the base sketch's side, so it
            // follows the work plane when that moves; else (no reference) a plane parallel to the
            // base, `depth` along its normal.
            let top = base.offset(depth);
            let focus = drawn_centre(app, active).map(|c| c + base.normal() * depth);
            let tied = work_plane_ref(app).map(|(plane, normal)| {
                let sign = if normal.dot(base.normal()) < 0.0 { -1.0 } else { 1.0 };
                json!({"plane": plane, "offset": format!("{} mm", sign * depth), "project_edges": false})
            });
            let opened = match tied {
                Some(params) => open_next(app, params, focus).or_else(|_| open_next(app, json!({"plane": plane_json(&top)}), focus)),
                None => open_next(app, json!({"plane": plane_json(&top)}), focus),
            };
            opened.inspect_err(|_| abort(app, true))?;
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
            open_feature(app, "solid.loft", vec![(0, sections)], &session);
        }
        (Form::Sweep | Form::SweptBlend, 1) => {
            let path = path_of(app, active)?;
            let start = path.start;
            session.path = Some(path);
            leave(app, "sketch.finish");
            app.forms.session = Some(session.clone());
            open_next(app, json!({"plane": plane_json(&start)}), Some(start.origin)).inspect_err(|_| abort(app, true))?;
            let what = if session.form == Form::Sweep { "the profile" } else { "the first profile" };
            status(app, format!("{}: draw {what} (a closed loop across the path's start), then Finish", session.form.label(session.void)), false);
        }
        (Form::Sweep, _) => {
            let profiles = closed(app)?;
            let path = session.path.clone().ok_or("the path is gone")?;
            app.forms.session = None;
            leave(app, "sketch.finish");
            let curves = path.curves.iter().map(|id| crate::selection::curve_pick(id, path.sketch, app.session.active_sketch)).collect();
            open_feature(app, "solid.sweep", vec![(0, profile_sels(active, &profiles)), (1, curves)], &session);
        }
        (Form::SweptBlend, 2) => {
            closed(app)?;
            let end = session.path.as_ref().map(|p| p.end).ok_or("the path is gone")?;
            leave(app, "sketch.finish");
            app.forms.session = Some(session.clone());
            open_next(app, json!({"plane": plane_json(&end)}), Some(end.origin)).inspect_err(|_| abort(app, true))?;
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
            open_feature(app, "solid.loft", vec![(0, sections)], &session);
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
fn open_feature(app: &mut SolveApp, command: &'static str, inputs: Vec<(usize, Vec<Sel>)>, session: &FormSession) {
    let (void, name, pending) = (session.void, session.name.clone(), session.pending.clone());
    app.forms.fill = Some(Fill { command, inputs, void, name, pending, frames: 30 });
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
    // A form is an object of its own: it never joins others by itself (MODIFY ▸ Join does); a
    // void cuts. Either way the object is one body, whatever its sketch's loops or the cut.
    let op = if fill.void { 2 } else { 0 };
    let p = &fill.pending;
    match &mut d.kind {
        Kind::Extrude { operation, auto_op, distance, start, .. } => {
            *operation = op;
            *auto_op = false;
            let (s0, s1) = (plain_mm(&p.start).unwrap_or(0.0), plain_mm(&p.end));
            match s1 {
                Some(e) if (e - s0).abs() > 1e-9 => *distance = format!("{} mm", round6(e - s0)),
                Some(_) => {}
                None => *distance = p.end.clone(),
            }
            if s0 != 0.0 {
                *start = format!("{} mm", round6(s0));
            }
        }
        Kind::Revolve { operation, angle, .. } => {
            *operation = op;
            *angle = p.angle.clone();
        }
        Kind::Loft { operation, .. } | Kind::Sweep { operation, .. } => *operation = op,
        _ => {}
    }
    d.extra.insert("touching".into(), json!("one_body"));
    if !fill.void {
        d.extra.insert("body_name".into(), json!(fill.name));
        d.extra.insert("name".into(), json!(fill.name));
    } else {
        d.extra.insert("name".into(), json!(fill.name));
    }
    app.forms.fill = None;
    // The feature's window opened from the MODEL tab: the ribbon stays there.
    app.ui.tab = "MODEL".into();
    // Made at once, with the values set in Properties; changed there afterwards.
    let Some(dialog) = app.dialog.take() else { return true };
    let commands = crate::dialogs::apply_commands(app, &dialog);
    let made = commands.and_then(|cmds| {
        for (cmd, params) in cmds {
            app.run(&cmd, params)?;
        }
        Ok(())
    });
    match made {
        Ok(()) => {
            if !fill.void && app.session.model.state().body(&fill.name).is_some() {
                let _ = app.run("select.set", json!({"items": [{"type": "body", "name": fill.name}]}));
            }
            app.shell_ui.properties = true;
            let text = if fill.void {
                format!("{} cut; change it in Properties", fill.name)
            } else {
                format!("{} made; change it in Properties", fill.name)
            };
            status(app, text, false);
        }
        Err(e) => {
            // Back to the window, to fix what failed.
            app.dialog = Some(dialog);
            status(app, format!("{}: {e}", fill.name), true);
        }
    }
    true
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// A length expression's value in mm when it is a plain number, with or without `mm`.
pub(crate) fn plain_mm(e: &str) -> Option<f64> {
    e.trim().trim_end_matches("mm").trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The next free name for an object made by `tool`: `Extrusion 1`, `Extrusion 2`…
fn next_object_name(app: &SolveApp, tool: &str) -> String {
    let st = app.session.model.state();
    let taken = |n: &str| st.body(n).is_some() || app.session.doc.features.iter().any(|f| f.name == n || f.name.starts_with(&format!("{n} · ")));
    (1..10_000).map(|k| format!("{tool} {k}")).find(|n| !taken(n)).unwrap_or_else(|| tool.to_string())
}

/// A form's `index`th sketch's name: the object's, and what it is for when there are several.
fn sketch_name(s: &FormSession, index: usize) -> Option<String> {
    let part = match (s.form, index) {
        (Form::Extrusion | Form::Revolve, _) | (Form::ModelLine, _) => None,
        (Form::Reference, _) => return None,
        (Form::Blend, 0) => Some("Base"),
        (Form::Blend, _) => Some("Top"),
        (Form::Sweep | Form::SweptBlend, 0) => Some("Path"),
        (Form::Sweep, _) => Some("Profile"),
        (Form::SweptBlend, 1) => Some("Profile 1"),
        (Form::SweptBlend, _) => Some("Profile 2"),
    };
    Some(match part {
        Some(p) => format!("{} · {p}", s.name),
        None => s.name.clone(),
    })
}

/// The labels of a form's sketches, in order (made or still to come).
fn stage_labels(form: Form) -> &'static [&'static str] {
    match form {
        Form::Blend => &["Base", "Top"],
        Form::Sweep => &["Path", "Profile"],
        Form::SweptBlend => &["Path", "Profile 1", "Profile 2"],
        _ => &["Sketch"],
    }
}

/// An earlier sketch of the form was finished after being opened again: the path is read again
/// (a Sweep's profiles stay where they were drawn) and the sketch being drawn opens again.
fn resume(app: &mut SolveApp, mut session: FormSession, active: u64) -> Result<(), String> {
    if session.path.as_ref().is_some_and(|p| p.sketch == active) {
        session.path = Some(path_of(app, active)?);
    }
    leave(app, "sketch.finish");
    let last = *session.sketches.last().ok_or("the form has no sketch")?;
    app.forms.session = Some(session);
    reopen(app, last)
}

fn reopen(app: &mut SolveApp, sketch: u64) -> Result<(), String> {
    app.forms.leaving = true;
    app.edit_feature(sketch);
    app.forms.leaving = false;
    if app.session.active_sketch != Some(sketch) {
        return Err("the sketch could not be opened".into());
    }
    app.ui.tab = "SKETCH".into();
    Ok(())
}

/// Properties while a form is drawn: its name, its sketches (open any made so far, and come
/// back), and its values.
pub fn properties_ui(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    let Some(session) = app.forms.session.clone() else { return false };
    if session.form == Form::Reference {
        return false;
    }
    let active = app.session.active_sketch;
    ui.label(egui::RichText::new(&session.name).strong());
    ui.label(egui::RichText::new(if session.void { "Void (cuts what it meets)" } else { "One object" }).small().weak());
    ui.add_space(4.0);
    let labels = stage_labels(session.form);
    let mut open: Option<u64> = None;
    if session.form != Form::ModelLine && labels.len() > 1 {
        ui.label("Sketches");
        ui.horizontal_wrapped(|ui| {
            for (i, label) in labels.iter().enumerate() {
                let made = session.sketches.get(i).copied();
                let current = made.is_some() && made == active;
                let r = ui.add_enabled(made.is_some() && !current, egui::Button::new(*label).selected(current));
                crate::scenario::publish_target(&format!("form:stage:{label}"), r.rect);
                if r.clicked() {
                    open = made;
                }
            }
        });
        ui.add_space(4.0);
    }
    let mut pending = session.pending.clone();
    let mut changed = false;
    egui::Grid::new("sc_form_values").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        let mut field = |ui: &mut egui::Ui, label: &str, value: &mut String| {
            ui.label(label);
            let r = ui.add(egui::TextEdit::singleline(value).desired_width(120.0));
            crate::scenario::publish_target(&format!("form:{label}"), r.rect);
            changed |= r.changed();
            ui.end_row();
        };
        match session.form {
            Form::Extrusion => {
                field(ui, "Start", &mut pending.start);
                field(ui, "End", &mut pending.end);
            }
            Form::Revolve => field(ui, "Angle", &mut pending.angle),
            Form::Blend if session.sketches.len() <= 1 => {
                if pending.distance.is_empty() {
                    pending.distance = "auto".into();
                }
                field(ui, "Distance", &mut pending.distance);
                if pending.distance == "auto" {
                    pending.distance.clear();
                }
            }
            _ => {}
        }
    });
    if changed && let Some(s) = app.forms.session.as_mut() {
        s.pending = pending;
    }
    ui.label(egui::RichText::new("Finish the sketch to go on; the object is made with these values.").small().weak());
    if let Some(sketch) = open
        && let Some(current) = active
    {
        // Leave the sketch being drawn as it is (no checks) and open the one asked for.
        app.forms.leaving = true;
        app.leave_sketch("sketch.finish");
        app.forms.leaving = false;
        // A path changed: the profiles still to come go across it as it is now.
        if session.path.as_ref().is_some_and(|p| p.sketch == current)
            && let Ok(path) = path_of(app, current)
            && let Some(s) = app.forms.session.as_mut()
        {
            s.path = Some(path);
        }
        if let Err(e) = reopen(app, sketch) {
            status(app, e, true);
        }
    }
    true
}

/// Whether Set Work Plane or Orient to a plane is waiting for a pick.
pub fn picking_plane(app: &SolveApp) -> bool {
    app.forms.picking_work_plane || app.forms.picking_view_plane
}

/// The plane or flat face a click would take while picking a plane: lit under the pointer.
pub fn plane_candidate(app: &SolveApp, hits: &[crate::viewport::Hit]) -> Option<(crate::viewport::Hit, Sel)> {
    hits.iter().find_map(|h| match h {
        crate::viewport::Hit::Plane { name, .. } => Some((h.clone(), Sel::Plane { name: name.clone() })),
        crate::viewport::Hit::Face { body, index, point } => {
            Some(Sel::Face { body: body.clone(), index: *index, point: *point }).filter(|s| plane_pick(app, s)).map(|s| (h.clone(), s))
        }
        _ => None,
    })
}

// ---- Every frame --------------------------------------------------------------------------

/// Once a frame: Set Work Plane's pick, the feature window waiting to be filled, and a form
/// whose sketch was closed some other way (undo, another document).
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if std::mem::take(&mut app.forms.clear_selection) {
        let _ = app.session.execute("select.clear", &json!({}));
    }
    if app.forms.picking_work_plane || app.forms.picking_view_plane {
        let click = ctx.input(|i| if i.pointer.primary_clicked() { i.pointer.interact_pos() } else { None });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape))
            || app.dialog.is_some()
            || (app.forms.picking_work_plane && app.session.active_sketch.is_some())
        {
            app.forms.picking_work_plane = false;
            app.forms.picking_view_plane = false;
            status(app, "Cancelled", false);
        } else if let Some(pos) = click
            && let Some(rect) = app.viewport.rect.filter(|r| r.contains(pos))
        {
            // What is under the click, whatever the selection filter would make of it (a click
            // on a face selects its body by default).
            let proj = crate::viewport::projection(app, rect);
            let hits = crate::viewport::pick(app, &proj, pos);
            let found = plane_candidate(app, &hits).map(|(_, s)| s);
            match found {
                Some(sel) if app.forms.picking_view_plane => orient_to(app, sel),
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
            if app.forms.picking_view_plane {
                orient_to(app, sel);
            } else {
                adopt_work_plane(app, sel);
            }
        }
    }
    // A reference plane is made as soon as its line is drawn.
    if let (Some(session), Some(active)) = (app.forms.session.clone(), app.session.active_sketch)
        && session.form == Form::Reference
        && session.sketches.last() == Some(&active)
        && reference_line(app, active).is_some()
        && let Err(why) = step(app, session, active)
    {
        status(app, format!("Reference Plane: {why}"), true);
    }
    if let Some((sketch, feature, seen)) = app.forms.after_edit {
        if app.session.active_sketch == Some(sketch) {
            app.forms.after_edit = Some((sketch, feature, true));
        } else if seen && app.session.active_sketch.is_none() && app.dialog.is_none() && app.deferred.is_none() {
            app.forms.after_edit = None;
            // The object takes the sketch's closed regions as they are now (a loop drawn around
            // the old one, a hole removed), each by its outer loop.
            let loops = profile_loops(app, sketch);
            if !loops.is_empty()
                && let Err(e) = app.run("timeline.set_profiles", json!({"feature": feature, "profiles": loops}))
            {
                status(app, format!("The object keeps its old outline: {e}"), true);
            }
            // The object follows its sketch; its values are in Properties.
            let st = crate::open_meshing::session_world(&app.session);
            let body = st.bodies.iter().find(|b| b.feature == feature).map(|b| b.name.clone());
            drop(st);
            if let Some(body) = body {
                let _ = app.run("select.set", json!({"items": [{"type": "body", "name": body}]}));
                app.shell_ui.properties = true;
                status(app, format!("{body} follows its sketch; change its values in Properties"), false);
            }
        } else if seen || app.session.doc.feature(feature).is_none() {
            app.forms.after_edit = None;
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
        && session.sketches.last().is_some_and(|_| app.session.active_sketch.is_none_or(|a| !session.sketches.contains(&a)))
    {
        let label = session.form.label(session.void);
        app.forms.session = None;
        status(app, format!("{label}: its sketch was closed; the form was left unfinished"), false);
    }
}
