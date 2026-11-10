//! The editing tools, after CADCraft's MODIFY commands: select objects (or take the ones picked
//! before), then the points and values each needs.

use std::cell::Cell;

use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_engine::geom::prelude::*;

use super::geo::{self, Shape, polar};
use super::{Accept, Input, Machine, Picker, Prev, Prompt, Res, Step, add_line, delete, ent, ents, made, preselected, run, xy};
use crate::SolveApp;

thread_local! {
    static FILLET_R: Cell<f64> = const { Cell::new(0.0) };
    static CHAMFER_D: Cell<(f64, f64)> = const { Cell::new((1.0, 1.0)) };
    static OFFSET_D: Cell<f64> = const { Cell::new(1.0) };
    static ARRAY: Cell<(u32, u32, f64, f64)> = const { Cell::new((4, 3, 10.0, 10.0)) };
    static POLAR: Cell<(u32, f64)> = const { Cell::new((6, 360.0)) };
}

pub fn make(id: &str) -> Option<Box<dyn Machine>> {
    use Op::*;
    Some(match id {
        "cad.move" => Box::new(SelectThen::new(Move)),
        "cad.copy" => Box::new(SelectThen::new(Copy)),
        "cad.rotate" => Box::new(SelectThen::new(Rotate)),
        "cad.scale" => Box::new(SelectThen::new(Scale)),
        "cad.mirror" => Box::new(SelectThen::new(Mirror)),
        "cad.erase" => Box::new(SelectThen::new(Erase)),
        "cad.explode" => Box::new(SelectThen::new(Explode)),
        "cad.join" => Box::new(SelectThen::new(Join)),
        "cad.arrayrect" => Box::new(SelectThen::new(ArrayRect)),
        "cad.arraypolar" => Box::new(SelectThen::new(ArrayPolar)),
        "cad.trim" => Box::new(TrimM::new(false)),
        "cad.extend" => Box::new(TrimM::new(true)),
        "cad.fillet" => Box::new(FilletM::default()),
        "cad.chamfer" => Box::new(ChamferM::default()),
        "cad.offset" => Box::new(OffsetM::default()),
        "cad.stretch" => Box::new(StretchM::default()),
        "cad.break" => Box::new(BreakM::default()),
        "cad.lengthen" => Box::new(LengthenM::default()),
        "cad.matchprop" => Box::new(MatchM::default()),
        _ => return None,
    })
}

fn bad_input() -> String {
    "Point or option keyword required.".into()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Move,
    Copy,
    Rotate,
    Scale,
    Mirror,
    Erase,
    Explode,
    Join,
    ArrayRect,
    ArrayPolar,
}

/// The selected shapes moved by `f`, for the previews.
fn moved(app: &SolveApp, objs: &[String], f: impl Fn(Vec2) -> Vec2) -> Vec<Prev> {
    ents(app).into_iter().filter(|e| objs.contains(&e.id)).map(|e| Prev::Line(e.shape.polyline().into_iter().map(&f).collect())).collect()
}

fn rotate(p: Vec2, c: Vec2, a: f64) -> Vec2 {
    let (s, co) = a.sin_cos();
    let d = p - c;
    c + Vec2::new(d.x * co - d.y * s, d.x * s + d.y * co)
}

fn mirror(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let Some(d) = (b - a).normalized() else { return p };
    let v = p - a;
    let along = d * v.dot(d);
    a + along * 2.0 - v
}

struct SelectThen {
    op: Op,
    pick: Picker,
    picking: bool,
    pts: Vec<Vec2>,
    copy: bool,
    /// Rotate / Scale Reference: asking the reference (angle or length), then the new one.
    reference: Option<Option<f64>>,
    ask_erase: bool,
    /// Array answers so far.
    stage: u8,
}

impl SelectThen {
    fn new(op: Op) -> Self {
        SelectThen { op, pick: Picker::default(), picking: true, pts: Vec::new(), copy: false, reference: None, ask_erase: false, stage: 0 }
    }
    fn objs(&self) -> Vec<String> {
        self.pick.objs.clone()
    }
    /// The selection is complete: the operations that need nothing more run now.
    fn selected_all(&mut self, app: &mut SolveApp) -> Res {
        self.picking = false;
        let objs = self.objs();
        let _ = app.session.execute("select.clear", &json!({}));
        match self.op {
            Op::Erase => {
                delete(app, &objs);
                Ok(Step::Done)
            }
            Op::Explode => {
                let r = app.run("sketch.break_link", json!({"entities": objs}));
                if r.is_err() {
                    app.set_status("Nothing to explode: sketch geometry is already lines, arcs and circles", false);
                }
                Ok(Step::Done)
            }
            Op::Join => join(app, &objs),
            _ => Ok(Step::Continue),
        }
    }
    fn transform(&mut self, app: &mut SolveApp, params: serde_json::Value) -> Result<(), String> {
        run(app, "sketch.move", params)?;
        Ok(())
    }
}

