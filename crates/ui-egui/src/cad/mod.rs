//! The sketch's drawing and editing tools, after CADCraft's: each tool is a prompt machine
//! (CADCraft's `Interactive`) asking for points, values, options and objects in turn. Points come
//! from clicks, snapped to the sketch's and the model's geometry (`snap`) or to the grid; values
//! are typed (shown at the cursor; Enter or Space takes them, a right-click is Enter); options are
//! the buttons of the prompt bar at the bottom of the view. The geometry is made with the
//! sketch's own commands, without constraints: the sketch is plain geometry, as a drawing is.

pub mod geo;
mod draw;
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
    Point { p: Vec2, curve: Option<String> },
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

/// Running settings of the sketch tools (the prompt bar's switches).
#[derive(Clone, Debug)]
pub struct Settings {
    pub osnap: bool,
    pub grid_snap: bool,
    pub ortho: bool,
    pub nearest: bool,
    /// The grid's step for Grid Snap (mm).
    pub grid_step: f64,
    pub step_text: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { osnap: true, grid_snap: false, ortho: false, nearest: false, grid_step: 1.0, step_text: "1".into() }
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
    let mut p = raw;
    // Selecting picks objects: the cursor is not snapped then.
    if !prompt.accept.select {
        if s.osnap && !crate::drag_snap::suppressed() {
            let own = geo::ents(&sk);
            let foreign = snap::foreign(app, sid, &plane);
            let aperture = 10.0 * snap::mm_per_px(&plane, proj, raw);
            hit = snap::find(&own, &foreign, raw, aperture, prompt.base, s.nearest);
        }
        if let Some(h) = hit {
            p = h.p;
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
    app.cad.raw = Some(raw);
    app.cad.cursor = Some(p);
    app.cad.hit = hit;
    app.cad.hover_curve = curve;
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
        let (f3, f8, f9) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::F3),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F8),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F9),
            )
        });
        let s = &mut app.cad.settings;
        s.osnap ^= f3;
        s.ortho ^= f8;
        s.grid_snap ^= f9;
    }
    if free && let Some(run) = app.cad.run.as_mut() {
        let mut submit = false;
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                egui::Event::Text(t) if t.chars().all(|c| c.is_ascii_digit() || ".,-+@<*/".contains(c)) => {
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
                        ui.toggle_value(&mut s.ortho, "Ortho (F8)");
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

// ---- Drawing -------------------------------------------------------------------------------

/// The rubber band, the selected objects, the snap marker and the typed value at the cursor.
pub fn paint(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some((_, _, plane)) = active(app) else { return };
    let Some(run) = app.cad.run.as_ref() else { return };
    let t = crate::theme::Tokens::get();
    let to = |p: Vec2| proj.to_screen(plane.to_world(p));
    let line = |pts: &[Vec2], stroke: Stroke| {
        let s: Vec<Pos2> = pts.iter().filter_map(|p| to(*p)).collect();
        if s.len() >= 2 {
            painter.line(s, stroke);
        }
    };
    let sel = run.m.selected();
    if !sel.is_empty() {
        for e in ents(app).into_iter().filter(|e| sel.contains(&e.id)) {
            dashed(painter, &e.shape.polyline().iter().filter_map(|p| to(*p)).collect::<Vec<_>>(), Stroke::new(2.5, t.accent));
        }
    }
    if let Some(c) = app.cad.cursor {
        for prev in run.m.preview(app, c) {
            match prev {
                Prev::Line(pts) => line(&pts, Stroke::new(1.6, t.accent)),
                Prev::Window(a, b) => {
                    let quad = [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y), a];
                    let crossing = b.x < a.x;
                    let s: Vec<Pos2> = quad.iter().filter_map(|p| to(*p)).collect();
                    if s.len() == 5 {
                        let fill = if crossing { Color32::from_rgba_unmultiplied(60, 200, 90, 40) } else { Color32::from_rgba_unmultiplied(60, 120, 230, 40) };
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
            if !run.typed.is_empty() {
                let galley = painter.layout_no_wrap(run.typed.clone(), egui::FontId::monospace(13.0), t.text);
                let r = egui::Rect::from_min_size(sp + vec2(16.0, -28.0), galley.size() + vec2(8.0, 4.0));
                painter.rect(r, 3.0, t.panel, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                painter.galley(r.min + vec2(4.0, 2.0), galley, t.text);
            }
        }
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
    v.get("created").and_then(|c| c.get("curves")).and_then(Value::as_array).map_or_else(Vec::new, |a| {
        a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()
    })
}

/// The points a command's reply says it made.
pub fn made_points(v: &Value) -> Vec<String> {
    v.get("created").and_then(|c| c.get("points")).and_then(Value::as_array).map_or_else(Vec::new, |a| {
        a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()
    })
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
                            pts.iter().any(inside) || pts.windows(2).any(|w| box_edges(lo, hi).iter().any(|(c, d)| geo::seg_cross(w[0], w[1], *c, *d).is_some()))
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
