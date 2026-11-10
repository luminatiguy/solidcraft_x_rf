//! The drawing tools, after CADCraft's DRAW commands: their prompts, options and order of points.

use std::cell::Cell;
use std::f64::consts::{FRAC_PI_2, TAU};

use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_engine::geom::prelude::*;

use super::geo::{self, arc_points, arc3_points, circle3, polar};
use super::{Accept, Input, Machine, Prev, Prompt, Res, Step, add_arc3, add_line, delete, made, made_points, run, xy};
use crate::SolveApp;

thread_local! {
    static LAST_RADIUS: Cell<f64> = const { Cell::new(10.0) };
    static POLY_SIDES: Cell<u32> = const { Cell::new(4) };
    static DONUT: Cell<(f64, f64)> = const { Cell::new((5.0, 10.0)) };
    static CLOUD_ARC: Cell<f64> = const { Cell::new(10.0) };
}

pub fn make(id: &str) -> Option<Box<dyn Machine>> {
    Some(match id {
        "cad.line" => Box::new(LineM::default()),
        "cad.pline" => Box::new(PlineM::default()),
        "cad.circle" => Box::new(CircleM::default()),
        "cad.arc" => Box::new(ArcM::default()),
        "cad.rectang" => Box::new(RectM::default()),
        "cad.polygon" => Box::new(PolygonM::default()),
        "cad.ellipse" => Box::new(EllipseM::default()),
        "cad.spline" => Box::new(SplineM::default()),
        "cad.xline" => Box::new(XlineM::default()),
        "cad.point" => Box::new(PointM),
        "cad.donut" => Box::new(DonutM::default()),
        "cad.revcloud" => Box::new(CloudM::default()),
        _ => return None,
    })
}

fn bad_input() -> String {
    "Point or option keyword required.".into()
}

// ---- LINE ----------------------------------------------------------------------------------

#[derive(Default)]
struct LineM {
    pts: Vec<Vec2>,
    made: Vec<Vec<String>>,
}

