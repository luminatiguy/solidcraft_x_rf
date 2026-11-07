//! The sketch's drawing and editing tools, after CADCraft's: each tool is a prompt machine
//! (CADCraft's `Interactive`) asking for points, values, options and objects in turn. Points come
//! from clicks, snapped to the sketch's and the model's geometry (`snap`) or to the grid; values
//! are typed (shown at the cursor; Enter or Space takes them, a right-click is Enter); options are
//! the buttons of the prompt bar at the bottom of the view. The geometry is made with the
//! sketch's own commands, without constraints: the sketch is plain geometry, as a drawing is.

mod draw;
pub mod geo;
mod modify;
pub mod snap;

use egui::{Color32, Pos2, Stroke, vec2};
use serde_json::{Value, json};
use solidcore_record::{Plane, Vec2};
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::viewport::{Hit as VHit, Proj};
use geo::Ent;

/// What a prompt takes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Accept {
    pub point: bool,
    pub number: bool,
    pub select: bool,
}

impl Accept {
    pub const POINT: Accept = Accept { point: true, number: false, select: false };
    pub const POINT_OR_NUMBER: Accept = Accept { point: true, number: true, select: false };
    pub const NUMBER: Accept = Accept { point: false, number: true, select: false };
    pub const SELECT: Accept = Accept { point: true, number: false, select: true };
    pub const PICK: Accept = Accept { point: true, number: false, select: false };
}

#[derive(Clone, Debug, Default)]
pub struct Prompt {
    pub message: String,
    pub keywords: Vec<&'static str>,
    pub default: Option<String>,
    pub accept: Accept,
    /// Rubber-band origin: typed distances go from it toward the cursor, ortho bends to it.
    pub base: Option<Vec2>,
}