impl Machine for SelectThen {
    fn begin(&mut self, app: &mut SolveApp) -> Res {
        let pre = preselected(app);
        if pre.is_empty() {
            return Ok(Step::Continue);
        }
        self.pick.objs = pre;
        self.selected_all(app)
    }
    fn prompt(&self, _: &SolveApp) -> Prompt {
        if self.picking {
            return self.pick.prompt();
        }
        let base = self.pts.first().copied();
        if self.ask_erase {
            return Prompt::new("Erase source objects?", Accept::NUMBER).kw(&["Yes", "No"]).default("No");
        }
        if let Some(r) = self.reference {
            return match (self.op, r) {
                (Op::Rotate, None) => Prompt::new("Specify the reference angle", Accept::NUMBER).default("0"),
                (Op::Rotate, Some(_)) => Prompt::new("Specify the new angle", Accept::POINT_OR_NUMBER).base(base),
                (_, None) => Prompt::new("Specify reference length", Accept::NUMBER).default("1"),
                (_, Some(_)) => Prompt::new("Specify new length", Accept::POINT_OR_NUMBER).base(base),
            };
        }
        let (cols, rows, dc, dr) = ARRAY.with(Cell::get);
        let (items, fill) = POLAR.with(Cell::get);
        match (self.op, self.pts.len()) {
            (Op::Move | Op::Copy, 0) => Prompt::new("Specify base point", Accept::POINT),
            (Op::Move, _) => Prompt::new("Specify second point", Accept::POINT).base(base),
            (Op::Copy, _) => Prompt::new("Specify second point", Accept::POINT).kw(&["Exit"]).base(base),
            (Op::Rotate | Op::Scale, 0) => Prompt::new("Specify base point", Accept::POINT),
            (Op::Rotate, _) => Prompt::new("Specify rotation angle", Accept::POINT_OR_NUMBER).kw(&["Copy", "Reference"]).base(base).default("0"),
            (Op::Scale, _) => Prompt::new("Specify scale factor", Accept::POINT_OR_NUMBER).kw(&["Copy", "Reference"]).base(base),
            (Op::Mirror, 0) => Prompt::new("Specify first point of mirror line", Accept::POINT),
            (Op::Mirror, _) => Prompt::new("Specify second point of mirror line", Accept::POINT).base(base),
            (Op::ArrayRect, _) => match self.stage {
                0 => Prompt::new("Enter the number of columns (|||)", Accept::NUMBER).default(cols.to_string()),
                1 => Prompt::new("Enter the number of rows (---)", Accept::NUMBER).default(rows.to_string()),
                2 => Prompt::new("Specify the distance between columns (|||)", Accept::NUMBER).default(geo::fmt(dc)),
                _ => Prompt::new("Specify the distance between rows (---)", Accept::NUMBER).default(geo::fmt(dr)),
            },
            (Op::ArrayPolar, 0) => Prompt::new("Specify center point of array", Accept::POINT),
            (Op::ArrayPolar, _) => match self.stage {
                0 => Prompt::new("Enter number of items in array", Accept::NUMBER).default(items.to_string()),
                _ => Prompt::new("Specify the angle to fill (+=ccw, -=cw)", Accept::NUMBER).default(geo::fmt(fill)),
            },
            _ => Prompt::new("", Accept::POINT),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        if self.picking {
            return if self.pick.input(app, &i)? { self.selected_all(app) } else { Ok(Step::Continue) };
        }
        let objs = self.objs();
        if self.ask_erase {
            let erase = matches!(i, Input::Keyword("Yes"));
            let (a, b) = (self.pts[0], self.pts[1]);
            let axis = add_line(app, a, b)?;
            let line = axis.first().cloned().unwrap_or_default();
            let r = run(app, "sketch.mirror", json!({"entities": objs, "line": line}));
            delete(app, &axis);
            r?;
            if erase {
                delete(app, &objs);
            }
            return Ok(Step::Done);
        }
        if let Some(r) = self.reference {
            let base = self.pts.first().copied().unwrap_or(Vec2::ZERO);
            match (self.op, r, i) {
                (Op::Rotate, None, Input::Number(v)) => self.reference = Some(Some(v)),
                (Op::Rotate, None, Input::Enter) => self.reference = Some(Some(0.0)),
                (Op::Rotate, Some(r), Input::Number(v)) => return self.rotate_by(app, v - r),
                (Op::Rotate, Some(r), Input::Point { p, .. }) => return self.rotate_by(app, (p - base).angle().to_degrees() - r),
                (_, None, Input::Number(v)) if v > 0.0 => self.reference = Some(Some(v)),
                (_, None, Input::Enter) => self.reference = Some(Some(1.0)),
                (_, Some(r), Input::Number(v)) => return self.scale_by(app, v / r),
                (_, Some(r), Input::Point { p, .. }) => return self.scale_by(app, base.dist(p) / r),
                _ => return Err(bad_input()),
            }
            return Ok(Step::Continue);
        }
        match (self.op, i) {
            (Op::Rotate | Op::Scale, Input::Keyword("Copy")) => {
                self.copy = !self.copy;
                Ok(Step::Continue)
            }
            (Op::Rotate | Op::Scale, Input::Keyword("Reference")) => {
                self.reference = Some(None);
                Ok(Step::Continue)
            }
            (Op::Copy, Input::Keyword("Exit")) | (Op::Copy, Input::Enter) => Ok(Step::Done),
            (Op::ArrayRect, Input::Number(v)) | (Op::ArrayPolar, Input::Number(v)) => self.array_answer(app, Some(v)),
            (Op::ArrayRect, Input::Enter) => self.array_answer(app, None),
            (Op::ArrayPolar, Input::Enter) if !self.pts.is_empty() => self.array_answer(app, None),
            (_, Input::Point { p, .. }) if self.pts.is_empty() => {
                self.pts.push(p);
                Ok(Step::Continue)
            }
            (Op::Move | Op::Copy, Input::Point { p, .. }) => {
                let d = p - self.pts[0];
                self.transform(app, json!({"entities": objs, "translate": [d.x, d.y], "copy": self.op == Op::Copy}))?;
                Ok(if self.op == Op::Copy { Step::Continue } else { Step::Done })
            }
            (Op::Rotate, Input::Point { p, .. }) => self.rotate_by(app, (p - self.pts[0]).angle().to_degrees()),
            (Op::Rotate, Input::Number(v)) => self.rotate_by(app, v),
            (Op::Scale, Input::Point { p, .. }) => self.scale_by(app, self.pts[0].dist(p)),
            (Op::Scale, Input::Number(v)) => self.scale_by(app, v),
            (Op::Mirror, Input::Point { p, .. }) => {
                if p.dist(self.pts[0]) < 1e-9 {
                    return Err("The mirror line points coincide".into());
                }
                self.pts.push(p);
                self.ask_erase = true;
                Ok(Step::Continue)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Err(bad_input()),
        }
    }
    fn preview(&self, app: &SolveApp, c: Vec2) -> Vec<Prev> {
        if self.picking {
            return self.pick.preview(c);
        }
        let objs = self.objs();
        let Some(b) = self.pts.first().copied() else { return Vec::new() };
        let mut out = match (self.op, self.reference) {
            (Op::Move | Op::Copy, _) => moved(app, &objs, |p| p + (c - b)),
            (Op::Rotate, None) => moved(app, &objs, |p| rotate(p, b, (c - b).angle())),
            (Op::Rotate, Some(Some(r))) => moved(app, &objs, |p| rotate(p, b, (c - b).angle() - r.to_radians())),
            (Op::Scale, None) => {
                let f = b.dist(c);
                moved(app, &objs, |p| b + (p - b) * f)
            }
            (Op::Scale, Some(Some(r))) => {
                let f = b.dist(c) / r;
                moved(app, &objs, |p| b + (p - b) * f)
            }
            (Op::Mirror, _) if self.pts.len() == 1 => moved(app, &objs, |p| mirror(p, b, c)),
            (Op::ArrayPolar, _) => vec![Prev::Line(vec![b, c])],
            _ => Vec::new(),
        };
        if matches!(self.op, Op::Move | Op::Copy | Op::Rotate | Op::Scale | Op::Mirror) {
            out.push(Prev::Line(vec![b, c]));
        }
        out
    }
    fn selected(&self) -> Vec<String> {
        self.objs()
    }
}

impl SelectThen {
    fn rotate_by(&mut self, app: &mut SolveApp, deg: f64) -> Res {
        let c = self.pts[0];
        self.transform(app, json!({"entities": self.objs(), "angle": deg, "center": xy(c), "copy": self.copy}))?;
        Ok(Step::Done)
    }
    fn scale_by(&mut self, app: &mut SolveApp, f: f64) -> Res {
        if !(f > 1e-9 && f.is_finite()) {
            return Err("Value must be positive and nonzero.".into());
        }
        let base = self.pts[0];
        let mut objs = self.objs();
        if self.copy {
            let v = run(app, "sketch.move", json!({"entities": objs, "translate": [0.0, 0.0], "copy": true}))?;
            objs = made(&v);
        }
        run(app, "sketch.scale", json!({"entities": objs, "base": xy(base), "factor": f}))?;
        Ok(Step::Done)
    }
    fn array_answer(&mut self, app: &mut SolveApp, v: Option<f64>) -> Res {
        let objs = self.objs();
        if self.op == Op::ArrayRect {
            let (mut cols, mut rows, mut dc, mut dr) = ARRAY.with(Cell::get);
            let count = |v: f64| {
                if (1.0..=500.0).contains(&v) && v.fract() == 0.0 { Ok(v as u32) } else { Err("Enter a whole number from 1 to 500.".to_string()) }
            };
            match (self.stage, v) {
                (0, Some(v)) => cols = count(v)?,
                (1, Some(v)) => rows = count(v)?,
                (2, Some(v)) => dc = v,
                (3, Some(v)) => dr = v,
                _ => {}
            }
            ARRAY.with(|a| a.set((cols, rows, dc, dr)));
            self.stage += 1;
            // Distances are asked only for the directions that have more than one item.
            if self.stage == 2 && cols <= 1 {
                self.stage = 3;
            }
            if self.stage == 3 && rows <= 1 || self.stage >= 4 {
                if cols * rows <= 1 {
                    return Ok(Step::Done);
                }
                run(app, "sketch.pattern.rectangular", json!({"entities": objs, "count": cols, "spacing": dc, "count2": rows, "spacing2": dr}))?;
                return Ok(Step::Done);
            }
            Ok(Step::Continue)
        } else {
            let (mut items, mut fill) = POLAR.with(Cell::get);
            match (self.stage, v) {
                (0, Some(v)) if (2.0..=500.0).contains(&v) && v.fract() == 0.0 => items = v as u32,
                (0, Some(_)) => return Err("Enter a whole number from 2 to 500.".into()),
                (1, Some(v)) if v != 0.0 && v.abs() <= 360.0 => fill = v,
                (1, Some(_)) => return Err("The angle to fill is between -360 and 360, not 0.".into()),
                _ => {}
            }
            POLAR.with(|p| p.set((items, fill)));
            self.stage += 1;
            if self.stage < 2 {
                return Ok(Step::Continue);
            }
            let c = self.pts[0];
            run(
                app,
                "sketch.pattern.circular",
                json!({"entities": objs, "center": xy(c), "count": items, "angle": fill.abs(), "reversed": fill < 0.0}),
            )?;
            Ok(Step::Done)
        }
    }
}

/// JOIN: lines on one line, and arcs on one circle, that meet end to end become one.
fn join(app: &mut SolveApp, objs: &[String]) -> Res {
    let mut shapes: Vec<(String, Shape)> = ents(app).into_iter().filter(|e| objs.contains(&e.id)).map(|e| (e.id, e.shape)).collect();
    let mut joined = 0;
    let tol = 1e-6;
    'again: loop {
        for i in 0..shapes.len() {
            for j in 0..shapes.len() {
                if i == j {
                    continue;
                }
                let merged = match (&shapes[i].1, &shapes[j].1) {
                    (Shape::Line(a, b), Shape::Line(c, d)) => {
                        let Some(u) = (*b - *a).normalized() else { continue };
                        let on = |p: Vec2| (p - *a).cross(u).abs() < tol;
                        if !(on(*c) && on(*d)) {
                            continue;
                        }
                        let ts = [0.0, (*b - *a).dot(u), (*c - *a).dot(u), (*d - *a).dot(u)];
                        let (t0, t1) = ((*c - *a).dot(u).min((*d - *a).dot(u)), (*c - *a).dot(u).max((*d - *a).dot(u)));
                        let (s0, s1) = (0.0_f64.min(ts[1]), ts[1].max(0.0));
                        if t0 > s1 + tol || s0 > t1 + tol {
                            continue;
                        }
                        let lo = ts.iter().copied().fold(f64::MAX, f64::min);
                        let hi = ts.iter().copied().fold(f64::MIN, f64::max);
                        Some((*a + u * lo, None, *a + u * hi))
                    }
                    (Shape::Arc { c, r, a0, a1 }, Shape::Arc { c: c2, r: r2, a0: b0, a1: b1 }) if c.dist(*c2) < tol && (r - r2).abs() < tol => {
                        // j continues i counter-clockwise.
                        if (geo::norm_angle_from(*b0, *a1) - *a1).abs() < 1e-7
                            || (geo::norm_angle_from(*b0, *a1) - *a1 - std::f64::consts::TAU).abs() < 1e-7
                        {
                            let end = geo::norm_angle_from(*b1, *b0) - *b0 + *a1;
                            if end - *a0 >= std::f64::consts::TAU - 1e-7 {
                                None
                            } else {
                                let s = *c + polar(*a0) * *r;
                                let m = *c + polar((*a0 + end) * 0.5) * *r;
                                let e = *c + polar(end) * *r;
                                Some((s, Some(m), e))
                            }
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                };
                let Some((s, m, e)) = merged else { continue };
                let (ia, ib) = (shapes[i].0.clone(), shapes[j].0.clone());
                let ids = match m {
                    None => add_line(app, s, e)?,
                    Some(m) => super::add_arc3(app, s, m, e)?,
                };
                delete(app, &[ia.clone(), ib.clone()]);
                joined += 1;
                shapes.retain(|(id, _)| *id != ia && *id != ib);
                if let Some(id) = ids.first()
                    && let Some(e) = ent(app, id)
                {
                    shapes.push((e.id, e.shape));
                }
                continue 'again;
            }
        }
        break;
    }
    if joined == 0 {
        app.set_status("Nothing joined: pick lines on one line or arcs on one circle that meet end to end", false);
    } else {
        app.set_status(format!("{} objects joined", joined + 1), false);
    }
    Ok(Step::Done)
}

// ---- TRIM / EXTEND --------------------------------------------------------------------------

#[derive(Default)]
struct TrimM {
    extend: bool,
    /// The fence's first point: a click on empty space (or the Fence option) starts a line, and
    /// every curve it crosses is trimmed (extended) where it crosses.
    fence: Option<Vec2>,
    /// Fence typed: the next point starts the fence even over a curve.
    fence_next: bool,
}

/// Where the fence from `a` to `b` crosses the open sketch's curves: (curve, point), in order
/// along the fence.
fn fence_crossings(app: &SolveApp, a: Vec2, b: Vec2) -> Vec<(String, Vec2)> {
    let fence = Shape::Line(a, b);
    let mut out: Vec<(f64, String, Vec2)> = Vec::new();
    for e in ents(app).into_iter().take(10_000) {
        for x in geo::crossings(&fence, &e.shape) {
            out.push((x.dist(a), e.id.clone(), x));
        }
    }
    out.sort_by(|x, y| x.0.total_cmp(&y.0));
    out.into_iter().map(|(_, id, p)| (id, p)).collect()
}

impl TrimM {
    fn new(extend: bool) -> TrimM {
        TrimM { extend, ..Default::default() }
    }

    /// Trim (extend) every curve the fence crosses, at the crossing.
    fn run_fence(&self, app: &mut SolveApp, a: Vec2, b: Vec2) -> Result<usize, String> {
        let cmd = if self.extend { "sketch.extend" } else { "sketch.trim" };
        let crossings = fence_crossings(app, a, b);
        if crossings.is_empty() {
            return Err("The fence crosses no object".into());
        }
        let mut done = 0;
        for (id, at) in crossings.into_iter().take(500) {
            // The curve under the crossing now: an earlier trim may have split or removed it.
            let now = ents(app);
            let tol = 1e-6_f64.max(at.len() * 1e-9);
            let under = now.iter().find(|e| e.id == id && e.shape.dist(at) < 1e-4).or_else(|| now.iter().find(|e| e.shape.dist(at) < tol));
            let Some(e) = under else { continue };
            let curve = e.id.clone();
            if run(app, cmd, json!({"curve": curve, "at": xy(at)})).is_ok() {
                done += 1;
            }
        }
        if done == 0 {
            return Err(if self.extend { "Nothing to extend along the fence".into() } else { "Nothing to trim along the fence".into() });
        }
        Ok(done)
    }
}

impl Machine for TrimM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        if let Some(a) = self.fence {
            return Prompt::new("Specify next fence point", Accept::POINT).base(Some(a));
        }
        if self.fence_next {
            return Prompt::new("Specify first fence point", Accept::POINT);
        }
        let what = if self.extend { "Select object to extend (near the end)" } else { "Select object to trim" };
        Prompt::new(format!("{what}, or click empty space for a fence"), Accept::PICK).kw(&["Fence"])
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match (self.fence, i) {
            (Some(a), Input::Point { p, .. }) => {
                self.fence = None;
                self.fence_next = false;
                if p.dist(a) < 1e-9 {
                    return Err("pick a second fence point away from the first".into());
                }
                let n = self.run_fence(app, a, p)?;
                app.set_status(format!("{n} object{} {}", if n == 1 { "" } else { "s" }, if self.extend { "extended" } else { "trimmed" }), false);
                Ok(Step::Continue)
            }
            (Some(_), Input::Enter) => {
                self.fence = None;
                self.fence_next = false;
                Ok(Step::Continue)
            }
            (None, Input::Keyword("Fence")) => {
                self.fence_next = true;
                Ok(Step::Continue)
            }
            (None, Input::Point { p, curve: Some(c) }) if !self.fence_next => {
                let cmd = if self.extend { "sketch.extend" } else { "sketch.trim" };
                run(app, cmd, json!({"curve": c, "at": xy(p)}))?;
                Ok(Step::Continue)
            }
            // Empty space (or Fence typed): the fence starts here.
            (None, Input::Point { p, .. }) => {
                self.fence = Some(p);
                Ok(Step::Continue)
            }
            (None, Input::Enter) => Ok(Step::Done),
            _ => Err(bad_input()),
        }
    }
    fn preview(&self, app: &SolveApp, cursor: Vec2) -> Vec<Prev> {
        if let Some(a) = self.fence {
            let mut out = vec![Prev::Line(vec![a, cursor])];
            // Where the fence will cut: a small cross at each crossing.
            for (_, x) in fence_crossings(app, a, cursor).into_iter().take(200) {
                let d = (cursor - a).len().max(1e-9) * 0.015;
                out.push(Prev::Line(vec![x + Vec2::new(-d, -d), x + Vec2::new(d, d)]));
                out.push(Prev::Line(vec![x + Vec2::new(-d, d), x + Vec2::new(d, -d)]));
            }
            return out;
        }
        let (Some(sid), Some(c)) = (app.session.active_sketch, app.cad.hover_curve.as_ref()) else { return Vec::new() };
        if self.extend {
            return Vec::new();
        }
        let Some(at) = app.cad.raw else { return Vec::new() };
        let st = crate::open_meshing::session_world(&app.session);
        st.sketch(sid).and_then(|ss| solidcraft_engine::cmd::trim_piece(&ss.sketch, c, at)).map(|p| vec![Prev::Line(p)]).unwrap_or_default()
    }
}

// ---- FILLET / CHAMFER -----------------------------------------------------------------------

#[derive(Default)]
struct FilletM {
    first: Option<(String, Vec2)>,
    asking: bool,
}

/// Two curves brought to their corner (a fillet or chamfer of zero).
fn corner(app: &mut SolveApp, a: &(String, Vec2), b: &(String, Vec2)) -> Res {
    run(app, "sketch.extend", json!({"curves": [a.0, b.0], "at": [xy(a.1), xy(b.1)]}))?;
    Ok(Step::Done)
}

impl Machine for FilletM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        let r = FILLET_R.with(Cell::get);
        if self.asking {
            return Prompt::new("Specify fillet radius", Accept::NUMBER).default(geo::fmt(r));
        }
        match self.first {
            None => Prompt::new(format!("Select first object (radius {})", geo::fmt(r)), Accept::PICK).kw(&["Radius"]),
            Some(_) => Prompt::new("Select second object", Accept::PICK).kw(&["Radius"]),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match i {
            Input::Keyword("Radius") => self.asking = true,
            Input::Number(v) if self.asking && v >= 0.0 => {
                FILLET_R.with(|r| r.set(v));
                self.asking = false;
            }
            Input::Enter if self.asking => self.asking = false,
            Input::Point { p, curve: Some(c) } => match self.first.take() {
                None => self.first = Some((c, p)),
                Some(a) => {
                    let b = (c, p);
                    if a.0 == b.0 {
                        self.first = Some(a);
                        return Err("Pick a second, different object".into());
                    }
                    let r = FILLET_R.with(Cell::get);
                    if r <= 1e-9 {
                        return corner(app, &a, &b);
                    }
                    run(app, "sketch.fillet", json!({"a": a.0, "b": b.0, "radius": r}))?;
                    return Ok(Step::Done);
                }
            },
            Input::Point { .. } => return Err("No object there".into()),
            Input::Enter => return Ok(Step::Done),
            _ => return Err(bad_input()),
        }
        Ok(Step::Continue)
    }
    fn selected(&self) -> Vec<String> {
        self.first.iter().map(|f| f.0.clone()).collect()
    }
}