impl Machine for LineM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        match self.pts.len() {
            0 => Prompt::new("Specify first point", Accept::POINT),
            n => {
                let kw: &[&'static str] = if n >= 3 { &["Close", "Undo"] } else { &["Undo"] };
                Prompt::new("Specify next point", Accept::POINT).kw(kw).base(self.pts.last().copied())
            }
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match i {
            Input::Point { p, .. } => {
                if let Some(last) = self.pts.last().copied() {
                    if last.dist(p) < 1e-9 {
                        return Ok(Step::Continue);
                    }
                    self.made.push(add_line(app, last, p)?);
                }
                self.pts.push(p);
                Ok(Step::Continue)
            }
            Input::Keyword("Undo") => {
                if let Some(ids) = self.made.pop() {
                    delete(app, &ids);
                }
                self.pts.pop();
                Ok(Step::Continue)
            }
            Input::Keyword("Close") => {
                if let (Some(a), Some(b)) = (self.pts.last().copied(), self.pts.first().copied()) {
                    add_line(app, a, b)?;
                }
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err(bad_input()),
        }
    }
    fn preview(&self, _: &SolveApp, c: Vec2) -> Vec<Prev> {
        self.pts.last().map(|l| vec![Prev::Line(vec![*l, c])]).unwrap_or_default()
    }
}

// ---- PLINE (lines and tangent arcs end to end) -----------------------------------------------

#[derive(Default)]
struct PlineM {
    verts: Vec<Vec2>,
    /// Direction the polyline runs in at each vertex (after the segment ending there).
    dirs: Vec<Vec2>,
    arc: bool,
    made: Vec<Vec<String>>,
}

impl PlineM {
    fn dir(&self) -> Vec2 {
        self.dirs.last().copied().unwrap_or(Vec2::X)
    }
    /// The tangent arc from the last vertex to `e`: its middle point and its direction at `e`.
    fn arc_to(&self, e: Vec2) -> Option<(Vec2, Vec2)> {
        let s = *self.verts.last()?;
        let m = geo::tangent_arc_mid(s, self.dir(), e)?;
        let (c, _) = circle3(s, m, e)?;
        let ccw = (m - s).cross(e - m) > 0.0;
        let r = e - c;
        let t = if ccw { Vec2::new(-r.y, r.x) } else { Vec2::new(r.y, -r.x) };
        Some((m, t.normalized()?))
    }
    fn segment(&mut self, app: &mut SolveApp, e: Vec2) -> Result<(), String> {
        let Some(s) = self.verts.last().copied() else { return Ok(()) };
        if s.dist(e) < 1e-9 {
            return Ok(());
        }
        let (ids, dir) = if self.arc && !self.verts.is_empty() {
            match self.arc_to(e) {
                Some((m, d)) => (add_arc3(app, s, m, e)?, d),
                None => (add_line(app, s, e)?, (e - s).normalized().unwrap_or(Vec2::X)),
            }
        } else {
            (add_line(app, s, e)?, (e - s).normalized().unwrap_or(Vec2::X))
        };
        self.made.push(ids);
        self.dirs.push(dir);
        self.verts.push(e);
        Ok(())
    }
}

impl Machine for PlineM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        let base = self.verts.last().copied();
        match (self.verts.len(), self.arc) {
            (0, _) => Prompt::new("Specify start point", Accept::POINT),
            (n, false) => {
                let kw: &[&'static str] = if n >= 2 { &["Arc", "Close", "Undo"] } else { &["Arc", "Undo"] };
                Prompt::new("Specify next point", Accept::POINT).kw(kw).base(base)
            }
            (n, true) => {
                let kw: &[&'static str] = if n >= 2 { &["Line", "CLose", "Undo"] } else { &["Line", "Undo"] };
                Prompt::new("Specify endpoint of arc", Accept::POINT).kw(kw).base(base)
            }
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match i {
            Input::Point { p, .. } => {
                if self.verts.is_empty() {
                    self.verts.push(p);
                } else {
                    self.segment(app, p)?;
                }
                Ok(Step::Continue)
            }
            Input::Keyword("Arc") => {
                self.arc = true;
                Ok(Step::Continue)
            }
            Input::Keyword("Line") => {
                self.arc = false;
                Ok(Step::Continue)
            }
            Input::Keyword("Close" | "CLose") => {
                if let Some(f) = self.verts.first().copied() {
                    self.segment(app, f)?;
                }
                Ok(Step::Done)
            }
            Input::Keyword("Undo") => {
                if let Some(ids) = self.made.pop() {
                    delete(app, &ids);
                    self.dirs.pop();
                }
                self.verts.pop();
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err(bad_input()),
        }
    }
    fn preview(&self, _: &SolveApp, c: Vec2) -> Vec<Prev> {
        let Some(s) = self.verts.last().copied() else { return Vec::new() };
        if self.arc
            && let Some((m, _)) = self.arc_to(c)
            && let Some(pts) = arc3_points(s, m, c)
        {
            return vec![Prev::Line(pts)];
        }
        vec![Prev::Line(vec![s, c])]
    }
}

// ---- CIRCLE ---------------------------------------------------------------------------------

#[derive(Default)]
struct CircleM {
    /// 0 centre-radius, 1 3P, 2 2P, 3 Ttr.
    mode: u8,
    pts: Vec<Vec2>,
    diameter: bool,
    tans: Vec<(String, Vec2)>,
}

fn add_circle(app: &mut SolveApp, c: Vec2, r: f64) -> Result<(), String> {
    if !(r > 1e-9 && r.is_finite()) {
        return Err("Value must be positive and nonzero.".into());
    }
    run(app, "sketch.circle.center", json!({"center": xy(c), "radius": r, "connect": false}))?;
    LAST_RADIUS.with(|l| l.set(r));
    Ok(())
}

impl Machine for CircleM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        let n = self.pts.len();
        let last = LAST_RADIUS.with(Cell::get);
        match (self.mode, n) {
            (0, 0) => Prompt::new("Specify center point for circle", Accept::POINT).kw(&["3P", "2P", "Ttr"]),
            (0, _) if self.diameter => {
                Prompt::new("Specify diameter of circle", Accept::POINT_OR_NUMBER).base(self.pts.first().copied()).default(geo::fmt(last * 2.0))
            }
            (0, _) => Prompt::new("Specify radius of circle", Accept::POINT_OR_NUMBER)
                .kw(&["Diameter"])
                .base(self.pts.first().copied())
                .default(geo::fmt(last)),
            (1, k) => Prompt::new(
                ["Specify first point on circle", "Specify second point on circle", "Specify third point on circle"].get(k).copied().unwrap_or(""),
                Accept::POINT,
            )
            .base(self.pts.last().copied()),
            (2, k) => Prompt::new(
                if k == 0 { "Specify first end point of circle's diameter" } else { "Specify second end point of circle's diameter" },
                Accept::POINT,
            )
            .base(self.pts.last().copied()),
            (_, _) => match self.tans.len() {
                0 => Prompt::new("Specify point on object for first tangent of circle", Accept::PICK),
                1 => Prompt::new("Specify point on object for second tangent of circle", Accept::PICK),
                _ => Prompt::new("Specify radius of circle", Accept::NUMBER).default(geo::fmt(last)),
            },
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match i {
            Input::Keyword(k) => {
                match k {
                    "3P" => self.mode = 1,
                    "2P" => self.mode = 2,
                    "Ttr" => self.mode = 3,
                    "Diameter" => self.diameter = true,
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Input::Point { p, curve } if self.mode == 3 => {
                let c = curve.ok_or("Pick a line, circle or arc")?;
                self.tans.push((c, p));
                Ok(Step::Continue)
            }
            Input::Point { p, .. } => {
                self.pts.push(p);
                match (self.mode, self.pts.as_slice()) {
                    (0, [c, e]) => {
                        let r = if self.diameter { c.dist(*e) / 2.0 } else { c.dist(*e) };
                        add_circle(app, *c, r)?;
                        Ok(Step::Done)
                    }
                    (1, [a, b, c]) => {
                        let (o, r) = circle3(*a, *b, *c).ok_or("The points are on one line")?;
                        add_circle(app, o, r)?;
                        Ok(Step::Done)
                    }
                    (2, [a, b]) => {
                        add_circle(app, (*a + *b) * 0.5, a.dist(*b) / 2.0)?;
                        Ok(Step::Done)
                    }
                    _ => Ok(Step::Continue),
                }
            }
            Input::Number(v) if self.mode == 0 && self.pts.len() == 1 => {
                let r = if self.diameter { v / 2.0 } else { v };
                add_circle(app, self.pts[0], r)?;
                Ok(Step::Done)
            }
            Input::Number(v) if self.mode == 3 && self.tans.len() == 2 => self.ttr(app, v),
            Input::Enter if self.mode == 0 && self.pts.len() == 1 => {
                let last = LAST_RADIUS.with(Cell::get);
                add_circle(app, self.pts[0], last)?;
                Ok(Step::Done)
            }
            Input::Enter if self.mode == 3 && self.tans.len() == 2 => self.ttr(app, LAST_RADIUS.with(Cell::get)),
            Input::Enter => Ok(Step::Done),
            _ => Err(bad_input()),
        }
    }
    fn preview(&self, _: &SolveApp, c: Vec2) -> Vec<Prev> {
        match (self.mode, self.pts.as_slice()) {
            (0, [o]) => {
                let r = if self.diameter { o.dist(c) / 2.0 } else { o.dist(c) };
                vec![Prev::Line(arc_points(*o, r, 0.0, TAU)), Prev::Line(vec![*o, c])]
            }
            (1, [a, b]) => circle3(*a, *b, c).map(|(o, r)| vec![Prev::Line(arc_points(o, r, 0.0, TAU))]).unwrap_or_default(),
            (1, [a]) | (2, [a]) if self.mode == 2 => vec![Prev::Line(arc_points((*a + c) * 0.5, a.dist(c) / 2.0, 0.0, TAU))],
            (1, [a]) => vec![Prev::Line(vec![*a, c])],
            _ => Vec::new(),
        }
    }
}

impl CircleM {
    fn ttr(&mut self, app: &mut SolveApp, r: f64) -> Res {
        let [(a, pa), (b, pb)] = [self.tans[0].clone(), self.tans[1].clone()];
        run(app, "sketch.circle.two_tangent", json!({"curves": [a, b], "radius": r, "near": xy((pa + pb) * 0.5)}))?;
        LAST_RADIUS.with(|l| l.set(r));
        Ok(Step::Done)
    }
}

// ---- ARC ------------------------------------------------------------------------------------

#[derive(Default)]
struct ArcM {
    center_first: bool,
    pts: Vec<Vec2>,
}

impl Machine for ArcM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        let base = self.pts.last().copied();
        match (self.center_first, self.pts.len()) {
            (false, 0) => Prompt::new("Specify start point of arc", Accept::POINT).kw(&["Center"]),
            (false, 1) => Prompt::new("Specify second point of arc", Accept::POINT).base(base),
            (false, _) => Prompt::new("Specify end point of arc", Accept::POINT).base(base),
            (true, 0) => Prompt::new("Specify center point of arc", Accept::POINT),
            (true, 1) => Prompt::new("Specify start point of arc", Accept::POINT).base(base),
            (true, _) => Prompt::new("Specify end point of arc (counter-clockwise)", Accept::POINT).base(self.pts.first().copied()),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match i {
            Input::Keyword("Center") if self.pts.is_empty() => {
                self.center_first = true;
                Ok(Step::Continue)
            }
            Input::Point { p, .. } => {
                self.pts.push(p);
                if self.pts.len() < 3 {
                    return Ok(Step::Continue);
                }
                let (a, b, c) = (self.pts[0], self.pts[1], self.pts[2]);
                if self.center_first {
                    run(app, "sketch.arc.center_point", json!({"center": xy(a), "start": xy(b), "end": xy(c), "connect": false}))?;
                } else {
                    circle3(a, b, c).ok_or("The points are on one line")?;
                    add_arc3(app, a, b, c)?;
                }
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err(bad_input()),
        }
    }
    fn preview(&self, _: &SolveApp, c: Vec2) -> Vec<Prev> {
        match (self.center_first, self.pts.as_slice()) {
            (false, [a]) => vec![Prev::Line(vec![*a, c])],
            (false, [a, b]) => arc3_points(*a, *b, c).map(|p| vec![Prev::Line(p)]).unwrap_or_default(),
            (true, [o]) => vec![Prev::Line(vec![*o, c])],
            (true, [o, s]) => {
                let r = o.dist(*s);
                let a0 = (*s - *o).angle();
                let a1 = geo::norm_angle_from((c - *o).angle(), a0);
                vec![Prev::Line(arc_points(*o, r, a0, a1)), Prev::Line(vec![*o, c])]
            }
            _ => Vec::new(),
        }
    }
}

// ---- RECTANG --------------------------------------------------------------------------------

#[derive(Default)]
struct RectM {
    first: Option<Vec2>,
    /// 1 fillet radius, 2 chamfer distance, 3 length, 4 width, 5 side to place.
    ask: u8,
    fillet: f64,
    chamfer: f64,
    size: Option<(f64, f64)>,
}

impl RectM {
    fn corners(a: Vec2, b: Vec2) -> [Vec2; 4] {
        [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)]
    }
    fn other(&self, c: Vec2) -> Option<Vec2> {
        let a = self.first?;
        Some(match self.size {
            Some((l, w)) if l > 0.0 && w > 0.0 => a + Vec2::new(l * (c.x - a.x).signum(), w * (c.y - a.y).signum()),
            _ => c,
        })
    }
    fn make(&self, app: &mut SolveApp, b: Vec2) -> Res {
        let a = self.first.ok_or("Specify the first corner")?;
        if (a.x - b.x).abs() < 1e-9 || (a.y - b.y).abs() < 1e-9 {
            return Err("The rectangle has no area".into());
        }
        let c = Self::corners(a, b);
        let v = run(app, "sketch.line", json!({"points": c.iter().map(|p| xy(*p)).collect::<Vec<_>>(), "closed": true, "connect": false}))?;
        let corners = made_points(&v);
        if self.fillet > 0.0 && !corners.is_empty() {
            run(app, "sketch.fillet", json!({"points": corners, "radius": self.fillet}))?;
        } else if self.chamfer > 0.0 && !corners.is_empty() {
            run(app, "sketch.chamfer.equal_distance", json!({"points": corners, "distance": self.chamfer}))?;
        }
        Ok(Step::Done)
    }
}

impl Machine for RectM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        match (self.ask, self.first) {
            (1, _) => Prompt::new("Specify fillet radius for rectangles", Accept::NUMBER).default(geo::fmt(self.fillet)),
            (2, _) => Prompt::new("Specify chamfer distance for rectangles", Accept::NUMBER).default(geo::fmt(self.chamfer)),
            (3, _) => Prompt::new("Specify length for rectangles", Accept::NUMBER),
            (4, _) => Prompt::new("Specify width for rectangles", Accept::NUMBER),
            (_, None) => Prompt::new("Specify first corner point", Accept::POINT).kw(&["Chamfer", "Fillet"]),
            (_, Some(a)) => Prompt::new("Specify other corner point", Accept::POINT).kw(&["Dimensions"]).base(Some(a)).sides(),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match (self.ask, i) {
            (1, Input::Number(v)) => {
                (self.fillet, self.chamfer, self.ask) = (v.max(0.0), 0.0, 0);
                Ok(Step::Continue)
            }
            (2, Input::Number(v)) => {
                (self.chamfer, self.fillet, self.ask) = (v.max(0.0), 0.0, 0);
                Ok(Step::Continue)
            }
            (3, Input::Number(v)) if v > 0.0 => {
                self.size = Some((v, 0.0));
                self.ask = 4;
                Ok(Step::Continue)
            }
            (4, Input::Number(v)) if v > 0.0 => {
                self.size = self.size.map(|(l, _)| (l, v));
                self.ask = 5;
                Ok(Step::Continue)
            }
            (1 | 2, Input::Enter) => {
                self.ask = 0;
                Ok(Step::Continue)
            }
            (_, Input::Keyword("Fillet")) => {
                self.ask = 1;
                Ok(Step::Continue)
            }
            (_, Input::Keyword("Chamfer")) => {
                self.ask = 2;
                Ok(Step::Continue)
            }
            (_, Input::Keyword("Dimensions")) => {
                self.ask = 3;
                Ok(Step::Continue)
            }
            (_, Input::Point { p, .. }) if self.first.is_none() => {
                self.first = Some(p);
                Ok(Step::Continue)
            }
            (_, Input::Point { p, .. }) => {
                let b = self.other(p).ok_or("Specify the first corner")?;
                self.make(app, b)
            }
            (_, Input::Enter) => Ok(Step::Done),
            _ => Err(bad_input()),
        }
    }
    fn preview(&self, _: &SolveApp, c: Vec2) -> Vec<Prev> {
        let (Some(a), Some(b)) = (self.first, self.other(c)) else { return Vec::new() };
        let k = Self::corners(a, b);
        vec![Prev::Line(vec![k[0], k[1], k[2], k[3], k[0]])]
    }
}

// ---- POLYGON --------------------------------------------------------------------------------

#[derive(Default)]
struct PolygonM {
    sides: Option<u32>,
    edge: bool,
    pts: Vec<Vec2>,
    /// Inscribed (`Some(true)`) or circumscribed, once chosen.
    inscribed: Option<bool>,
}

fn polygon_points(c: Vec2, r: f64, n: u32, angle: f64, inscribed: bool) -> Vec<Vec2> {
    let n = n.max(3);
    let step = TAU / f64::from(n);
    // Circumscribed: `r` reaches the middle of an edge, the corners lie further out.
    let (rv, a0) = if inscribed { (r, angle) } else { (r / (step / 2.0).cos(), angle + step / 2.0) };
    let mut v: Vec<Vec2> = (0..n).map(|k| c + polar(a0 + step * f64::from(k)) * rv).collect();
    if let Some(f) = v.first().copied() {
        v.push(f);
    }
    v
}

impl Machine for PolygonM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        let def = POLY_SIDES.with(Cell::get);
        match (self.sides, self.edge, self.pts.len(), self.inscribed) {
            (None, ..) => Prompt::new("Enter number of sides", Accept::NUMBER).default(def.to_string()),
            (Some(_), true, 0, _) => Prompt::new("Specify first endpoint of edge", Accept::POINT),
            (Some(_), true, _, _) => Prompt::new("Specify second endpoint of edge", Accept::POINT).base(self.pts.first().copied()),
            (Some(_), false, 0, _) => Prompt::new("Specify center of polygon", Accept::POINT).kw(&["Edge"]),
            (Some(_), false, _, None) => {
                Prompt::new("Enter an option", Accept::NUMBER).kw(&["Inscribed in circle", "Circumscribed about circle"]).default("I")
            }
            (Some(_), false, _, Some(_)) => Prompt::new("Specify radius of circle", Accept::POINT_OR_NUMBER).base(self.pts.first().copied()),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        let n = self.sides.unwrap_or(4);
        match i {
            Input::Number(v) if self.sides.is_none() => {
                if !(3.0..=1024.0).contains(&v) || v.fract() != 0.0 {
                    return Err("Enter an integer between 3 and 1024.".into());
                }
                self.sides = Some(v as u32);
                POLY_SIDES.with(|s| s.set(v as u32));
                Ok(Step::Continue)
            }
            Input::Enter if self.sides.is_none() => {
                self.sides = Some(POLY_SIDES.with(Cell::get));
                Ok(Step::Continue)
            }
            Input::Keyword("Edge") => {
                self.edge = true;
                Ok(Step::Continue)
            }
            Input::Keyword("Inscribed in circle") => {
                self.inscribed = Some(true);
                Ok(Step::Continue)
            }
            Input::Keyword("Circumscribed about circle") => {
                self.inscribed = Some(false);
                Ok(Step::Continue)
            }
            Input::Enter if self.inscribed.is_none() && !self.pts.is_empty() => {
                self.inscribed = Some(true);
                Ok(Step::Continue)
            }
            Input::Point { p, .. } if self.edge && self.pts.len() == 1 => {
                run(app, "sketch.polygon.edge", json!({"p0": xy(self.pts[0]), "p1": xy(p), "sides": n}))?;
                Ok(Step::Done)
            }
            Input::Point { p, .. } if self.pts.is_empty() && self.sides.is_some() => {
                self.pts.push(p);
                Ok(Step::Continue)
            }
            Input::Point { p, .. } if self.inscribed.is_some() => {
                let c = self.pts[0];
                self.make(app, c, c.dist(p), (p - c).angle(), n)
            }
            Input::Number(r) if self.inscribed.is_some() && r > 0.0 => {
                let c = self.pts[0];
                // A typed radius: a flat bottom edge, as in CADCraft.
                let step = TAU / f64::from(n);
                let angle = if self.inscribed == Some(true) { -FRAC_PI_2 + step / 2.0 } else { -FRAC_PI_2 };
                self.make(app, c, r, angle, n)
            }
            _ => Err(bad_input()),
        }
    }
    fn preview(&self, _: &SolveApp, c: Vec2) -> Vec<Prev> {
        let n = self.sides.unwrap_or(4);
        match (self.edge, self.pts.as_slice(), self.inscribed) {
            (true, [a], _) => {
                let len = a.dist(c);
                let step = TAU / f64::from(n.max(3));
                let r = len / (2.0 * (step / 2.0).sin());
                let mid = (*a + c) * 0.5;
                let Some(d) = (c - *a).normalized() else { return Vec::new() };
                let left = Vec2::new(-d.y, d.x);
                let o = mid + left * (r * (step / 2.0).cos());
                vec![Prev::Line(polygon_points(o, r, n, (*a - o).angle(), true))]
            }
            (false, [o], Some(ins)) => vec![Prev::Line(polygon_points(*o, o.dist(c), n, (c - *o).angle(), ins))],
            _ => Vec::new(),
        }
    }
}

impl PolygonM {
    fn make(&self, app: &mut SolveApp, c: Vec2, r: f64, angle: f64, n: u32) -> Res {
        if !(r > 1e-9) {
            return Err("Value must be positive and nonzero.".into());
        }
        let ins = self.inscribed != Some(false);
        // The vertex angle the sketch command takes: a corner of an inscribed polygon, the middle
        // of an edge of a circumscribed one.
        let cmd = if ins { "sketch.polygon.inscribed" } else { "sketch.polygon.circumscribed" };
        run(app, cmd, json!({"center": xy(c), "radius": r, "sides": n, "angle": angle.to_degrees()}))?;
        Ok(Step::Done)
    }
}

// ---- ELLIPSE --------------------------------------------------------------------------------

#[derive(Default)]
struct EllipseM {
    center_first: bool,
    pts: Vec<Vec2>,
}

impl EllipseM {
    /// Centre, end of the first axis.
    fn axis(&self) -> Option<(Vec2, Vec2)> {
        match (self.center_first, self.pts.as_slice()) {
            (true, [c, e, ..]) => Some((*c, *e)),
            (false, [a, b, ..]) => Some(((*a + *b) * 0.5, *b)),
            _ => None,
        }
    }
    fn points(c: Vec2, e: Vec2, r2: f64) -> Vec<Vec2> {
        let u = e - c;
        let r1 = u.len();
        let Some(d) = u.normalized() else { return Vec::new() };
        let n = Vec2::new(-d.y, d.x);
        (0..=96)
            .map(|k| {
                let t = TAU * k as f64 / 96.0;
                c + d * (r1 * t.cos()) + n * (r2 * t.sin())
            })
            .collect()
    }
    fn make(&self, app: &mut SolveApp, r2: f64) -> Res {
        let (c, e) = self.axis().ok_or("Specify the axis first")?;
        let r1 = c.dist(e);
        if !(r2 > 1e-9 && r1 > 1e-9) {
            return Err("Value must be positive and nonzero.".into());
        }
        // The sketch's ellipse takes its major axis: the longer of the two.
        let (major, minor) = if r2 > r1 {
            let d = (e - c).normalized().ok_or("Bad axis")?;
            (c + Vec2::new(-d.y, d.x) * r2, r1)
        } else {
            (e, r2)
        };
        run(app, "sketch.ellipse", json!({"center": xy(c), "major": xy(major), "minor_radius": minor, "axes": false}))?;
        Ok(Step::Done)
    }
}

impl Machine for EllipseM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        match (self.center_first, self.pts.len()) {
            (false, 0) => Prompt::new("Specify axis endpoint of ellipse", Accept::POINT).kw(&["Center"]),
            (false, 1) => Prompt::new("Specify other endpoint of axis", Accept::POINT).base(self.pts.first().copied()),
            (true, 0) => Prompt::new("Specify center of ellipse", Accept::POINT),
            (true, 1) => Prompt::new("Specify endpoint of axis", Accept::POINT).base(self.pts.first().copied()),
            _ => Prompt::new("Specify distance to other axis", Accept::POINT_OR_NUMBER).base(self.axis().map(|a| a.0)),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match i {
            Input::Keyword("Center") if self.pts.is_empty() => {
                self.center_first = true;
                Ok(Step::Continue)
            }
            Input::Point { p, .. } if self.pts.len() < 2 => {
                self.pts.push(p);
                Ok(Step::Continue)
            }
            Input::Point { p, .. } => {
                let (c, e) = self.axis().ok_or("Specify the axis first")?;
                let d = (e - c).normalized().ok_or("Bad axis")?;
                self.make(app, (p - c).cross(d).abs())
            }
            Input::Number(v) if self.pts.len() >= 2 => self.make(app, v),
            _ => Err(bad_input()),
        }
    }
    fn preview(&self, _: &SolveApp, c: Vec2) -> Vec<Prev> {
        match (self.axis(), self.pts.len()) {
            (Some((o, e)), 2) => {
                let Some(d) = (e - o).normalized() else { return Vec::new() };
                vec![Prev::Line(Self::points(o, e, (c - o).cross(d).abs())), Prev::Line(vec![o, c])]
            }
            (_, 1) => vec![Prev::Line(vec![self.pts[0], c])],
            _ => Vec::new(),
        }
    }
}