impl Prompt {
    pub fn new(message: impl Into<String>, accept: Accept) -> Prompt {
        Prompt { message: message.into(), accept, ..Default::default() }
    }
    pub fn kw(mut self, k: &[&'static str]) -> Prompt {
        self.keywords = k.to_vec();
        self
    }
    pub fn default(mut self, d: impl Into<String>) -> Prompt {
        self.default = Some(d.into());
        self
    }
    pub fn base(mut self, b: Option<Vec2>) -> Prompt {
        self.base = b;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// A point (snapped), with the sketch curve under the cursor, if any.
    Point {
        p: Vec2,
        curve: Option<String>,
    },
    Number(f64),
    Keyword(&'static str),
    Enter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Done,
}

pub type Res = Result<Step, String>;

/// What a tool draws under the cursor, in sketch coordinates.
pub enum Prev {
    Line(Vec<Vec2>),
    /// A selection window: left to right (window) or crossing.
    Window(Vec2, Vec2),
}

pub trait Machine {
    fn prompt(&self, app: &SolveApp) -> Prompt;
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res;
    /// Called once at the start (may finish at once).
    fn begin(&mut self, _app: &mut SolveApp) -> Res {
        Ok(Step::Continue)
    }
    fn preview(&self, _app: &SolveApp, _cursor: Vec2) -> Vec<Prev> {
        Vec::new()
    }
    /// Curves shown as selected (the objects a modify tool works on).
    fn selected(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The sketch tools' drafting settings (CADCraft's Drafting Settings and its status bar
/// switches), kept with the preferences.
#[derive(Clone, Debug)]
pub struct Settings {
    pub osnap: bool,
    pub grid_snap: bool,
    pub ortho: bool,
    pub nearest: bool,
    /// The grid's step for Grid Snap (mm).
    pub grid_step: f64,
    pub step_text: String,
    /// The running object snap modes, a bit per kind ([`snap::MODES`]).
    pub modes: u16,
    /// Polar tracking (F10) and its angle step in degrees.
    pub polar: bool,
    pub polar_step: f64,
    /// Dynamic Input (F12): the value boxes at the cursor.
    pub dyn_input: bool,
    /// Second and next points are typed as distance and angle (else X and Y) …
    pub dyn_polar: bool,
    /// … measured from the last point (else absolute).
    pub dyn_relative: bool,
    /// The snap aperture, in screen points.
    pub aperture: f32,
    /// The Drafting Settings window is open.
    pub dialog: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            osnap: true,
            grid_snap: false,
            ortho: false,
            nearest: false,
            grid_step: 1.0,
            step_text: "1".into(),
            modes: snap::DEFAULT_MODES,
            polar: true,
            polar_step: 90.0,
            dyn_input: true,
            dyn_polar: true,
            dyn_relative: true,
            aperture: 10.0,
            dialog: false,
        }
    }
}

impl Settings {
    pub fn to_json(&self) -> Value {
        json!({"osnap": self.osnap, "grid_snap": self.grid_snap, "ortho": self.ortho, "grid_step": self.grid_step, "modes": self.modes,
            "polar": self.polar, "polar_step": self.polar_step, "dyn_input": self.dyn_input, "dyn_polar": self.dyn_polar,
            "dyn_relative": self.dyn_relative, "aperture": self.aperture})
    }
    pub fn load(&mut self, v: &Value) {
        let b = |k: &str| v.get(k).and_then(Value::as_bool);
        let f = |k: &str| v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        if let Some(x) = b("osnap") {
            self.osnap = x;
        }
        if let Some(x) = b("grid_snap") {
            self.grid_snap = x;
        }
        if let Some(x) = b("ortho") {
            self.ortho = x;
        }
        if let Some(x) = f("grid_step").filter(|x| *x > 0.0) {
            self.grid_step = x;
            self.step_text = geo::fmt(x);
        }
        if let Some(x) = v.get("modes").and_then(Value::as_u64) {
            self.modes = (x & 0xffff) as u16;
        }
        if let Some(x) = b("polar") {
            self.polar = x;
        }
        if let Some(x) = f("polar_step").filter(|x| *x > 0.0 && *x <= 180.0) {
            self.polar_step = x;
        }
        if let Some(x) = b("dyn_input") {
            self.dyn_input = x;
        }
        if let Some(x) = b("dyn_polar") {
            self.dyn_polar = x;
        }
        if let Some(x) = b("dyn_relative") {
            self.dyn_relative = x;
        }
        if let Some(x) = f("aperture").filter(|x| (2.0..=50.0).contains(x)) {
            self.aperture = x as f32;
        }
    }
}

struct Running {
    cmd: &'static str,
    m: Box<dyn Machine>,
    typed: String,
    error: Option<String>,
}

#[derive(Default)]
pub struct State {
    pub settings: Settings,
    run: Option<Running>,
    /// The cursor on the sketch plane, after snaps; its snap; the sketch curve under it.
    cursor: Option<Vec2>,
    raw: Option<Vec2>,
    hit: Option<snap::Hit>,
    hover_curve: Option<String>,
    /// The last tool, for Enter on an empty prompt.
    last: Option<&'static str>,
    /// Keyword clicked in the prompt bar, fed next frame.
    pending: Option<Input>,
    /// Polar tracking's line: from the base, at this angle (radians).
    track: Option<(Vec2, f64)>,
}

/// What each tool does, for its tooltip (CADCraft's descriptions).
const DESCRIPTIONS: &[(&str, &str)] = &[
    ("cad.line", "Draws straight line segments, one after another."),
    ("cad.pline", "Draws a polyline: connected straight and arc segments that form one object."),
    ("cad.circle", "Draws a circle by its centre and radius, or by points."),
    ("cad.rectang", "Draws a rectangle by two corners, or by its width and height."),
    ("cad.arc", "Draws an arc through three points, or by centre, start and end."),
    ("cad.spline", "Draws a smooth curve through or near the points you pick."),
    ("cad.xline", "Draws an infinite construction line, for alignment."),
    ("cad.ellipse", "Draws an ellipse or an elliptical arc."),
    ("cad.polygon", "Draws a regular polygon with as many sides as you like."),
    ("cad.point", "Places points, one after another."),
    ("cad.donut", "Draws a filled ring or a filled circle."),
    ("cad.revcloud", "Draws a revision cloud around something to mark a change."),
    ("cad.move", "Moves objects by a distance, from a base point to another point."),
    ("cad.copy", "Copies objects to other places."),
    ("cad.rotate", "Turns objects around a base point."),
    ("cad.trim", "Cuts off the parts of objects that cross other objects. Hover to see what goes."),
    ("cad.fillet", "Rounds the corner between two objects with an arc."),
    ("cad.arrayrect", "Copies objects in rows and columns."),
    ("cad.mirror", "Makes a mirror copy of objects across a line (and can erase the originals)."),
    ("cad.scale", "Makes objects larger or smaller from a base point."),
    ("cad.extend", "Lengthens objects to meet other objects. Hover to see the extension."),
    ("cad.offset", "Makes a parallel copy of a line, curve or outline at a distance."),
    ("cad.arraypolar", "Copies objects around a centre point."),
    ("cad.stretch", "Stretches objects crossed by a selection window, moving their points inside it."),
    ("cad.chamfer", "Bevels the corner between two lines."),
    ("cad.explode", "Breaks a compound object (a polyline, a rectangle) into its parts."),
    ("cad.break", "Breaks an object in two, or removes a part between two points."),
    ("cad.join", "Joins lines and arcs that touch into one object."),
    ("cad.erase", "Deletes the selected objects."),
    ("cad.lengthen", "Changes the length of lines and arcs."),
    ("cad.matchprop", "Copies the properties of one object (construction or not) to others."),
    ("form.extrusion", "Draw a closed profile on the work plane, then give the depth: a solid pulled straight out of it."),
    ("form.blend", "Draw a base profile and a top profile: a solid blending from one to the other."),
    ("form.revolve", "Draw a closed profile and an axis line: a solid turned around the axis."),
    ("form.sweep", "Draw a path, then a closed profile across it: a solid swept along the path."),
    ("form.swept_blend", "Draw a path and a profile at each end: a solid blending along the path."),
    ("workplane.set", "Pick a plane or a flat face: new sketches go on it."),
    ("workplane.show", "Shows the work plane's grid and outlines the plane it sits on."),
    (
        "workplane.reference",
        "Draw a line in a sketch: a reference plane through it, square to the sketch. It shows as a dashed line where it is seen edge-on.",
    ),
    ("model.line", "Draws lines on the work plane that stay in the model (and show on drawings)."),
    ("obj.move", "Moves the selected object from a base point to another point."),
    ("obj.copy", "Copies the selected object from a base point to another point."),
    ("obj.rotate", "Turns the selected object around an axis."),
    ("obj.mirror", "Mirrors the selected object across a plane."),
    ("obj.join", "Joins objects that touch or overlap into one."),
    ("obj.unjoin", "Takes back the last join of the selected object."),
    ("obj.cut", "Cuts objects out of another."),
];

/// A tool's description, for its tooltip.
pub fn description(id: &str) -> Option<&'static str> {
    DESCRIPTIONS.iter().find(|(c, _)| *c == id).map(|(_, d)| *d)
}

fn make(id: &str) -> Option<Box<dyn Machine>> {
    draw::make(id).or_else(|| modify::make(id))
}

// ---- Starting and ending ------------------------------------------------------------------

/// `start`'s hook: the sketch tools (`cad.*`).
pub fn start_hook(app: &mut SolveApp, id: &str) -> bool {
    if !id.starts_with("cad.") {
        return false;
    }
    let Some(m) = make(id) else { return false };
    if app.session.active_sketch.is_none() {
        app.set_status("Open a sketch first: choose a form tool on the MODEL tab", true);
        return true;
    }
    crate::dialogs::cancel(app);
    app.dialog = None;
    let cmd = solidcraft_engine::find_command(id).map_or("cad", |c| c.id);
    app.tool = crate::tools::Tool::for_command(cmd);
    app.cad.run = Some(Running { cmd, m, typed: String::new(), error: None });
    app.cad.last = Some(cmd);
    let step = with_machine(app, |m, app| m.begin(app));
    after(app, step);
    true
}

fn with_machine(app: &mut SolveApp, f: impl FnOnce(&mut dyn Machine, &mut SolveApp) -> Res) -> Res {
    let Some(mut run) = app.cad.run.take() else { return Ok(Step::Done) };
    let r = f(run.m.as_mut(), app);
    // The machine may have started another tool meanwhile (it never does today): keep ours.
    if app.cad.run.is_none() {
        app.cad.run = Some(run);
    }
    r
}

fn after(app: &mut SolveApp, step: Res) {
    match step {
        Ok(Step::Continue) => {
            if let Some(r) = app.cad.run.as_mut() {
                r.error = None;
            }
        }
        Ok(Step::Done) => end(app),
        Err(e) => {
            if let Some(r) = app.cad.run.as_mut() {
                r.error = Some(e.clone());
            }
            app.set_status(e, true);
        }
    }
}

fn end(app: &mut SolveApp) {
    app.cad.run = None;
    if app.tool.as_ref().is_some_and(|t| t.kind == crate::tools::Kind::Cad) {
        app.tool = None;
    }
}

/// Esc: the tool ends (what it already made stays, as in CADCraft).
pub fn escape(app: &mut SolveApp) {
    if let Some(r) = app.cad.run.as_mut()
        && !r.typed.is_empty()
    {
        r.typed.clear();
        return;
    }
    end(app);
}

pub(crate) fn feed(app: &mut SolveApp, i: Input) {
    let step = with_machine(app, |m, app| m.input(app, i));
    after(app, step);
}

/// Enter (or Space, or a right-click): the typed value, else the prompt's Enter.
pub fn enter(app: &mut SolveApp) {
    let Some(prompt) = app.cad.run.as_ref().map(|r| r.m.prompt(app)) else { return };
    let Some(run) = app.cad.run.as_mut() else { return };
    let typed = std::mem::take(&mut run.typed);
    let typed = dyn_submit(&app.cad.settings, &prompt, &typed, app.cad.cursor.unwrap_or(Vec2::ZERO));
    let input = if typed.trim().is_empty() { Ok(Input::Enter) } else { typed_input(app, &prompt, &typed) };
    match input {
        Ok(i) => feed(app, i),
        Err(e) => {
            if let Some(r) = app.cad.run.as_mut() {
                r.error = Some(e);
            }
        }
    }
}

/// A typed answer: "x,y" (absolute), "@dx,dy" (from the base), "@d<a" or "d<a" (polar), a
/// number (a value, or a distance from the base toward the cursor: direct distance entry).
fn typed_input(app: &SolveApp, prompt: &Prompt, t: &str) -> Result<Input, String> {
    let t = t.trim();
    let rel = t.starts_with('@');
    let body = t.trim_start_matches('@');
    let base = if rel { prompt.base.or(app.cad.cursor).unwrap_or(Vec2::ZERO) } else { Vec2::ZERO };
    if let Some((d, a)) = body.split_once('<') {
        let (d, a) = (geo::number(d).ok_or("Requires a distance")?, geo::number(a).ok_or("Requires an angle")?);
        let p = base + geo::polar(a.to_radians()) * d;
        return if prompt.accept.point { Ok(Input::Point { p, curve: None }) } else { Err("Requires a value".into()) };
    }
    if let Some((x, y)) = body.split_once(',') {
        let p = base + Vec2::new(geo::number(x).ok_or("Requires a point: x,y")?, geo::number(y).ok_or("Requires a point: x,y")?);
        return if prompt.accept.point { Ok(Input::Point { p, curve: None }) } else { Err("Requires a value".into()) };
    }
    let v = geo::number(body).ok_or("Requires a number, a point (x,y) or @dx,dy")?;
    if prompt.accept.number {
        return Ok(Input::Number(v));
    }
    if prompt.accept.point
        && let (Some(b), Some(c)) = (prompt.base, app.cad.cursor)
        && let Some(dir) = (c - b).normalized()
    {
        return Ok(Input::Point { p: b + dir * v, curve: None });
    }
    Err("Requires a point".into())
}

// ---- The cursor ---------------------------------------------------------------------------

fn active(app: &SolveApp) -> Option<(u64, solidcraft_engine::sketch::Sketch, Plane)> {
    let sid = app.session.active_sketch?;
    let st = crate::open_meshing::shown_world(app);
    let ss = st.sketch(sid)?;
    Some((sid, ss.sketch.clone(), ss.plane))
}

/// The open sketch's curves.
pub fn ents(app: &SolveApp) -> Vec<Ent> {
    let Some(sid) = app.session.active_sketch else { return Vec::new() };
    let st = crate::open_meshing::shown_world(app);
    st.sketch(sid).map(|ss| geo::ents(&ss.sketch)).unwrap_or_default()
}

pub fn ent(app: &SolveApp, id: &str) -> Option<Ent> {
    ents(app).into_iter().find(|e| e.id == id)
}

/// Follow the cursor: where it is on the sketch plane after snaps and ortho.
pub fn hover(app: &mut SolveApp, proj: &Proj, pos: Pos2) {
    let Some((sid, sk, plane)) = active(app) else { return };
    let (o, d) = proj.ray(pos);
    let Some(w) = plane.intersect_ray(o, d) else { return };
    let raw = plane.to_local(w);
    let prompt = app.cad.run.as_ref().map(|r| r.m.prompt(app)).unwrap_or_default();
    let curve = crate::viewport::pick(app, proj, pos).into_iter().find_map(|h| match h {
        VHit::SketchCurve { sketch, id, .. } if sketch == sid => Some(id),
        _ => None,
    });
    let s = &app.cad.settings;
    let mut hit = None;
    let mut track = None;
    let mut p = raw;
    // Selecting picks objects: the cursor is not snapped then.
    if !prompt.accept.select {
        let mm_px = snap::mm_per_px(&plane, proj, raw);
        if s.osnap && !crate::drag_snap::suppressed() {
            let own = geo::ents(&sk);
            let foreign = snap::foreign(app, sid, &plane);
            let aperture = f64::from(s.aperture) * mm_px;
            hit = snap::find_modes(&own, &foreign, raw, aperture, prompt.base, s.modes | if s.nearest { 1 << snap::NEA } else { 0 });
        }
        if let Some(h) = hit {
            p = h.p;
        } else if let Some((b, a)) = polar_track(s, prompt.base, raw, mm_px) {
            // Polar tracking: on the ray from the last point at a multiple of the step.
            let dir = geo::polar(a);
            p = b + dir * (raw - b).dot(dir).max(0.0);
            track = Some((b, a));
        } else {
            if s.ortho
                && let Some(b) = prompt.base
            {
                let d = raw - b;
                p = if d.x.abs() >= d.y.abs() { Vec2::new(raw.x, b.y) } else { Vec2::new(b.x, raw.y) };
            }
            if s.grid_snap {
                let g = snap::grid(p, s.grid_step);
                // Ortho keeps the base's other coordinate.
                p = match (s.ortho, prompt.base) {
                    (true, Some(b)) if (p.y - b.y).abs() < 1e-12 => Vec2::new(g.x, b.y),
                    (true, Some(b)) if (p.x - b.x).abs() < 1e-12 => Vec2::new(b.x, g.y),
                    _ => g,
                };
                hit = Some(snap::Hit { p, kind: snap::GRID });
            }
        }
    }
    // Dynamic Input: a value typed and locked (Tab) holds the cursor to it.
    if let Some(run) = app.cad.run.as_ref()
        && prompt.accept.point
        && !prompt.accept.select
        && let Some(e) = dyn_entry(&app.cad.settings, &prompt, &run.typed)
    {
        p = e.apply(p);
    }
    app.cad.raw = Some(raw);
    app.cad.cursor = Some(p);
    app.cad.hit = hit;
    app.cad.track = track;
    app.cad.hover_curve = curve;
}

/// Polar tracking: the base and the tracked angle when the cursor is near a multiple of the step.
fn polar_track(s: &Settings, base: Option<Vec2>, raw: Vec2, mm_px: f64) -> Option<(Vec2, f64)> {
    let b = base?;
    if !s.polar || s.ortho {
        return None;
    }
    let d = raw - b;
    let len = d.len();
    if len < 8.0 * mm_px {
        return None;
    }
    let step = s.polar_step.to_radians();
    let a = d.y.atan2(d.x);
    let k = (a / step).round();
    let tracked = k * step;
    let off = (a - tracked).sin().abs() * len;
    (off <= 8.0 * mm_px).then_some((b, tracked))
}

// ---- Dynamic Input ------------------------------------------------------------------------------

/// The value boxes' reading of what is typed: polar (distance, angle) or Cartesian (X, Y), each
/// value typed or not, and whether it is locked (a separator typed after it).
#[derive(Clone, Debug, PartialEq)]
pub struct DynEntry {
    pub polar: bool,
    pub first: String,
    pub second: String,
    pub first_locked: bool,
    /// The point the values are measured from (the last point when relative).
    pub origin: Vec2,
    pub relative: bool,
}

impl DynEntry {
    /// The cursor held to the locked values.
    pub fn apply(&self, cursor: Vec2) -> Vec2 {
        let first = geo::number(&self.first).filter(|_| self.first_locked);
        let second = geo::number(&self.second);
        if self.polar {
            let toward = cursor - self.origin;
            let angle = second.map(f64::to_radians).unwrap_or(toward.y.atan2(toward.x));
            let dist = first.unwrap_or(toward.len());
            self.origin + geo::polar(angle) * dist
        } else {
            let x = first.map_or(cursor.x, |v| self.origin.x + v);
            let y = second.map_or(cursor.y, |v| self.origin.y + v);
            Vec2::new(x, y)
        }
    }

    /// The live values the open boxes show.
    pub fn live(&self, cursor: Vec2) -> (f64, f64) {
        let d = cursor - self.origin;
        if self.polar { (d.len(), d.y.atan2(d.x).to_degrees().rem_euclid(360.0)) } else { (d.x, d.y) }
    }
}

/// The boxes for a point prompt (none for other prompts, or typed `@`/`#` entries, which go
/// through as typed).
pub fn dyn_entry(s: &Settings, prompt: &Prompt, typed: &str) -> Option<DynEntry> {
    if !s.dyn_input || !prompt.accept.point || prompt.accept.select || typed.starts_with('@') || typed.starts_with('#') {
        return None;
    }
    let relative = prompt.base.is_some() && s.dyn_relative;
    let origin = if relative { prompt.base.unwrap_or(Vec2::ZERO) } else { Vec2::ZERO };
    let (polar, first, second, locked) = if let Some((a, b)) = typed.split_once('<') {
        (true, a, b, true)
    } else if let Some((a, b)) = typed.split_once(',') {
        (false, a, b, true)
    } else {
        (prompt.base.is_some() && s.dyn_polar, typed, "", false)
    };
    Some(DynEntry { polar, first: first.to_string(), second: second.to_string(), first_locked: locked, origin, relative })
}

/// Tab at a point prompt: lock the first box (typing the separator, or the live value and the
/// separator when nothing is typed) and move to the second.
fn dyn_tab(app: &mut SolveApp) {
    let Some(prompt) = app.cad.run.as_ref().map(|r| r.m.prompt(app)) else { return };
    let cursor = app.cad.cursor.unwrap_or(Vec2::ZERO);
    let s = app.cad.settings.clone();
    let Some(run) = app.cad.run.as_mut() else { return };
    let Some(e) = dyn_entry(&s, &prompt, &run.typed) else { return };
    if e.first_locked {
        // Tab on the second box: lock it as well by filling it from the cursor when empty.
        if e.second.is_empty() {
            let (_, b) = e.live(cursor);
            run.typed.push_str(&geo::fmt((b * 1000.0).round() / 1000.0));
        }
        return;
    }
    if e.first.is_empty() {
        let (a, _) = e.live(cursor);
        run.typed.push_str(&geo::fmt((a * 1000.0).round() / 1000.0));
    }
    run.typed.push(if e.polar { '<' } else { ',' });
}

/// What Enter sends for the boxes: a full entry, measured from the last point (`@`) when
/// relative, the open box filled from the cursor.
pub(crate) fn dyn_submit(s: &Settings, prompt: &Prompt, typed: &str, cursor: Vec2) -> String {
    let typed = typed.trim();
    if let Some(rest) = typed.strip_prefix('#') {
        return rest.to_string();
    }
    let Some(e) = dyn_entry(s, prompt, typed) else { return typed.to_string() };
    if !e.first_locked {
        // A plain number: a value, or direct distance entry toward the cursor.
        return typed.to_string();
    }
    let (_, live_second) = e.live(cursor);
    let second = if e.second.is_empty() { geo::fmt((live_second * 1e6).round() / 1e6) } else { e.second.clone() };
    format!("{}{}{}{}", if e.relative { "@" } else { "" }, e.first, if e.polar { '<' } else { ',' }, second)
}

pub fn click(app: &mut SolveApp, proj: &Proj, pos: Pos2) {
    hover(app, proj, pos);
    let Some(p) = app.cad.cursor else { return };
    let prompt = app.cad.run.as_ref().map(|r| r.m.prompt(app)).unwrap_or_default();
    // A click while a value is typed: the value wins (as Enter), when the prompt takes one.
    if app.cad.run.as_ref().is_some_and(|r| !r.typed.trim().is_empty()) && prompt.accept.number {
        enter(app);
        return;
    }
    if let Some(r) = app.cad.run.as_mut() {
        r.typed.clear();
    }
    let p = if prompt.accept.select { app.cad.raw.unwrap_or(p) } else { p };
    feed(app, Input::Point { p, curve: app.cad.hover_curve.clone() });
}

// ---- Keys and the prompt bar ----------------------------------------------------------------

/// Once a frame: typed values, Enter / Space, the snap switches (F3, F8, F9), a keyword clicked in
/// the prompt bar, and the bar itself.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if let Some(i) = app.cad.pending.take() {
        feed(app, i);
    }
    // A tool whose sketch closed (finished, undone) or that another tool replaced ends.
    if app.cad.run.is_some() && (app.session.active_sketch.is_none() || !app.tool.as_ref().is_some_and(|t| t.kind == crate::tools::Kind::Cad)) {
        app.cad.run = None;
    }
    if app.session.active_sketch.is_none() {
        return;
    }
    let free = ctx.memory(|m| m.focused().is_none());
    if free {
        let (f3, f8, f9, f10, f12) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::F3),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F8),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F9),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F10),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F12),
            )
        });
        let s = &mut app.cad.settings;
        s.osnap ^= f3;
        s.ortho ^= f8;
        s.grid_snap ^= f9;
        s.polar ^= f10;
        s.dyn_input ^= f12;
        // Ortho and polar tracking exclude each other, as in CADCraft.
        if f8 && s.ortho {
            s.polar = false;
        }
        if f10 && s.polar {
            s.ortho = false;
        }
        if app.cad.run.is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
            dyn_tab(app);
        }
    }
    if free && let Some(run) = app.cad.run.as_mut() {
        let mut submit = false;
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                egui::Event::Text(t) if t.chars().all(|c| c.is_ascii_digit() || ".,-+@<*/#".contains(c)) => {
                    run.typed.push_str(t);
                    false
                }
                egui::Event::Text(t) if t == " " => {
                    submit = true;
                    false
                }
                egui::Event::Key { key: egui::Key::Backspace, pressed: true, modifiers, .. } if modifiers.is_none() && !run.typed.is_empty() => {
                    run.typed.pop();
                    false
                }
                egui::Event::Key { key: egui::Key::Enter | egui::Key::Space, pressed, modifiers, .. } if modifiers.is_none() => {
                    if *pressed {
                        submit = true;
                    }
                    false
                }
                _ => true,
            });
        });
        if submit {
            enter(app);
        }
    } else if free && app.tool.is_none() && app.dialog.is_none() {
        // Enter on no tool repeats the last one, as at CADCraft's command line.
        let again = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        if again && let Some(last) = app.cad.last {
            app.start(last);
        }
    }
    bar(app, ctx);
    if app.cad.run.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// The prompt bar under the view: the tool's prompt and options, what is typed, and the snap