#[derive(Default)]
struct ChamferM {
    first: Option<(String, Vec2)>,
    /// 1: asking the first distance, 2: the second.
    asking: u8,
}

impl Machine for ChamferM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        let (d1, d2) = CHAMFER_D.with(Cell::get);
        match (self.asking, &self.first) {
            (1, _) => Prompt::new("Specify first chamfer distance", Accept::NUMBER).default(geo::fmt(d1)),
            (2, _) => Prompt::new("Specify second chamfer distance", Accept::NUMBER).default(geo::fmt(d2)),
            (_, None) => Prompt::new(format!("Select first line (distances {}, {})", geo::fmt(d1), geo::fmt(d2)), Accept::PICK).kw(&["Distance"]),
            (_, Some(_)) => Prompt::new("Select second line", Accept::PICK).kw(&["Distance"]),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        let (d1, d2) = CHAMFER_D.with(Cell::get);
        match (self.asking, i) {
            (_, Input::Keyword("Distance")) => self.asking = 1,
            (1, Input::Number(v)) if v >= 0.0 => {
                // The second distance defaults to the first.
                CHAMFER_D.with(|d| d.set((v, v)));
                self.asking = 2;
            }
            (2, Input::Number(v)) if v >= 0.0 => {
                CHAMFER_D.with(|d| d.set((d1, v)));
                self.asking = 0;
            }
            (1, Input::Enter) => self.asking = 2,
            (2, Input::Enter) => self.asking = 0,
            (0, Input::Point { p, curve: Some(c) }) => match self.first.take() {
                None => self.first = Some((c, p)),
                Some(a) => {
                    let b = (c, p);
                    if a.0 == b.0 {
                        self.first = Some(a);
                        return Err("Pick a second, different line".into());
                    }
                    if d1 <= 1e-9 && d2 <= 1e-9 {
                        return corner(app, &a, &b);
                    }
                    run(app, "sketch.chamfer.two_distance", json!({"a": a.0, "b": b.0, "distance": d1, "distance2": d2}))?;
                    return Ok(Step::Done);
                }
            },
            (0, Input::Point { .. }) => return Err("No line there".into()),
            (0, Input::Enter) => return Ok(Step::Done),
            _ => return Err(bad_input()),
        }
        Ok(Step::Continue)
    }
    fn selected(&self) -> Vec<String> {
        self.first.iter().map(|f| f.0.clone()).collect()
    }
}