// ---- SPLINE ---------------------------------------------------------------------------------

#[derive(Default)]
struct SplineM {
    cv: bool,
    pts: Vec<Vec2>,
}

impl SplineM {
    fn make(&self, app: &mut SolveApp, periodic: bool) -> Res {
        if self.pts.len() < 2 {
            return Ok(Step::Done);
        }
        let pts: Vec<_> = self.pts.iter().map(|p| xy(*p)).collect();
        if self.cv {
            run(app, "sketch.spline.control_point", json!({"points": pts}))?;
        } else {
            run(app, "sketch.spline.fit_point", json!({"points": pts, "periodic": periodic && self.pts.len() >= 3}))?;
        }
        Ok(Step::Done)
    }
}

impl Machine for SplineM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        let base = self.pts.last().copied();
        match self.pts.len() {
            0 => Prompt::new(if self.cv { "Specify first control point" } else { "Specify first point" }, Accept::POINT).kw(&["Method"]),
            1 | 2 => Prompt::new("Enter next point", Accept::POINT).kw(&["Undo"]).base(base),
            _ => Prompt::new("Enter next point", Accept::POINT).kw(&["Close", "Undo"]).base(base),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match i {
            Input::Keyword("Method") => {
                self.cv = !self.cv;
                Ok(Step::Continue)
            }
            Input::Keyword("Undo") => {
                self.pts.pop();
                Ok(Step::Continue)
            }
            Input::Keyword("Close") => self.make(app, true),
            Input::Point { p, .. } => {
                self.pts.push(p);
                Ok(Step::Continue)
            }
            Input::Enter => self.make(app, false),
            _ => Err(bad_input()),
        }
    }
    fn preview(&self, _: &SolveApp, c: Vec2) -> Vec<Prev> {
        let mut v = self.pts.clone();
        if v.is_empty() {
            return Vec::new();
        }
        v.push(c);
        vec![Prev::Line(catmull(&v))]
    }
}

