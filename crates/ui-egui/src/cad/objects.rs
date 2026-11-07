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
    Second(Vec2),
    /// Mirror: erase the source objects?
    Erase(Vec2, Vec2),
}

pub struct ObjectTool {
    op: Op,
    stage: Stage,
    bodies: Vec<String>,
    /// Copy: copies made so far.
    copies: usize,
}

impl ObjectTool {
    pub fn new(op: Op) -> ObjectTool {
        ObjectTool { op, stage: Stage::Select, bodies: Vec::new(), copies: 0 }
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

/// The work plane, where the points are picked.
pub fn plane(app: &SolveApp) -> Plane {
    crate::forms::work_plane_world(app).unwrap_or(Plane::XY)
}

fn world(app: &SolveApp, p: Vec2) -> V3 {
    plane(app).to_world(p)
}

fn arr(v: V3) -> serde_json::Value {
    json!([v.x, v.y, v.z])
}

/// `v` turned by `angle` (radians) about the unit axis `k` (Rodrigues).
fn rotated(v: V3, k: V3, angle: f64) -> V3 {
    let (s, c) = angle.sin_cos();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

impl ObjectTool {
    fn run_move(&mut self, app: &mut SolveApp, from: Vec2, to: Vec2, copy: bool) -> Result<(), String> {
        let d = world(app, to) - world(app, from);
        app.run("solid.move", json!({"bodies": self.bodies, "translate": arr(d), "copy": copy})).map(|_| ()).map_err(|e| e.to_string())
    }

    fn run_rotate(&mut self, app: &mut SolveApp, base: Vec2, degrees: f64) -> Result<(), String> {
        let pl = plane(app);
        let k = pl.normal();
        let b = pl.to_world(base);
        // Turned about the axis through the base point: R·p + (b − R·b).
        let t = b - rotated(b, k, degrees.to_radians());
        app.run("solid.move", json!({"bodies": self.bodies, "axis": arr(k), "angle": format!("{degrees} deg"), "translate": arr(t)}))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn run_mirror(&mut self, app: &mut SolveApp, a: Vec2, b: Vec2, erase: bool) -> Result<(), String> {
        let pl = plane(app);
        let (wa, wb) = (pl.to_world(a), pl.to_world(b));
        let normal = pl.normal().cross(wb - wa);
        if normal.len() < 1e-12 {
            return Err("the mirror line has no length".into());
        }
        app.run("solid.mirror", json!({"bodies": self.bodies, "plane": {"origin": arr(wa), "normal": arr(normal)}})).map_err(|e| e.to_string())?;
        if erase {
            app.run("solid.remove", json!({"bodies": self.bodies})).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

impl Machine for ObjectTool {
    fn prompt(&self, _app: &SolveApp) -> Prompt {
        match self.stage {
            Stage::Select => Prompt::new("Select objects", Accept::SELECT),
            Stage::Base => match self.op {
                Op::Mirror => Prompt::new("Specify first point of mirror line", Accept::POINT),
                _ => Prompt::new("Specify base point", Accept::POINT),
            },
            Stage::Second(b) => match self.op {
                Op::Move | Op::Copy => Prompt::new("Specify second point", Accept::POINT).base(Some(b)),
                Op::Rotate => Prompt::new("Specify rotation angle", Accept::POINT_OR_NUMBER).base(Some(b)),
                Op::Mirror => Prompt::new("Specify second point of mirror line", Accept::POINT).base(Some(b)),
            },
            Stage::Erase(..) => Prompt::new("Erase source objects?", Accept::NUMBER).kw(&["Yes", "No"]).default("No"),
        }
    }

    fn begin(&mut self, app: &mut SolveApp) -> Res {
        self.bodies = selected_bodies(app);
        if !self.bodies.is_empty() {
            self.stage = Stage::Base;
        }
        Ok(Step::Continue)
    }

    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match (self.stage, i) {
            (Stage::Select, Input::Object(name)) => {
                if let Some(at) = self.bodies.iter().position(|b| *b == name) {
                    self.bodies.remove(at);
                } else {
                    self.bodies.push(name);
                }
                let _ = app
                    .session
                    .execute("select.set", &json!({"items": self.bodies.iter().map(|b| Sel::Body { name: b.clone() }).collect::<Vec<_>>()}));
                Ok(Step::Continue)
            }
            (Stage::Select, Input::Enter) => {
                if self.bodies.is_empty() {
                    return Err("select an object first (click it), then Enter".into());
                }
                self.stage = Stage::Base;
                Ok(Step::Continue)
            }
            (Stage::Select, _) => Ok(Step::Continue),
            (Stage::Base, Input::Point { p, .. }) => {
                self.stage = Stage::Second(p);
                Ok(Step::Continue)
            }
            (Stage::Base, Input::Enter) => Ok(Step::Done),
            (Stage::Second(b), Input::Point { p, .. }) => match self.op {
                Op::Move => {
                    self.run_move(app, b, p, false)?;
                    Ok(Step::Done)
                }
                Op::Copy => {
                    self.run_move(app, b, p, true)?;
                    self.copies += 1;
                    Ok(Step::Continue)
                }
                Op::Rotate => {
                    let d = p - b;
                    self.run_rotate(app, b, d.y.atan2(d.x).to_degrees())?;
                    Ok(Step::Done)
                }
                Op::Mirror => {
                    if p.dist(b) < 1e-9 {
                        return Err("pick a second point away from the first".into());
                    }
                    self.stage = Stage::Erase(b, p);
                    Ok(Step::Continue)
                }
            },
            (Stage::Second(b), Input::Number(v)) => match self.op {
                Op::Rotate => {
                    self.run_rotate(app, b, v)?;
                    Ok(Step::Done)
                }
                _ => Ok(Step::Continue),
            },
            (Stage::Second(_), Input::Enter) => Ok(Step::Done),
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

    fn preview(&self, _app: &SolveApp, cursor: Vec2) -> Vec<Prev> {
        match self.stage {
            Stage::Second(b) => vec![Prev::Line(vec![b, cursor])],
            Stage::Erase(a, b) => vec![Prev::Line(vec![a, b])],
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