// ---- OFFSET ---------------------------------------------------------------------------------

#[derive(Default)]
struct OffsetM {
    distance: Option<f64>,
    through: bool,
    object: Option<String>,
}

impl Machine for OffsetM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        match (self.distance.is_some() || self.through, &self.object) {
            (false, _) => {
                Prompt::new("Specify offset distance", Accept::POINT_OR_NUMBER).kw(&["Through"]).default(geo::fmt(OFFSET_D.with(Cell::get)))
            }
            (true, None) => Prompt::new("Select object to offset", Accept::PICK).kw(&["Exit"]),
            (true, Some(_)) if self.through => Prompt::new("Specify through point", Accept::POINT),
            (true, Some(_)) => Prompt::new("Specify point on side to offset", Accept::POINT),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        if self.distance.is_none() && !self.through {
            match i {
                Input::Keyword("Through") => self.through = true,
                Input::Number(v) if v > 0.0 => {
                    OFFSET_D.with(|d| d.set(v));
                    self.distance = Some(v);
                }
                Input::Enter => self.distance = Some(OFFSET_D.with(Cell::get)),
                _ => return Err("Requires a positive distance".into()),
            }
            return Ok(Step::Continue);
        }
        match (&self.object, i) {
            (_, Input::Keyword("Exit")) | (None, Input::Enter) => Ok(Step::Done),
            (None, Input::Point { curve: Some(c), .. }) => {
                self.object = Some(c);
                Ok(Step::Continue)
            }
            (None, Input::Point { .. }) => Err("No object there".into()),
            (Some(c), Input::Point { p, .. }) => {
                let c = c.clone();
                let d = match self.distance {
                    Some(d) if !self.through => d,
                    _ => ent(app, &c).map(|e| e.shape.dist(p)).ok_or("The object is gone")?,
                };
                if d <= 1e-9 {
                    return Err("The point is on the object".into());
                }
                let v = run(app, "sketch.offset", json!({"curves": [c], "distance": d, "side": xy(p), "chain": false}))?;
                // A plain copy: no tie to the curve it came from.
                for k in made(&v) {
                    let _ = app.run("sketch.offset.break", json!({"curve": k}));
                }
                self.object = None;
                Ok(Step::Continue)
            }
            _ => Err(bad_input()),
        }
    }
    fn selected(&self) -> Vec<String> {
        self.object.iter().cloned().collect()
    }
}