/// A smooth curve through `p` for the preview (Catmull-Rom).
fn catmull(p: &[Vec2]) -> Vec<Vec2> {
    if p.len() < 3 {
        return p.to_vec();
    }
    let mut out = Vec::new();
    for i in 0..p.len() - 1 {
        let p0 = if i == 0 { p[0] } else { p[i - 1] };
        let (p1, p2) = (p[i], p[i + 1]);
        let p3 = p.get(i + 2).copied().unwrap_or(p2);
        for k in 0..12 {
            let t = k as f64 / 12.0;
            let (t2, t3) = (t * t, t * t * t);
            out.push((p1 * 2.0 + (p2 - p0) * t + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2 + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3) * 0.5);
        }
    }
    out.push(p[p.len() - 1]);
    out
}

// ---- XLINE ----------------------------------------------------------------------------------

#[derive(Default)]
struct XlineM {
    /// 0 two points, 1 horizontal, 2 vertical, 3 at an angle (asking it), 4 at the angle.
    mode: u8,
    angle: f64,
    first: Option<Vec2>,
}

fn add_xline(app: &mut SolveApp, a: Vec2, dir: Vec2) -> Result<(), String> {
    let d = dir.normalized().ok_or("The points coincide")?;
    let v = run(app, "sketch.line", json!({"points": [xy(a), xy(a + d * 10.0)], "construction": true, "connect": false}))?;
    let ids = made(&v);
    if !ids.is_empty() {
        let _ = app.run("sketch.infinite", json!({"curves": ids, "value": true}));
    }
    Ok(())
}