/// switches with the grid's step.
fn bar(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(rect) = app.viewport.rect else { return };
    let t = crate::theme::Tokens::get();
    let prompt = app.cad.run.as_ref().map(|r| (r.m.prompt(app), r.typed.clone(), r.error.clone(), r.cmd));
    let mut clicked: Option<Input> = None;
    egui::Area::new(egui::Id::new("cad_prompt_bar"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(rect.left() + 12.0, rect.bottom() - 112.0))
        .show(ctx, |ui| {
            egui::Frame::new().fill(t.panel).stroke(Stroke::new(1.0, t.border)).corner_radius(6.0).inner_margin(egui::Margin::same(6)).show(
                ui,
                |ui| {
                    ui.set_max_width((rect.width() - 24.0).max(200.0));
                    if let Some((p, typed, error, cmd)) = &prompt {
                        ui.horizontal_wrapped(|ui| {
                            let label = solidcraft_engine::find_command(cmd).map_or("", |c| c.label);
                            ui.label(egui::RichText::new(label.to_uppercase()).strong().color(t.accent));
                            let mut msg = p.message.clone();
                            if !p.keywords.is_empty() {
                                msg.push_str(" or");
                            }
                            ui.label(egui::RichText::new(msg).color(t.text));
                            for k in &p.keywords {
                                if ui.small_button(*k).clicked() {
                                    clicked = Some(Input::Keyword(k));
                                }
                            }
                            if let Some(d) = &p.default {
                                ui.label(egui::RichText::new(format!("<{d}>")).color(t.text_dim));
                            }
                            ui.label(":");
                            let shown = if typed.is_empty() { "…".to_string() } else { typed.clone() };
                            ui.label(egui::RichText::new(shown).monospace().strong().color(t.text));
                        });
                        if let Some(e) = error {
                            ui.label(egui::RichText::new(e).color(t.error));
                        }
                        ui.separator();
                    }
                    ui.horizontal(|ui| {
                        let s = &mut app.cad.settings;
                        ui.toggle_value(&mut s.osnap, "Object snap (F3)");
                        ui.toggle_value(&mut s.nearest, "Nearest");
                        ui.toggle_value(&mut s.grid_snap, "Grid snap (F9)");
                        if ui.toggle_value(&mut s.ortho, "Ortho (F8)").changed() && s.ortho {
                            s.polar = false;
                        }
                        if ui.toggle_value(&mut s.polar, "Polar (F10)").changed() && s.polar {
                            s.ortho = false;
                        }
                        ui.toggle_value(&mut s.dyn_input, "Dynamic input (F12)");
                        if ui.button("Drafting Settings…").clicked() {
                            s.dialog = true;
                        }
                        ui.label("Grid step");
                        let r = ui.add(egui::TextEdit::singleline(&mut s.step_text).desired_width(54.0));
                        if r.changed()
                            && let Some(v) = geo::number(&s.step_text).filter(|v| *v > 0.0)
                        {
                            s.grid_step = v;
                        }
                        ui.label("mm");
                    });
                },
            );
        });
    if let Some(i) = clicked {
        app.cad.pending = Some(i);
    }
}

/// The Drafting Settings window: snap and grid, polar tracking, object snap modes, dynamic input.
pub fn drafting_settings(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.cad.settings.dialog {
        return;
    }
    let mut open = true;
    let tab_id = egui::Id::new("cad_drafting_tab");
    let mut tab: usize = ctx.data(|d| d.get_temp(tab_id)).unwrap_or(2);
    egui::Window::new("Drafting Settings").open(&mut open).collapsible(false).resizable(false).default_width(380.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (k, name) in ["Snap and Grid", "Polar Tracking", "Object Snap", "Dynamic Input"].iter().enumerate() {
                ui.selectable_value(&mut tab, k, *name);
            }
        });
        ui.separator();
        let s = &mut app.cad.settings;
        match tab {
            0 => {
                ui.checkbox(&mut s.grid_snap, "Snap On (F9)");
                ui.horizontal(|ui| {
                    ui.label("Snap and grid spacing");
                    if ui.add(egui::TextEdit::singleline(&mut s.step_text).desired_width(70.0)).changed()
                        && let Some(v) = geo::number(&s.step_text).filter(|v| *v > 0.0)
                    {
                        s.grid_step = v;
                    }
                    ui.label("mm");
                });
                ui.checkbox(&mut s.ortho, "Ortho mode (F8)");
            }
            1 => {
                if ui.checkbox(&mut s.polar, "Polar Tracking On (F10)").changed() && s.polar {
                    s.ortho = false;
                }
                ui.horizontal(|ui| {
                    ui.label("Increment angle");
                    egui::ComboBox::from_id_salt("cad_polar_step").selected_text(format!("{}°", geo::fmt(s.polar_step))).show_ui(ui, |ui| {
                        for a in [90.0, 60.0, 45.0, 30.0, 22.5, 18.0, 15.0, 10.0, 5.0] {
                            ui.selectable_value(&mut s.polar_step, a, format!("{}°", geo::fmt(a)));
                        }
                    });
                });
                ui.label(egui::RichText::new("The cursor follows rays from the last point at multiples of the angle.").small());
            }
            2 => {
                ui.checkbox(&mut s.osnap, "Object Snap On (F3)");
                ui.add_space(4.0);
                egui::Grid::new("cad_osnap_modes").num_columns(2).show(ui, |ui| {
                    for (k, (kind, name)) in snap::MODES.iter().enumerate() {
                        let bit = 1u16 << kind;
                        let mut on = s.modes & bit != 0;
                        let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                        snap::paint_marker(ui.painter(), r.center(), &snap::Hit { p: Vec2::ZERO, kind: *kind }, Color32::from_rgb(0x2e, 0xb8, 0x4a));
                        if ui.checkbox(&mut on, *name).changed() {
                            s.modes = if on { s.modes | bit } else { s.modes & !bit };
                        }
                        if k % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Select All").clicked() {
                        s.modes = snap::MODES.iter().fold(0, |m, (k, _)| m | (1 << k));
                    }
                    if ui.button("Clear All").clicked() {
                        s.modes = 0;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Aperture size");
                    ui.add(egui::Slider::new(&mut s.aperture, 4.0..=30.0).suffix(" px"));
                });
            }
            _ => {
                ui.checkbox(&mut s.dyn_input, "Enable Pointer Input (F12)");
                ui.label("For second or next points, default to:");
                ui.radio_value(&mut s.dyn_polar, true, "Polar format (distance < angle)");
                ui.radio_value(&mut s.dyn_polar, false, "Cartesian format (X, Y)");
                ui.radio_value(&mut s.dyn_relative, true, "Relative coordinates (from the last point)");
                ui.radio_value(&mut s.dyn_relative, false, "Absolute coordinates");
                ui.label(
                    egui::RichText::new("Type a value, Tab to the next box, Enter to accept. # types an absolute point, @ a relative one.").small(),
                );
            }
        }
    });
    ctx.data_mut(|d| d.insert_temp(tab_id, tab));
    if !open {
        app.cad.settings.dialog = false;
    }
}