// ---- STRETCH --------------------------------------------------------------------------------

#[derive(Default)]
struct StretchM {
    corner: Option<Vec2>,
    window: Option<(Vec2, Vec2)>,
    base: Option<Vec2>,
}

impl StretchM {
    /// What the crossing window takes: the curves wholly inside, and the points inside of the
    /// curves it crosses.
    fn taken(app: &SolveApp, lo: Vec2, hi: Vec2) -> (Vec<String>, Vec<String>) {
        let inside = |q: Vec2| q.x >= lo.x && q.x <= hi.x && q.y >= lo.y && q.y <= hi.y;
        let Some(sid) = app.session.active_sketch else { return (Vec::new(), Vec::new()) };
        let st = crate::open_meshing::shown_world(app);
        let Some(ss) = st.sketch(sid) else { return (Vec::new(), Vec::new()) };
        let sk = &ss.sketch;
        let mut curves = Vec::new();
        let mut points = Vec::new();
        for c in &sk.curves {
            let ids = c.kind.point_ids();
            let pos: Vec<Vec2> = ids.iter().filter_map(|k| sk.point(*k)).collect();
            if !pos.is_empty() && pos.iter().all(|p| inside(*p)) {
                curves.push(c.id.clone());
            } else {
                for k in ids {
                    if sk.point(k).is_some_and(inside)
                        && let Some(pt) = sk.points.get(k)
                        && !points.contains(&pt.id)
                    {
                        points.push(pt.id.clone());
                    }
                }
            }
        }
        (curves, points)
    }
}