impl Machine for XlineM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        match (self.mode, self.first) {
            (3, _) => Prompt::new("Enter angle of xline", Accept::NUMBER).default("0"),
            (0, None) => Prompt::new("Specify a point", Accept::POINT).kw(&["Hor", "Ver", "Ang"]),
            (0, Some(a)) => Prompt::new("Specify through point", Accept::POINT).base(Some(a)),
            _ => Prompt::new("Specify through point", Accept::POINT),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match i {
            Input::Keyword("Hor") => self.mode = 1,
            Input::Keyword("Ver") => self.mode = 2,
            Input::Keyword("Ang") => self.mode = 3,
            Input::Number(v) if self.mode == 3 => {
                self.angle = v.to_radians();
                self.mode = 4;
            }
            Input::Point { p, .. } => match (self.mode, self.first) {
                (0, None) => self.first = Some(p),
                (0, Some(a)) => add_xline(app, a, p - a)?,
                (1, _) => add_xline(app, p, Vec2::X)?,
                (2, _) => add_xline(app, p, Vec2::Y)?,
                (4, _) => add_xline(app, p, polar(self.angle))?,
                _ => return Err(bad_input()),
            },
            Input::Enter => return Ok(Step::Done),
            _ => return Err(bad_input()),
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _: &SolveApp, c: Vec2) -> Vec<Prev> {
        let far = 1.0e4;
        let line = |a: Vec2, d: Vec2| d.normalized().map(|d| vec![Prev::Line(vec![a - d * far, a + d * far])]).unwrap_or_default();
        match (self.mode, self.first) {
            (0, Some(a)) => line(a, c - a),
            (1, _) => line(c, Vec2::X),
            (2, _) => line(c, Vec2::Y),
            (4, _) => line(c, polar(self.angle)),
            _ => Vec::new(),
        }
    }
}

