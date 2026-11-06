//! Plane geometry for the sketch tools: the sketch's curves as simple shapes, and the arc, circle
//! and polyline helpers the tools' previews and snaps need.

use solidcore_record::Vec2;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::{CurveKind, Sketch};

use std::f64::consts::TAU;

/// A sketch curve as the tools see it.
#[derive(Clone, Debug)]
pub enum Shape {
    Line(Vec2, Vec2),
    /// Counter-clockwise from `a0` to `a1` (radians; `a1 > a0`).
    Arc {
        c: Vec2,
        r: f64,
        a0: f64,
        a1: f64,
    },
    Circle(Vec2, f64),
    /// Anything else (ellipse, spline, conic): its polyline.
    Poly(Vec<Vec2>),
}

/// One curve of a sketch: its id, shape and whether it is construction geometry.
#[derive(Clone, Debug)]
pub struct Ent {
    pub id: String,
    pub shape: Shape,
    pub construction: bool,
}

/// The curves of `sk` the tools work with (text outlines and projected curves included).
pub fn ents(sk: &Sketch) -> Vec<Ent> {
    let mut out = Vec::new();
    for (i, c) in sk.curves.iter().enumerate() {
        if !solidcraft_engine::view::sketch_curve_visible(sk, i) {
            continue;
        }
        let pt = |k: usize| sk.point(k);
        let shape = match &c.kind {
            CurveKind::Line { a, b } => match (pt(*a), pt(*b)) {
                (Some(a), Some(b)) => Shape::Line(a, b),
                _ => continue,
            },
            CurveKind::Circle { c: cc, r } => match pt(*cc) {
                Some(cc) => Shape::Circle(cc, *r),
                None => continue,
            },
            CurveKind::Arc { c: cc, a, b } => match (pt(*cc), pt(*a), pt(*b)) {
                (Some(cc), Some(a), Some(b)) => {
                    let r = a.dist(cc);
                    let a0 = (a - cc).angle();
                    let mut a1 = (b - cc).angle();
                    while a1 <= a0 {
                        a1 += TAU;
                    }
                    Shape::Arc { c: cc, r, a0, a1 }
                }
                _ => continue,
            },
            _ => {
                let p = sk.polyline(i);
                if p.len() < 2 {
                    continue;
                }
                Shape::Poly(p)
            }
        };
        out.push(Ent { id: c.id.clone(), shape, construction: c.construction || c.centerline || c.infinite });
    }
    out
}

pub fn polar(a: f64) -> Vec2 {
    Vec2::new(a.cos(), a.sin())
}

impl Shape {
    /// Points along the shape, for drawing and hit tests.
    pub fn polyline(&self) -> Vec<Vec2> {
        match self {
            Shape::Line(a, b) => vec![*a, *b],
            Shape::Arc { c, r, a0, a1 } => arc_points(*c, *r, *a0, *a1),
            Shape::Circle(c, r) => arc_points(*c, *r, 0.0, TAU),
            Shape::Poly(p) => p.clone(),
        }
    }
    /// End points (none for a circle).
    pub fn ends(&self) -> Vec<Vec2> {
        match self {
            Shape::Line(a, b) => vec![*a, *b],
            Shape::Arc { c, r, a0, a1 } => vec![*c + polar(*a0) * *r, *c + polar(*a1) * *r],
            Shape::Circle(..) => Vec::new(),
            Shape::Poly(p) => {
                let (Some(f), Some(l)) = (p.first(), p.last()) else { return Vec::new() };
                if f.dist(*l) < 1e-9 { Vec::new() } else { vec![*f, *l] }
            }
        }
    }
    /// Mid points of lines and arcs.
    pub fn mid(&self) -> Option<Vec2> {
        match self {
            Shape::Line(a, b) => Some((*a + *b) * 0.5),
            Shape::Arc { c, r, a0, a1 } => Some(*c + polar((a0 + a1) * 0.5) * *r),
            _ => None,
        }
    }
    /// The nearest point of the shape to `p`.
    pub fn nearest(&self, p: Vec2) -> Option<Vec2> {
        match self {
            Shape::Line(a, b) => Some(nearest_on_segment(*a, *b, p)),
            Shape::Circle(c, r) => (p - *c).normalized().map(|d| *c + d * *r),
            Shape::Arc { c, r, a0, a1 } => {
                let d = (p - *c).normalized()?;
                let a = norm_angle_from(d.angle(), *a0);
                if a <= *a1 { Some(*c + d * *r) } else { self.ends().into_iter().min_by(|x, y| x.dist(p).total_cmp(&y.dist(p))) }
            }
            Shape::Poly(pts) => nearest_on_polyline(pts, p),
        }
    }
    pub fn dist(&self, p: Vec2) -> f64 {
        self.nearest(p).map_or(f64::MAX, |q| q.dist(p))
    }
}

/// `a` turned to the first angle at or after `from`.
pub fn norm_angle_from(a: f64, from: f64) -> f64 {
    let mut x = a;
    while x < from {
        x += TAU;
    }
    while x >= from + TAU {
        x -= TAU;
    }
    x
}