impl Machine for StretchM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        match (self.window, self.corner, self.base) {
            (None, None, _) => Prompt::new("Select objects to stretch by crossing-window: first corner", Accept::SELECT),
            (None, Some(_), _) => Prompt::new("Specify opposite corner", Accept::SELECT),
            (Some(_), _, None) => Prompt::new("Specify base point", Accept::POINT),
            (Some(_), _, Some(b)) => Prompt::new("Specify second point", Accept::POINT).base(Some(b)),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        let Input::Point { p, .. } = i else {
            return if i == Input::Enter { Ok(Step::Done) } else { Err(bad_input()) };
        };
        match (self.window, self.corner, self.base) {
            (None, None, _) => self.corner = Some(p),
            (None, Some(a), _) => self.window = Some((Vec2::new(a.x.min(p.x), a.y.min(p.y)), Vec2::new(a.x.max(p.x), a.y.max(p.y)))),
            (Some(_), _, None) => self.base = Some(p),
            (Some((lo, hi)), _, Some(b)) => {
                let (mut curves, points) = Self::taken(app, lo, hi);
                curves.extend(points);
                if curves.is_empty() {
                    return Err("Nothing in the window".into());
                }
                let d = p - b;
                run(app, "sketch.move", json!({"entities": curves, "translate": [d.x, d.y]}))?;
                return Ok(Step::Done);
            }
        }
        Ok(Step::Continue)
    }
    fn preview(&self, app: &SolveApp, c: Vec2) -> Vec<Prev> {
        match (self.window, self.corner, self.base) {
            (None, Some(a), _) => vec![Prev::Window(c, a)],
            (Some((lo, hi)), _, Some(b)) => {
                let inside = |q: Vec2| q.x >= lo.x && q.x <= hi.x && q.y >= lo.y && q.y <= hi.y;
                let d = c - b;
                let mut out: Vec<Prev> = ents(app)
                    .into_iter()
                    .filter_map(|e| match e.shape {
                        Shape::Line(a, z) if inside(a) || inside(z) => {
                            Some(Prev::Line(vec![if inside(a) { a + d } else { a }, if inside(z) { z + d } else { z }]))
                        }
                        ref s if s.polyline().iter().all(|q| inside(*q)) => Some(Prev::Line(s.polyline().into_iter().map(|q| q + d).collect())),
                        _ => None,
                    })
                    .collect();
                out.push(Prev::Line(vec![b, c]));
                out
            }
            _ => Vec::new(),
        }
    }
}