// ---- POINT ----------------------------------------------------------------------------------

struct PointM;

impl Machine for PointM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        Prompt::new("Specify a point", Accept::POINT)
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match i {
            Input::Point { p, .. } => {
                run(app, "sketch.point", json!({"point": xy(p)}))?;
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err(bad_input()),
        }
    }
}

// ---- DONUT ----------------------------------------------------------------------------------

#[derive(Default)]
struct DonutM {
    stage: u8,
}

impl Machine for DonutM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        let (i, o) = DONUT.with(Cell::get);
        match self.stage {
            0 => Prompt::new("Specify inside diameter of donut", Accept::NUMBER).default(geo::fmt(i)),
            1 => Prompt::new("Specify outside diameter of donut", Accept::NUMBER).default(geo::fmt(o)),
            _ => Prompt::new("Specify center of donut", Accept::POINT),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        let (di, dout) = DONUT.with(Cell::get);
        match (self.stage, i) {
            (0, Input::Number(v)) if v >= 0.0 => DONUT.with(|d| d.set((v, dout))),
            (1, Input::Number(v)) if v > 0.0 => DONUT.with(|d| d.set((di, v))),
            (0 | 1, Input::Enter) => {}
            (2, Input::Point { p, .. }) => {
                let (di, dout) = DONUT.with(Cell::get);
                if di > 1e-9 {
                    add_circle(app, p, di / 2.0)?;
                }
                add_circle(app, p, dout / 2.0)?;
                return Ok(Step::Continue);
            }
            (2, Input::Enter) => return Ok(Step::Done),
            _ => return Err("Requires a positive number.".into()),
        }
        self.stage += 1;
        Ok(Step::Continue)
    }
    fn preview(&self, _: &SolveApp, c: Vec2) -> Vec<Prev> {
        if self.stage < 2 {
            return Vec::new();
        }
        let (di, dout) = DONUT.with(Cell::get);
        vec![Prev::Line(arc_points(c, di / 2.0, 0.0, TAU)), Prev::Line(arc_points(c, dout / 2.0, 0.0, TAU))]
    }
}