pub fn arc_points(c: Vec2, r: f64, a0: f64, a1: f64) -> Vec<Vec2> {
    let n = (((a1 - a0).abs() / (TAU / 96.0)).ceil() as usize).clamp(2, 400);
    (0..=n).map(|i| c + polar(a0 + (a1 - a0) * i as f64 / n as f64) * r).collect()
}

pub fn nearest_on_segment(a: Vec2, b: Vec2, p: Vec2) -> Vec2 {
    let d = b - a;
    let l2 = d.len2();
    if l2 < 1e-24 {
        return a;
    }
    a + d * ((p - a).dot(d) / l2).clamp(0.0, 1.0)
}

pub fn nearest_on_polyline(pts: &[Vec2], p: Vec2) -> Option<Vec2> {
    pts.windows(2).map(|w| nearest_on_segment(w[0], w[1], p)).min_by(|x, y| x.dist(p).total_cmp(&y.dist(p)))
}

/// The circle through three points: centre and radius.
pub fn circle3(a: Vec2, b: Vec2, c: Vec2) -> Option<(Vec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.len2(), b.len2(), c.len2());
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let o = Vec2::new(ux, uy);
    Some((o, o.dist(a)))
}

/// The arc from `s` through `m` to `e`, as points.
pub fn arc3_points(s: Vec2, m: Vec2, e: Vec2) -> Option<Vec<Vec2>> {
    let (c, r) = circle3(s, m, e)?;
    let a0 = (s - c).angle();
    let am = norm_angle_from((m - c).angle(), a0);
    let ae = norm_angle_from((e - c).angle(), a0);
    Some(if am <= ae { arc_points(c, r, a0, ae) } else { arc_points(c, r, ae, a0 + TAU).into_iter().rev().collect() })
}

/// The point on the arc of `(c, r)` running counter-clockwise from `a0` to `a1` halfway along it.
pub fn arc_mid(c: Vec2, r: f64, a0: f64, a1: f64) -> Vec2 {
    c + polar((a0 + norm_angle_from(a1, a0)) * 0.5) * r
}

/// The arc starting at `s` with tangent direction `dir` that ends at `e`: its mid point (for a
/// 3-point arc), or none when `e` lies on the tangent line.
pub fn tangent_arc_mid(s: Vec2, dir: Vec2, e: Vec2) -> Option<Vec2> {
    let dir = dir.normalized()?;
    let chord = e - s;
    let n = Vec2::new(-dir.y, dir.x);
    let den = 2.0 * chord.dot(n);
    if den.abs() < 1e-12 {
        return None;
    }
    let r = chord.len2() / den;
    let c = s + n * r;
    let rr = r.abs();
    // The arc runs from s the way `dir` points: counter-clockwise when the centre is on the left.
    let a0 = (s - c).angle();
    let a1 = (e - c).angle();
    let mid = if r > 0.0 { arc_mid(c, rr, a0, a1) } else { arc_mid(c, rr, a1, a0) };
    Some(mid)
}

/// Signed area of a closed polygon (positive: counter-clockwise).
pub fn signed_area(p: &[Vec2]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i].cross(p[(i + 1) % n])).sum::<f64>() * 0.5
}

/// Where two shapes cross (lines as segments, arcs and circles), from their polylines.
pub fn crossings(a: &Shape, b: &Shape) -> Vec<Vec2> {
    let (pa, pb) = (a.polyline(), b.polyline());
    let mut out = Vec::new();
    for s in pa.windows(2) {
        for t in pb.windows(2) {
            if let Some(x) = seg_cross(s[0], s[1], t[0], t[1]) {
                out.push(x);
            }
        }
    }
    // Lines meet exactly: refine line/line crossings.
    if let (Shape::Line(a0, a1), Shape::Line(b0, b1)) = (a, b) {
        return seg_cross(*a0, *a1, *b0, *b1).into_iter().collect();
    }
    out
}

pub fn seg_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<Vec2> {
    let r = b - a;
    let s = d - c;
    let den = r.cross(s);
    if den.abs() < 1e-14 {
        return None;
    }
    let t = (c - a).cross(s) / den;
    let u = (c - a).cross(r) / den;
    ((-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u)).then(|| a + r * t)
}

/// Where the infinite lines through `a, b` and `c, d` cross.
pub fn line_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<Vec2> {
    let r = b - a;
    let s = d - c;
    let den = r.cross(s);
    if den.abs() < 1e-14 {
        return None;
    }
    Some(a + r * ((c - a).cross(s) / den))
}

/// A number typed by the user ("12.5", "-3", "1/4", "2*3").
pub fn number(t: &str) -> Option<f64> {
    let t = t.trim();
    if let Some((a, b)) = t.split_once('/') {
        let (a, b) = (number(a)?, number(b)?);
        return (b != 0.0).then(|| a / b).filter(|v| v.is_finite());
    }
    if let Some((a, b)) = t.split_once('*') {
        return Some(number(a)? * number(b)?).filter(|v| v.is_finite());
    }
    t.replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

pub fn fmt(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}