// ---- BREAK ----------------------------------------------------------------------------------

#[derive(Default)]
struct BreakM {
    object: Option<(String, Vec2)>,
    first_point: bool,
}

impl Machine for BreakM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        match (&self.object, self.first_point) {
            (None, _) => Prompt::new("Select object", Accept::PICK),
            (Some(_), true) => Prompt::new("Specify first break point", Accept::POINT),
            (Some(_), false) => Prompt::new("Specify second break point", Accept::POINT).kw(&["First point"]),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match (&self.object, i) {
            (None, Input::Point { p, curve: Some(c) }) => {
                let q = ent(app, &c).and_then(|e| e.shape.nearest(p)).unwrap_or(p);
                self.object = Some((c, q));
                Ok(Step::Continue)
            }
            (None, Input::Point { .. }) => Err("No object there".into()),
            (Some(_), Input::Keyword("First point")) => {
                self.first_point = true;
                Ok(Step::Continue)
            }
            (Some((c, q)), Input::Point { p, .. }) if self.first_point => {
                let c = c.clone();
                let q2 = ent(app, &c).and_then(|e| e.shape.nearest(p)).unwrap_or(p);
                let _ = q;
                self.object = Some((c, q2));
                self.first_point = false;
                Ok(Step::Continue)
            }
            (Some((c, q1)), Input::Point { p, .. }) => {
                let (c, q1) = (c.clone(), *q1);
                let e = ent(app, &c).ok_or("The object is gone")?;
                let q2 = e.shape.nearest(p).unwrap_or(p);
                // The piece that goes: between the two points (counter-clockwise from the first
                // on a circle, as CADCraft breaks one).
                let middle = match &e.shape {
                    Shape::Circle(o, r) => *o + polar((geo::norm_angle_from((q2 - *o).angle(), (q1 - *o).angle()) + (q1 - *o).angle()) * 0.5) * *r,
                    Shape::Arc { c: o, r, .. } => {
                        let (a, b) = ((q1 - *o).angle(), (q2 - *o).angle());
                        let (a, b) = if geo::norm_angle_from(b, a) - a < std::f64::consts::PI { (a, b) } else { (b, a) };
                        *o + polar((a + geo::norm_angle_from(b, a)) * 0.5) * *r
                    }
                    _ => (q1 + q2) * 0.5,
                };
                let before: Vec<String> = ents(app).into_iter().map(|e| e.id).collect();
                run(app, "sketch.break", json!({"curve": c, "points": [xy(q1), xy(q2)]}))?;
                let pieces: Vec<super::geo::Ent> = ents(app).into_iter().filter(|e| e.id == c || !before.contains(&e.id)).collect();
                if let Some(gone) = pieces.iter().min_by(|a, b| a.shape.dist(middle).total_cmp(&b.shape.dist(middle))) {
                    // Only the piece between the points: its ends are the two break points.
                    let ends = gone.shape.ends();
                    let between = ends.len() == 2 && ends.iter().all(|x| x.dist(q1) < 1e-6 || x.dist(q2) < 1e-6);
                    if between || q1.dist(q2) < 1e-9 {
                        delete(app, std::slice::from_ref(&gone.id));
                    }
                }
                Ok(Step::Done)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Err(bad_input()),
        }
    }
    fn selected(&self) -> Vec<String> {
        self.object.iter().map(|o| o.0.clone()).collect()
    }
}

// ---- LENGTHEN -------------------------------------------------------------------------------

#[derive(Default, Clone, Copy, PartialEq)]
enum LMode {
    #[default]
    Dynamic,
    Delta(f64),
    Percent(f64),
    Total(f64),
}

#[derive(Default)]
struct LengthenM {
    mode: LMode,
    /// Asking the value for this option.
    asking: Option<&'static str>,
    /// Dynamic: the end being dragged (curve, which end).
    dragging: Option<(String, bool)>,
    started: bool,
}

/// The curve's end nearest `p`: (point id, is the end).
fn end_point(app: &SolveApp, curve: &str, p: Vec2) -> Option<(String, Vec2, Vec2, bool)> {
    use solidcraft_engine::sketch::CurveKind;
    let sid = app.session.active_sketch?;
    let st = crate::open_meshing::shown_world(app);
    let sk = &st.sketch(sid)?.sketch;
    let c = sk.curves.get(sk.curve_index(curve)?)?;
    let (a, b) = match c.kind {
        CurveKind::Line { a, b } => (a, b),
        CurveKind::Arc { a, b, .. } => (a, b),
        _ => return None,
    };
    let (pa, pb) = (sk.point(a)?, sk.point(b)?);
    let end = pb.dist(p) < pa.dist(p);
    let (k, at, other) = if end { (b, pb, pa) } else { (a, pa, pb) };
    Some((sk.points.get(k)?.id.clone(), at, other, end))
}