// ---- REVCLOUD -------------------------------------------------------------------------------

#[derive(Default)]
struct CloudM {
    pts: Vec<Vec2>,
    asking: bool,
}

/// The arcs of a revision cloud along the closed outline `pts`: (start, middle, end) each.
fn cloud_arcs(pts: &[Vec2], arc_len: f64) -> Vec<(Vec2, Vec2, Vec2)> {
    let ccw = geo::signed_area(pts) > 0.0;
    let mut out = Vec::new();
    for k in 0..pts.len() {
        let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
        let len = a.dist(b);
        let Some(d) = (b - a).normalized() else { continue };
        // Bulges out of the outline: to the right of a counter-clockwise outline.
        let out_n = if ccw { Vec2::new(d.y, -d.x) } else { Vec2::new(-d.y, d.x) };
        let n = ((len / arc_len.max(1e-6)).round() as usize).max(1);
        for i in 0..n {
            let s = a + (b - a) * (i as f64 / n as f64);
            let e = a + (b - a) * ((i + 1) as f64 / n as f64);
            let m = (s + e) * 0.5 + out_n * (s.dist(e) * 0.3);
            out.push((s, m, e));
        }
    }
    out
}

impl Machine for CloudM {
    fn prompt(&self, _: &SolveApp) -> Prompt {
        if self.asking {
            return Prompt::new("Specify length of arc", Accept::NUMBER).default(geo::fmt(CLOUD_ARC.with(Cell::get)));
        }
        match self.pts.len() {
            0 => Prompt::new("Specify first corner of the cloud", Accept::POINT).kw(&["Arc length"]),
            1 | 2 => Prompt::new("Specify next corner", Accept::POINT).kw(&["Undo"]).base(self.pts.last().copied()),
            _ => Prompt::new("Specify next corner, Enter closes the cloud", Accept::POINT).kw(&["Undo"]).base(self.pts.last().copied()),
        }
    }
    fn input(&mut self, app: &mut SolveApp, i: Input) -> Res {
        match i {
            Input::Number(v) if self.asking && v > 0.0 => {
                CLOUD_ARC.with(|c| c.set(v));
                self.asking = false;
            }
            Input::Enter if self.asking => self.asking = false,
            Input::Keyword("Arc length") => self.asking = true,
            Input::Keyword("Undo") => {
                self.pts.pop();
            }
            Input::Point { p, .. } => self.pts.push(p),
            Input::Enter if self.pts.len() >= 3 => {
                for (s, m, e) in cloud_arcs(&self.pts, CLOUD_ARC.with(Cell::get)) {
                    add_arc3(app, s, m, e)?;
                }
                return Ok(Step::Done);
            }
            Input::Enter => return Ok(Step::Done),
            _ => return Err(bad_input()),
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _: &SolveApp, c: Vec2) -> Vec<Prev> {
        if self.pts.is_empty() {
            return Vec::new();
        }
        let mut v = self.pts.clone();
        v.push(c);
        if v.len() < 3 {
            return vec![Prev::Line(v)];
        }
        cloud_arcs(&v, CLOUD_ARC.with(Cell::get)).into_iter().filter_map(|(s, m, e)| arc3_points(s, m, e)).map(Prev::Line).collect()
    }
}
