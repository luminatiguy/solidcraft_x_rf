//! The MODIFY tab's Move, Copy, Rotate and Mirror in the design: the sketch tools' prompts,
//! snaps, polar tracking and value boxes, on the work plane, applied to whole objects (bodies).
//! The points are picked on the work plane; snaps find the model's corners, edges' middles and
//! circles' centres seen on it.

use serde_json::json;
use solidcore_record::{Plane, Vec2};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

use super::{Accept, Input, Machine, Prev, Prompt, Res, Step};
use crate::SolveApp;

type V3 = solidcore_record::Vec3;

/// Which tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Move,
    Copy,
    Rotate,
    Mirror,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Select,
    Base,
    /// The base point: on the work plane, and in the world (a snapped model corner may lie off
    /// the plane).
    Second(Vec2, V3),
    /// Mirror: erase the source objects?
    Erase(V3, V3),
}

pub struct ObjectTool {
    op: Op,
    stage: Stage,
    bodies: Vec<String>,
    /// Reference planes picked with the bodies: they move, turn and mirror too.
    planes: Vec<String>,
    /// Copy: copies made so far.
    copies: usize,
}

impl ObjectTool {
    pub fn new(op: Op) -> ObjectTool {
        ObjectTool { op, stage: Stage::Select, bodies: Vec::new(), planes: Vec::new(), copies: 0 }
    }

    fn is_empty(&self) -> bool {
        self.bodies.is_empty() && self.planes.is_empty()
    }

    fn select_all(&self, app: &mut SolveApp) {
        let items: Vec<Sel> =
            self.bodies.iter().map(|b| Sel::Body { name: b.clone() }).chain(self.planes.iter().map(|p| Sel::Plane { name: p.clone() })).collect();
        let _ = app.session.execute("select.set", &json!({ "items": items }));
    }
}

/// The bodies selected in the view (a face or an edge stands for its body).
pub fn selected_bodies(app: &SolveApp) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in &app.session.selection {
        let name = match s {
            Sel::Body { name } | Sel::Face { body: name, .. } | Sel::Edge { body: name, .. } => name.clone(),
            _ => continue,
        };
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// The reference planes selected in the view.
pub fn selected_planes(app: &SolveApp) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in &app.session.selection {
        if let Sel::Plane { name } = s
            && crate::reference_planes::is_reference(name)
            && !out.contains(name)
        {
            out.push(name.clone());
        }
    }
    out
}

/// The work plane, where the points are picked: the one set with Set Work Plane; with none set,
/// in a two-axis view (Top, Front, Right…) the origin plane the view looks at, else XY.
pub fn plane(app: &SolveApp) -> Plane {
    if app.forms.work_plane.is_none() && app.cam.aligned_standard_view(1e-4).is_some_and(|v| v != solidcraft_engine::render::StandardView::Iso) {
        let n = app.cam.back();
        let name = if n.z.abs() >= n.x.abs() && n.z.abs() >= n.y.abs() {
            "XY"
        } else if n.y.abs() >= n.x.abs() {
            "XZ"
        } else {
            "YZ"
        };
        if let Some(pl) = crate::viewport::named_plane(app, name) {
            return pl;
        }
    }
    crate::forms::work_plane_world(app).unwrap_or(Plane::XY)
}

/// The work plane's name for the prompts.
pub fn plane_name(app: &SolveApp) -> String {
    match app.forms.work_plane.as_ref() {
        Some(Sel::Plane { name }) => name.clone(),
        Some(Sel::Face { .. }) => "face".to_string(),
        _ => {
            let pl = app.cad.plane.unwrap_or_else(|| plane(app));
            let n = pl.normal();
            if n.z.abs() > 0.999 {
                "XY".into()
            } else if n.y.abs() > 0.999 {
                "XZ".into()
            } else if n.x.abs() > 0.999 {
                "YZ".into()
            } else {
                "work".into()
            }
        }
    }
}

/// The tool word of a body's name, for its copy's name: `Extrusion 1` → `Extrusion`.
fn tool_word(body: &str) -> String {
    let base = body.split(" (").next().unwrap_or(body).trim();
    match base.rsplit_once(' ') {
        Some((word, n)) if n.chars().all(|c| c.is_ascii_digit()) && !word.is_empty() => word.to_string(),
        _ => base.to_string(),
    }
}

fn arr(v: V3) -> serde_json::Value {
    json!([v.x, v.y, v.z])
}