// ---- Drawing -------------------------------------------------------------------------------

/// The rubber band, the selected objects, the snap marker and the typed value at the cursor.
pub fn paint(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some((sid, _, plane)) = active(app) else { return };
    paint_selection(app, painter, proj, sid, &plane);
    let Some(run) = app.cad.run.as_ref() else { return };
    let t = crate::theme::Tokens::get();
    let to = |p: Vec2| proj.to_screen(plane.to_world(p));
    let line = |pts: &[Vec2], stroke: Stroke| {
        let s: Vec<Pos2> = pts.iter().filter_map(|p| to(*p)).collect();
        if s.len() >= 2 {
            painter.line(s, stroke);
        }
    };
    let _ = run;
    if let Some(c) = app.cad.cursor {
        for prev in run.m.preview(app, c) {
            match prev {
                Prev::Line(pts) => line(&pts, Stroke::new(1.6, t.accent)),
                Prev::Window(a, b) => {
                    let quad = [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y), a];
                    let crossing = b.x < a.x;
                    let s: Vec<Pos2> = quad.iter().filter_map(|p| to(*p)).collect();
                    if s.len() == 5 {
                        let fill = if crossing {
                            Color32::from_rgba_unmultiplied(60, 200, 90, 40)
                        } else {
                            Color32::from_rgba_unmultiplied(60, 120, 230, 40)
                        };
                        painter.add(egui::Shape::convex_polygon(s[..4].to_vec(), fill, Stroke::NONE));
                        if crossing {
                            dashed(painter, &s, Stroke::new(1.0, t.text));
                        } else {
                            painter.line(s, Stroke::new(1.0, t.text));
                        }
                    }
                }
            }
        }
        if let Some(sp) = to(c) {
            if let Some(h) = &app.cad.hit {
                snap::paint_marker(painter, sp, h, Color32::from_rgb(0x2e, 0xb8, 0x4a));
            }
            if let Some((b, a)) = app.cad.track
                && let (Some(bs), Some(far)) = (to(b), to(b + geo::polar(a) * (c - b).len().max(1e-9) * 4.0))
            {
                dashed(painter, &[bs, far], Stroke::new(1.0, Color32::from_rgb(0x2e, 0xb8, 0x4a)));
                let text = format!(
                    "Polar: {} < {}°",
                    geo::fmt((c.dist(b) * 1000.0).round() / 1000.0),
                    geo::fmt((a.to_degrees().rem_euclid(360.0) * 10.0).round() / 10.0)
                );
                tooltip_text(painter, sp + vec2(18.0, 22.0), &text);
            }
            let prompt = run.m.prompt(app);
            match dyn_entry(&app.cad.settings, &prompt, &run.typed) {
                Some(e) => dyn_boxes(painter, sp, &prompt.message, &e, c),
                None if app.cad.settings.dyn_input || !run.typed.is_empty() => echo_box(painter, sp, &prompt.message, &run.typed),
                None => {}
            }
        }
    }
}