impl LengthenM {
    fn change(&self, app: &mut SolveApp, curve: &str, p: Vec2) -> Result<(), String> {
        let e = ent(app, curve).ok_or("The object is gone")?;
        let (pid, at, other, end) = end_point(app, curve, p).ok_or("Lengthen changes lines and arcs")?;
        let to = match (&e.shape, self.mode) {
            (Shape::Line(..), m) => {
                let len = other.dist(at);
                let dir = (at - other).normalized().ok_or("Zero-length line")?;
                let new = match m {
                    LMode::Delta(d) => len + d,
                    LMode::Percent(pc) => len * pc / 100.0,
                    LMode::Total(t) => t,
                    LMode::Dynamic => return Ok(()),
                };
                if new <= 1e-9 {
                    return Err("The new length is not positive".into());
                }
                other + dir * new
            }
            (Shape::Arc { c, r, a0, a1 }, m) => {
                let sweep = a1 - a0;
                let len = sweep * r;
                let new = match m {
                    LMode::Delta(d) => len + d,
                    LMode::Percent(pc) => len * pc / 100.0,
                    LMode::Total(t) => t,
                    LMode::Dynamic => return Ok(()),
                } / r;
                if !(new > 1e-9 && new < std::f64::consts::TAU) {
                    return Err("The new length does not fit the arc's circle".into());
                }
                // The arc runs counter-clockwise from a0 to a1: the end at a1 grows forward, the
                // one at a0 backward.
                if end { *c + polar(a0 + new) * *r } else { *c + polar(a1 - new) * *r }
            }
            _ => return Err("Lengthen changes lines and arcs".into()),
        };
        run(app, "sketch.move_point", json!({"point": pid, "to": xy(to)}))?;
        Ok(())
    }
}

impl Machine for LengthenM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        if let Some(k) = self.asking {
            return match k {
                "DElta" => Prompt::new("Enter delta length", Accept::NUMBER),
                "Percent" => Prompt::new("Enter percentage length", Accept::NUMBER).default("100"),
                _ => Prompt::new("Specify total length", Accept::NUMBER),
            };
        }
        if self.dragging.is_some() {
            return Prompt::new("Specify new end point", Accept::POINT);
        }
        if !self.started {
            return Prompt::new("Choose how", Accept::PICK).kw(&["DElta", "Percent", "Total", "DYnamic"]).default("DYnamic");
        }
        Prompt::new("Select an object to change (near the end)", Accept::PICK)
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        if let Some(k) = self.asking {
            let Input::Number(v) = i else { return Err("Requires a number".into()) };
            self.mode = match k {
                "DElta" => LMode::Delta(v),
                "Percent" if v > 0.0 => LMode::Percent(v),
                "Total" if v > 0.0 => LMode::Total(v),
                _ => return Err("Requires a positive number".into()),
            };
            self.asking = None;
            self.started = true;
            return Ok(Step::Continue);
        }
        if let Some((c, _)) = self.dragging.clone() {
            let Input::Point { p, .. } = i else { return Err(bad_input()) };
            let e = ent(app, &c).ok_or("The object is gone")?;
            let (pid, ..) = end_point(app, &c, p).ok_or("Lengthen changes lines and arcs")?;
            let to = match e.shape {
                Shape::Line(a, b) => geo::line_cross(a, b, p, p + Vec2::new(-(b - a).y, (b - a).x)).unwrap_or(p),
                Shape::Arc { c, r, .. } => (p - c).normalized().map(|d| c + d * r).unwrap_or(p),
                _ => p,
            };
            run(app, "sketch.move_point", json!({"point": pid, "to": xy(to)}))?;
            self.dragging = None;
            return Ok(Step::Continue);
        }
        match i {
            Input::Keyword(k @ ("DElta" | "Percent" | "Total")) => {
                self.asking = Some(k);
                Ok(Step::Continue)
            }
            Input::Keyword("DYnamic") | Input::Enter if !self.started => {
                self.mode = LMode::Dynamic;
                self.started = true;
                Ok(Step::Continue)
            }
            Input::Point { p, curve: Some(c) } => {
                if self.mode == LMode::Dynamic {
                    end_point(app, &c, p).ok_or("Lengthen changes lines and arcs")?;
                    self.dragging = Some((c, true));
                } else {
                    self.started = true;
                    self.change(app, &c, p)?;
                }
                Ok(Step::Continue)
            }
            Input::Point { .. } => Err("No object there".into()),
            Input::Enter => Ok(Step::Done),
            _ => Err(bad_input()),
        }
    }
    fn selected(&self) -> Vec<String> {
        self.dragging.iter().map(|d| d.0.clone()).collect()
    }
}

// ---- MATCHPROP ------------------------------------------------------------------------------

#[derive(Default)]
struct MatchM {
    source: Option<(String, bool)>,
}

impl Machine for MatchM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        match self.source {
            None => Prompt::new("Select source object", Accept::PICK),
            Some(_) => Prompt::new("Select destination object(s)", Accept::PICK),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match (&self.source, i) {
            (None, Input::Point { curve: Some(c), .. }) => {
                let construction = ent(app, &c).is_some_and(|e| e.construction);
                self.source = Some((c, construction));
                Ok(Step::Continue)
            }
            (Some((_, construction)), Input::Point { curve: Some(c), .. }) => {
                run(app, "sketch.construction", json!({"curves": [c], "value": construction}))?;
                Ok(Step::Continue)
            }
            (_, Input::Point { .. }) => Err("No object there".into()),
            (_, Input::Enter) => Ok(Step::Done),
            _ => Err(bad_input()),
        }
    }
    fn selected(&self) -> Vec<String> {
        self.source.iter().map(|s| s.0.clone()).collect()
    }
}