/// `v` turned by `angle` (radians) about the unit axis `k` (Rodrigues).
fn rotated(v: V3, k: V3, angle: f64) -> V3 {
    let (s, c) = angle.sin_cos();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

/// `p` mirrored in the plane through `o` with unit normal `n`.
fn reflected(p: V3, o: V3, n: V3) -> V3 {
    p - n * (2.0 * (p - o).dot(n))
}

/// The point in the world the input stands for: the snapped model point when the input is the
/// cursor, else the point on the work plane.
fn world_of(app: &SolveApp, p: Vec2) -> V3 {
    match (app.cad.world, app.cad.cursor) {
        (Some(w), Some(c)) if c.dist(p) < 1e-9 => w,
        _ => active_plane(app).to_world(p),
    }
}

fn active_plane(app: &SolveApp) -> Plane {
    app.cad.plane.unwrap_or_else(|| plane(app))
}

impl ObjectTool {
    /// Each picked reference plane moved by `f`: its three points through it, the plane defined
    /// again in place (sketches on it follow), or a new plane for a copy.
    fn move_planes(&self, app: &mut SolveApp, copy: bool, f: &dyn Fn(V3) -> V3) -> Result<(), String> {
        let planes = solidcraft_engine::view::construction_planes(&app.session);
        for name in &self.planes {
            let Some((id, _, pl)) = planes.iter().find(|(_, n, _)| n == name) else { continue };
            let pt = |v: V3| json!({ "point": arr(f(v)) });
            let params = json!({"a": pt(pl.origin), "b": pt(pl.origin + pl.x), "c": pt(pl.origin + pl.y)});
            if copy {
                let count = planes.iter().filter(|(_, n, _)| crate::reference_planes::is_reference(n)).count() + 1 + self.copies;
                let mut p = params;
                if let Some(m) = p.as_object_mut() {
                    m.insert("name".into(), json!(format!("{} {count}", crate::reference_planes::PREFIX)));
                }
                app.run("construct.plane.three_points", p).map_err(|e| e.to_string())?;
            } else {
                app.run("timeline.redefine", json!({"feature": id, "command": "construct.plane.three_points", "params": params}))
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Each body moved as a form (its sketches onto new planes of their own, so it is edited
    /// where it now is; a copy a new form named after the tool); the rest (a box, an import) by
    /// `fallback`, a command taking `bodies`.
    fn forms_then(
        &self,
        app: &mut SolveApp,
        motion: (&str, serde_json::Value),
        copy: bool,
        fallback: Option<(&str, serde_json::Value)>,
    ) -> Result<(), String> {
        let mut rest = Vec::new();
        for body in &self.bodies {
            let mut p = json!({"bodies": [body], "copy": copy});
            if let Some(m) = p.as_object_mut() {
                m.insert(motion.0.to_string(), motion.1.clone());
                if copy {
                    m.insert("names".into(), json!([crate::forms::next_object_name(app, &tool_word(body))]));
                }
            }
            if app.run("form.transform", p).is_err() {
                rest.push(body.clone());
            }
        }
        if let (false, Some((cmd, params))) = (rest.is_empty(), fallback) {
            let mut p = params;
            if let Some(m) = p.as_object_mut() {
                m.insert("bodies".into(), json!(rest));
            }
            app.run(cmd, p).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn run_move(&mut self, app: &mut SolveApp, from: V3, to: V3, copy: bool) -> Result<(), String> {
        let d = to - from;
        self.forms_then(app, ("translate", arr(d)), copy, Some(("solid.move", json!({"translate": arr(d), "copy": copy}))))?;
        self.move_planes(app, copy, &|v| v + d)
    }

    fn run_rotate(&mut self, app: &mut SolveApp, base: V3, degrees: f64) -> Result<(), String> {
        let k = active_plane(app).normal();
        // Turned about the axis through the base point: R·p + (b − R·b).
        let a = degrees.to_radians();
        let t = base - rotated(base, k, a);
        self.forms_then(
            app,
            ("rotate", json!({"origin": arr(base), "axis": arr(k), "angle": degrees})),
            false,
            Some(("solid.move", json!({"axis": arr(k), "angle": format!("{degrees} deg"), "translate": arr(t)}))),
        )?;
        self.move_planes(app, false, &|v| base + rotated(v - base, k, a))
    }

    fn run_mirror(&mut self, app: &mut SolveApp, wa: V3, wb: V3, erase: bool) -> Result<(), String> {
        let normal = active_plane(app).normal().cross(wb - wa);
        let Some(n) = normal.normalized() else {
            return Err("the mirror line has no length on the work plane".into());
        };
        // A form mirrored is a new form (or, erasing the source, the form itself mirrored).
        let mut rest = ObjectTool { op: self.op, stage: self.stage, bodies: Vec::new(), planes: Vec::new(), copies: 0 };
        for body in &self.bodies {
            let mut p = json!({"bodies": [body], "mirror": {"origin": arr(wa), "normal": arr(normal)}, "copy": !erase});
            if !erase && let Some(m) = p.as_object_mut() {
                m.insert("names".into(), json!([crate::forms::next_object_name(app, &tool_word(body))]));
            }
            if app.run("form.transform", p).is_err() {
                rest.bodies.push(body.clone());
            }
        }
        if !rest.bodies.is_empty() {
            app.run("solid.mirror", json!({"bodies": rest.bodies, "plane": {"origin": arr(wa), "normal": arr(normal)}}))
                .map_err(|e| e.to_string())?;
            if erase {
                app.run("solid.remove", json!({"bodies": rest.bodies})).map_err(|e| e.to_string())?;
            }
        }
        // A reference plane mirrored is the same plane moved (keeping it is a copy).
        self.move_planes(app, !erase, &|v| reflected(v, wa, n))
    }
}

impl Machine for ObjectTool {
    fn prompt(&self, app: &SolveApp) -> Prompt {
        let on = plane_name(app);
        match self.stage {
            Stage::Select => Prompt::new("Select objects (bodies, reference planes), then Enter", Accept::SELECT),
            Stage::Base => match self.op {
                Op::Mirror => Prompt::new(format!("Specify first point of mirror line (on the {on} plane)"), Accept::POINT),
                Op::Rotate => Prompt::new(format!("Specify base point (turns about the {on} plane's normal)"), Accept::POINT),
                _ => Prompt::new(format!("Specify base point (on the {on} plane or a snapped point)"), Accept::POINT),
            },
            Stage::Second(b, _) => match self.op {
                Op::Move | Op::Copy => Prompt::new("Specify second point", Accept::POINT).base(Some(b)),
                Op::Rotate => Prompt::new("Specify rotation angle", Accept::POINT_OR_NUMBER).base(Some(b)),
                Op::Mirror => Prompt::new("Specify second point of mirror line", Accept::POINT).base(Some(b)),
            },
            Stage::Erase(..) => Prompt::new("Erase source objects?", Accept::NUMBER).kw(&["Yes", "No"]).default("No"),
        }
    }

    fn begin(&mut self, app: &mut SolveApp) -> Res {
        self.bodies = selected_bodies(app);
        self.planes = selected_planes(app);
        if !self.is_empty() {
            self.stage = Stage::Base;
        }
        let on = plane_name(app);
        app.set_status(format!("Points are picked on the {on} plane (Set Work Plane, or a Top/Front/Right view, changes it); snaps find the model's corners, middles and centres"), false);
        Ok(Step::Continue)
    }

    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match (self.stage, i) {
            (Stage::Select, Input::Object(name)) => {
                let list = if crate::reference_planes::is_reference(&name) && app.session.doc.find_feature(&name).is_some() {
                    &mut self.planes
                } else {
                    &mut self.bodies
                };
                if let Some(at) = list.iter().position(|b| *b == name) {
                    list.remove(at);
                } else {
                    list.push(name);
                }
                self.select_all(app);
                Ok(Step::Continue)
            }
            (Stage::Select, Input::Enter) => {
                if self.is_empty() {
                    return Err("select an object first (click it), then Enter".into());
                }
                self.stage = Stage::Base;
                Ok(Step::Continue)
            }
            (Stage::Select, _) => Ok(Step::Continue),
            (Stage::Base, Input::Point { p, .. }) => {
                self.stage = Stage::Second(p, world_of(app, p));
                Ok(Step::Continue)
            }
            (Stage::Base, Input::Enter) => Ok(Step::Done),
            (Stage::Second(b, wb), Input::Point { p, .. }) => {
                let wp = world_of(app, p);
                match self.op {
                    Op::Move => {
                        self.run_move(app, wb, wp, false)?;
                        Ok(Step::Done)
                    }
                    Op::Copy => {
                        self.run_move(app, wb, wp, true)?;
                        self.copies += 1;
                        Ok(Step::Continue)
                    }
                    Op::Rotate => {
                        let d = p - b;
                        self.run_rotate(app, wb, d.y.atan2(d.x).to_degrees())?;
                        Ok(Step::Done)
                    }
                    Op::Mirror => {
                        if p.dist(b) < 1e-9 {
                            return Err("pick a second point away from the first".into());
                        }
                        self.stage = Stage::Erase(wb, wp);
                        Ok(Step::Continue)
                    }
                }
            }
            (Stage::Second(_, wb), Input::Number(v)) => match self.op {
                Op::Rotate => {
                    self.run_rotate(app, wb, v)?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
            (Stage::Second(..), Input::Enter) => Ok(Step::Done),
            (Stage::Erase(a, b), Input::Keyword(k)) => {
                self.run_mirror(app, a, b, k == "Yes")?;
                Ok(Step::Done)
            }
            (Stage::Erase(a, b), Input::Enter) => {
                self.run_mirror(app, a, b, false)?;
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }

    fn preview(&self, app: &SolveApp, cursor: Vec2) -> Vec<Prev> {
        match self.stage {
            Stage::Second(_, wb) => vec![Prev::World(vec![wb, world_of(app, cursor)])],
            Stage::Erase(a, b) => vec![Prev::World(vec![a, b])],
            _ => Vec::new(),
        }
    }
}

/// The tool for an `obj.*` command.
pub fn make(id: &str) -> Option<Box<dyn Machine>> {
    let op = match id {
        "obj.move" => Op::Move,
        "obj.copy" => Op::Copy,
        "obj.rotate" => Op::Rotate,
        "obj.mirror" => Op::Mirror,
        _ => return None,
    };
    Some(Box::new(ObjectTool::new(op)))
}