/// Selected objects as CADCraft shows them: dashed, thicker, in the selection colour over a
/// glow; the object under the pointer at a selection prompt glows too.
fn paint_selection(app: &SolveApp, painter: &egui::Painter, proj: &Proj, sid: u64, plane: &Plane) {
    let t = crate::theme::Tokens::get();
    let mut sel: Vec<String> = app.cad.run.as_ref().map(|r| r.m.selected()).unwrap_or_default();
    sel.extend(app.session.selection.iter().filter_map(|s| match s {
        solidcraft_engine::Sel::SketchCurve { id, sketch } if sketch.is_none_or(|k| k == sid) => Some(id.clone()),
        _ => None,
    }));
    let selecting = app.cad.run.as_ref().is_some_and(|r| r.m.prompt(app).accept.select);
    let hovered = app.cad.hover_curve.clone().filter(|_| selecting);
    if sel.is_empty() && hovered.is_none() {
        return;
    }
    let colour = t.sel_edge;
    let glow = Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), 70);
    for e in ents(app) {
        let is_sel = sel.contains(&e.id);
        let is_hover = hovered.as_deref() == Some(e.id.as_str());
        if !is_sel && !is_hover {
            continue;
        }
        let pts: Vec<Pos2> = e.shape.polyline().iter().filter_map(|p| proj.to_screen(plane.to_world(*p))).collect();
        if pts.len() < 2 {
            continue;
        }
        painter.add(egui::Shape::line(pts.clone(), Stroke::new(7.0, glow)));
        if is_sel {
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(2.0, t.viewport_top)));
            dashed(painter, &pts, Stroke::new(2.4, colour));
        } else {
            painter.add(egui::Shape::line(pts, Stroke::new(2.2, colour)));
        }
    }
}

/// The tooltip frame beside the cursor (above and to the right), as CADCraft's.
fn tooltip_frame(p: &egui::Painter, at: Pos2, size: egui::Vec2) -> egui::Rect {
    let t = crate::theme::Tokens::get();
    let r = egui::Rect::from_min_size(at + vec2(18.0, -size.y - 14.0), size);
    p.rect_filled(r, 3.0, t.panel);
    p.rect_stroke(r, 3.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    r
}

fn tooltip_text(p: &egui::Painter, at: Pos2, text: &str) {
    let t = crate::theme::Tokens::get();
    let g = p.layout_no_wrap(text.to_string(), egui::FontId::proportional(11.5), t.text);
    let r = egui::Rect::from_min_size(at, g.size() + vec2(10.0, 6.0));
    p.rect_filled(r, 3.0, t.panel);
    p.rect_stroke(r, 3.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    p.galley(r.min + vec2(5.0, 3.0), g, t.text);
}

/// A value typed at a prompt that takes a number (a radius, a count), in a box after the prompt.
fn echo_box(p: &egui::Painter, at: Pos2, message: &str, typed: &str) {
    let t = crate::theme::Tokens::get();
    let label = p.layout_no_wrap(format!("{message}:"), egui::FontId::proportional(11.5), t.text_dim);
    let value = p.layout_no_wrap(typed.to_string(), egui::FontId::proportional(13.0), t.text);
    let box_size = vec2((value.size().x + 14.0).max(48.0), value.size().y + 6.0);
    let r = tooltip_frame(p, at, vec2(label.size().x + 8.0 + box_size.x + 10.0, box_size.y + 8.0));
    let ly = r.center().y - label.size().y / 2.0;
    let lw = label.size().x;
    p.galley(egui::pos2(r.left() + 6.0, ly), label, t.text_dim);
    let b = egui::Rect::from_min_size(egui::pos2(r.left() + 5.0 + lw + 8.0, r.top() + 4.0), box_size);
    p.rect_filled(b, 2.0, t.field);
    p.rect_stroke(b, 2.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    let text_at = egui::pos2(b.left() + 5.0, b.center().y - value.size().y / 2.0);
    let caret = text_at.x + value.size().x + 1.0;
    p.galley(text_at, value, t.text);
    p.line_segment([egui::pos2(caret, b.top() + 3.0), egui::pos2(caret, b.bottom() - 3.0)], Stroke::new(1.0, t.text));
}

/// The Dynamic Input boxes (CADCraft's): X and Y, or distance and angle; typed values in full
/// colour, open ones showing the cursor's live value dimmed, a locked one with a lock, the box
/// being typed in outlined in the accent colour, `@` before values measured from the last point.
fn dyn_boxes(p: &egui::Painter, at: Pos2, message: &str, e: &DynEntry, cursor: Vec2) {
    let t = crate::theme::Tokens::get();
    let font = egui::FontId::proportional(11.5);
    let (live1, live2) = e.live(cursor);
    let r3 = |v: f64| geo::fmt((v * 1000.0).round() / 1000.0);
    let cells = [
        (
            if e.polar { "" } else { "X" },
            if e.first.is_empty() { r3(live1) } else { e.first.clone() },
            !e.first.is_empty(),
            e.first_locked,
            !e.first_locked,
        ),
        (
            if e.polar { "<" } else { "Y" },
            if e.second.is_empty() { if e.polar { format!("{}°", (live2 * 10.0).round() / 10.0) } else { r3(live2) } } else { e.second.clone() },
            !e.second.is_empty(),
            false,
            e.first_locked,
        ),
    ];
    let msg = p.layout_no_wrap(message.to_string(), font.clone(), t.text_dim);
    let marker = p.layout_no_wrap(if e.relative { "@".into() } else { String::new() }, font.clone(), t.text_dim);
    let laid: Vec<_> = cells
        .into_iter()
        .map(|(label, text, typed, locked, active)| {
            let label = p.layout_no_wrap(label.to_string(), font.clone(), t.text_dim);
            let value = p.layout_no_wrap(text, font.clone(), if typed { t.text } else { t.text_dim });
            let w = (value.size().x + 10.0 + if locked { 10.0 } else { 0.0 }).max(48.0);
            (label, value, w, locked, active)
        })
        .collect();
    let (pad, gap) = (6.0, 6.0);
    let box_h = msg.size().y + 6.0;
    let mut w = pad + msg.size().x + gap + marker.size().x;
    for (label, _, bw, _, _) in &laid {
        w += gap + label.size().x + 3.0 + bw;
    }
    let r = tooltip_frame(p, at, vec2(w + pad, box_h + 2.0 * pad - 2.0));
    let cy = r.center().y;
    let mut x = r.left() + pad;
    let (mw, mh) = (msg.size().x, msg.size().y);
    p.galley(egui::pos2(x, cy - mh / 2.0), msg, t.text_dim);
    x += mw + gap;
    let kw = marker.size().x;
    p.galley(egui::pos2(x, cy - marker.size().y / 2.0), marker, t.text_dim);
    x += kw;
    for (label, value, bw, locked, active) in laid {
        x += gap;
        let lw = label.size().x;
        p.galley(egui::pos2(x, cy - label.size().y / 2.0), label, t.text_dim);
        x += lw + 3.0;
        let b = egui::Rect::from_min_size(egui::pos2(x, cy - box_h / 2.0), vec2(bw, box_h));
        p.rect_filled(b, 2.0, t.field);
        p.rect_stroke(b, 2.0, if active { Stroke::new(1.5, t.accent) } else { Stroke::new(1.0, t.border) }, egui::StrokeKind::Inside);
        let vw = value.size().x;
        p.galley(egui::pos2(b.left() + 5.0, cy - value.size().y / 2.0), value, t.text);
        if active {
            let cx = b.left() + 6.0 + vw;
            p.line_segment([egui::pos2(cx, b.top() + 3.0), egui::pos2(cx, b.bottom() - 3.0)], Stroke::new(1.0, t.text));
        }
        if locked {
            // A small padlock.
            let c = egui::pos2(b.right() - 8.0, cy + 1.0);
            p.rect_filled(egui::Rect::from_center_size(c, vec2(7.0, 5.0)), 1.0, t.accent);
            p.circle_stroke(c - vec2(0.0, 3.5), 2.2, Stroke::new(1.2, t.accent));
        }
        x += bw;
    }
}

fn dashed(painter: &egui::Painter, pts: &[Pos2], stroke: Stroke) {
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = a.distance(b);
        let n = (len / 6.0).ceil().max(1.0) as usize;
        for k in (0..n).step_by(2) {
            let t0 = k as f32 / n as f32;
            let t1 = ((k + 1) as f32 / n as f32).min(1.0);
            painter.line_segment([a.lerp(b, t0), a.lerp(b, t1)], stroke);
        }
    }
}

// ---- Making geometry ------------------------------------------------------------------------

pub fn xy(p: Vec2) -> Value {
    json!([p.x, p.y])
}

/// Run sketch command `cmd` and take away the constraints and dimensions it added: the sketch
/// tools make plain geometry.
pub fn run(app: &mut SolveApp, cmd: &str, params: Value) -> Result<Value, String> {
    let v = app.run(cmd, params).map_err(|e| crate::friendly_error(cmd, &e))?;
    let created = v.get("created").cloned().unwrap_or(Value::Null);
    let mut ids: Vec<Value> = Vec::new();
    for k in ["constraints", "dimensions"] {
        if let Some(a) = created.get(k).and_then(Value::as_array) {
            ids.extend(a.iter().cloned());
        }
    }
    if !ids.is_empty() {
        let _ = app.run("sketch.delete", json!({"entities": ids}));
    }
    Ok(v)
}

/// The curves a command's reply says it made.
pub fn made(v: &Value) -> Vec<String> {
    v.get("created")
        .and_then(|c| c.get("curves"))
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
}

/// The points a command's reply says it made.
pub fn made_points(v: &Value) -> Vec<String> {
    v.get("created")
        .and_then(|c| c.get("points"))
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
}

/// A plain line from `a` to `b`; the curve made.
pub fn add_line(app: &mut SolveApp, a: Vec2, b: Vec2) -> Result<Vec<String>, String> {
    if a.dist(b) < 1e-6 {
        return Err("Zero-length line".into());
    }
    let v = run(app, "sketch.line", json!({"points": [xy(a), xy(b)], "connect": false}))?;
    let mut out = made(&v);
    out.extend(made_points(&v));
    Ok(out)
}

/// A 3-point arc; the curve and points made.
pub fn add_arc3(app: &mut SolveApp, s: Vec2, m: Vec2, e: Vec2) -> Result<Vec<String>, String> {
    let v = run(app, "sketch.arc.three_point", json!({"start": xy(s), "end": xy(e), "through": xy(m), "connect": false}))?;
    let mut out = made(&v);
    out.extend(made_points(&v));
    Ok(out)
}

pub fn delete(app: &mut SolveApp, ids: &[String]) {
    if !ids.is_empty() {
        let _ = app.run("sketch.delete", json!({"entities": ids}));
    }
}

/// The selection the tools work on: the sketch curves picked before the tool started.
pub fn preselected(app: &SolveApp) -> Vec<String> {
    app.session
        .selection
        .iter()
        .filter_map(|s| match s {
            solidcraft_engine::Sel::SketchCurve { id, sketch: None } => Some(id.clone()),
            solidcraft_engine::Sel::SketchCurve { id, sketch: Some(k) } if Some(*k) == app.session.active_sketch => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Object selection, as CADCraft's "Select objects:": a click on a curve adds it (again: takes it
/// away), a click on nothing starts a window (left to right: what is inside; right to left: what
/// it crosses too), Enter ends.
#[derive(Default, Clone)]
pub struct Picker {
    pub objs: Vec<String>,
    corner: Option<Vec2>,
}

impl Picker {
    pub fn prompt(&self) -> Prompt {
        match self.corner {
            Some(_) => Prompt::new("Specify opposite corner", Accept::SELECT),
            None => Prompt::new(format!("Select objects ({} selected), Enter when done", self.objs.len()), Accept::SELECT),
        }
    }
    /// Feed an input; true once the selection is done (Enter with something picked).
    pub fn input(&mut self, app: &SolveApp, i: &Input) -> Result<bool, String> {
        match i {
            Input::Point { p, curve } => {
                if let Some(a) = self.corner.take() {
                    let crossing = p.x < a.x;
                    let (lo, hi) = (Vec2::new(a.x.min(p.x), a.y.min(p.y)), Vec2::new(a.x.max(p.x), a.y.max(p.y)));
                    let inside = |q: &Vec2| q.x >= lo.x && q.x <= hi.x && q.y >= lo.y && q.y <= hi.y;
                    for e in ents(app) {
                        let pts = e.shape.polyline();
                        let take = if crossing {
                            pts.iter().any(inside)
                                || pts.windows(2).any(|w| box_edges(lo, hi).iter().any(|(c, d)| geo::seg_cross(w[0], w[1], *c, *d).is_some()))
                        } else {
                            pts.iter().all(inside)
                        };
                        if take && !self.objs.contains(&e.id) {
                            self.objs.push(e.id);
                        }
                    }
                } else if let Some(c) = curve {
                    if let Some(k) = self.objs.iter().position(|o| o == c) {
                        self.objs.remove(k);
                    } else {
                        self.objs.push(c.clone());
                    }
                } else {
                    self.corner = Some(*p);
                }
                Ok(false)
            }
            Input::Enter => {
                self.corner = None;
                if self.objs.is_empty() { Err("Select objects first".into()) } else { Ok(true) }
            }
            _ => Ok(false),
        }
    }
    pub fn preview(&self, cursor: Vec2) -> Vec<Prev> {
        self.corner.map(|a| vec![Prev::Window(a, cursor)]).unwrap_or_default()
    }
}

fn box_edges(lo: Vec2, hi: Vec2) -> [(Vec2, Vec2); 4] {
    let (a, b, c, d) = (lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y));
    [(a, b), (b, c), (c, d), (d, a)]
}

#[cfg(test)]
mod tests;
