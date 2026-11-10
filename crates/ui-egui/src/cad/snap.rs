//! Object snaps for the sketch tools, after CADCraft's OSNAP: end, mid, centre, quadrant,
//! intersection, perpendicular and nearest points of the sketch's own curves, of the model's
//! edges and of the other shown sketches (both projected onto the sketch plane), then the grid.

use std::sync::Arc;

use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_engine::geom::prelude::*;

use super::geo::{Ent, Shape, crossings, polar};
use crate::SolveApp;
use crate::viewport::Proj;

pub const END: u8 = 1;
pub const MID: u8 = 2;
pub const CEN: u8 = 3;
pub const QUA: u8 = 4;
pub const INT: u8 = 5;
pub const PER: u8 = 6;
pub const NEA: u8 = 7;
pub const GRID: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub p: Vec2,
    pub kind: u8,
}

impl Hit {
    pub fn name(&self) -> &'static str {
        match self.kind {
            END => "Endpoint",
            MID => "Midpoint",
            CEN => "Center",
            QUA => "Quadrant",
            INT => "Intersection",
            PER => "Perpendicular",
            NEA => "Nearest",
            _ => "Grid",
        }
    }
}

/// Sketch millimetres per screen point at `p` (for the snap aperture).
pub fn mm_per_px(plane: &Plane, proj: &Proj, p: Vec2) -> f64 {
    let a = proj.to_screen(plane.to_world(p));
    let b = proj.to_screen(plane.to_world(p + Vec2::new(1.0, 0.0)));
    let c = proj.to_screen(plane.to_world(p + Vec2::new(0.0, 1.0)));
    match (a, b, c) {
        (Some(a), Some(b), Some(c)) => {
            let px = a.distance(b).max(a.distance(c)) as f64;
            if px > 1e-9 { 1.0 / px } else { 1.0 }
        }
        _ => 1.0,
    }
}

thread_local! {
    /// The model's edges and the other sketches' curves on the open sketch's plane, kept while
    /// the model, the sketch and its plane stay the same.
    static FOREIGN: std::cell::RefCell<Option<(u64, usize, [u64; 9], Arc<Vec<Shape>>)>> = const { std::cell::RefCell::new(None) };
}

fn plane_key(p: &Plane) -> [u64; 9] {
    [p.origin.x, p.origin.y, p.origin.z, p.x.x, p.x.y, p.x.z, p.y.x, p.y.y, p.y.z].map(f64::to_bits)
}

/// Fit a circle to a closed or open polyline when all its points lie on one: centre, radius.
fn fit_circle(pts: &[Vec2]) -> Option<(Vec2, f64)> {
    if pts.len() < 4 {
        return None;
    }
    let (a, m, b) = (*pts.first()?, *pts.get(pts.len() / 3)?, *pts.get(2 * pts.len() / 3)?);
    let (c, r) = super::geo::circle3(a, m, b)?;
    let tol = (r * 1e-3).max(1e-4);
    pts.iter().all(|p| (p.dist(c) - r).abs() <= tol).then_some((c, r))
}

/// The model's edges and the other shown sketches' curves, on the open sketch's plane.
pub fn foreign(app: &SolveApp, sid: u64, plane: &Plane) -> Arc<Vec<Shape>> {
    let st = crate::open_meshing::shown_world(app);
    let key = (sid, Arc::as_ptr(&st) as usize, plane_key(plane));
    if let Some(found) = FOREIGN.with(|c| c.borrow().as_ref().filter(|k| (k.0, k.1, k.2) == key).map(|k| k.3.clone())) {
        return found;
    }
    let flat = |p: Vec3| plane.to_local(p);
    let mut out: Vec<Shape> = Vec::new();
    for b in st.bodies.iter().filter(|b| app.body_visible(&b.name)) {
        let (m, place) = b.mesh_in_place();
        let moved = !solidcraft_engine::doc::is_identity(&place);
        for e in m.edges.iter().take(20_000) {
            let pts: Vec<Vec2> = e.iter().map(|p| if moved { solidcraft_engine::doc::apply_point(&place, *p) } else { *p }).map(flat).collect();
            if pts.len() < 2 {
                continue;
            }
            if pts.len() == 2 {
                out.push(Shape::Line(pts[0], pts[1]));
                continue;
            }
            if let Some((c, r)) = fit_circle(&pts) {
                let (f, l) = (pts[0], pts[pts.len() - 1]);
                if f.dist(l) < r * 1e-3 {
                    out.push(Shape::Circle(c, r));
                } else {
                    let a0 = (f - c).angle();
                    // Which way round: by the point a third along.
                    let mid = super::geo::norm_angle_from((pts[pts.len() / 3] - c).angle(), a0);
                    let a1 = super::geo::norm_angle_from((l - c).angle(), a0);
                    if mid <= a1 {
                        out.push(Shape::Arc { c, r, a0, a1 });
                    } else {
                        let b0 = (l - c).angle();
                        out.push(Shape::Arc { c, r, a0: b0, a1: super::geo::norm_angle_from(a0, b0) });
                    }
                }
                continue;
            }
            out.push(Shape::Poly(pts));
        }
    }
    for other in crate::viewport::visible_sketches(app).into_iter().filter(|s| *s != sid) {
        let Some(ss) = st.sketch(other) else { continue };
        for e in super::geo::ents(&ss.sketch) {
            let pts: Vec<Vec2> = e.shape.polyline().into_iter().map(|p| flat(ss.plane.to_world(p))).collect();
            match pts.len() {
                0 | 1 => {}
                2 => out.push(Shape::Line(pts[0], pts[1])),
                _ => out.push(Shape::Poly(pts)),
            }
        }
    }
    let out = Arc::new(out);
    FOREIGN.with(|c| *c.borrow_mut() = Some((key.0, key.1, key.2, out.clone())));
    out
}

/// The snap a cursor at `cursor` (sketch coordinates) takes: within `aperture` (sketch mm) of a
/// curve's special point, its nearest point last. `base`: the point a line is drawn from (for
/// Perpendicular).
/// Every object snap mode but Nearest (as a bit per kind).
pub const DEFAULT_MODES: u16 = (1 << END) | (1 << MID) | (1 << CEN) | (1 << QUA) | (1 << INT) | (1 << PER);

/// The modes the Drafting Settings list, in order: (kind, name).
pub const MODES: [(u8, &str); 7] =
    [(END, "Endpoint"), (MID, "Midpoint"), (CEN, "Center"), (QUA, "Quadrant"), (INT, "Intersection"), (PER, "Perpendicular"), (NEA, "Nearest")];

#[cfg(test)]
pub fn find(own: &[Ent], foreign: &[Shape], cursor: Vec2, aperture: f64, base: Option<Vec2>, nearest: bool) -> Option<Hit> {
    find_modes(own, foreign, cursor, aperture, base, DEFAULT_MODES | if nearest { 1 << NEA } else { 0 })
}

/// The best snap among the running modes (`modes`: a bit per kind).
pub fn find_modes(own: &[Ent], foreign: &[Shape], cursor: Vec2, aperture: f64, base: Option<Vec2>, modes: u16) -> Option<Hit> {
    let nearest = modes & (1 << NEA) != 0;
    let near: Vec<&Shape> = own
        .iter()
        .map(|e| &e.shape)
        .chain(foreign.iter())
        .filter(|s| s.dist(cursor) <= aperture * 2.5 || matches!(s, Shape::Circle(c, _) | Shape::Arc { c, .. } if c.dist(cursor) <= aperture))
        .take(400)
        .collect();
    let mut cands: Vec<(f64, Hit)> = Vec::new();
    let offer_into = |cands: &mut Vec<(f64, Hit)>, p: Vec2, kind: u8, rank: f64| {
        let d = p.dist(cursor);
        if d <= aperture && p.is_finite() && modes & (1u16 << kind) != 0 {
            cands.push((d / aperture + rank, Hit { p, kind }));
        }
    };
    macro_rules! offer {
        ($p:expr, $k:expr, $r:expr) => {
            offer_into(&mut cands, $p, $k, $r)
        };
    }
    let mut centres: Vec<Vec2> = Vec::new();
    // The sketch origin is an end point too.
    offer!(Vec2::ZERO, END, 0.0);
    for s in &near {
        for e in s.ends() {
            offer!(e, END, 0.0);
        }
        if let Some(m) = s.mid() {
            offer!(m, MID, 0.05);
        }
        match s {
            Shape::Circle(c, r) | Shape::Arc { c, r, .. } => {
                // Centre: when the cursor is at the centre, or on the curve itself (AutoCAD shows
                // the centre while the cursor is over the circle).
                if s.dist(cursor) <= aperture && c.dist(cursor) > aperture {
                    centres.push(*c);
                }
                offer!(*c, CEN, 0.0);
                for k in 0..4 {
                    let q = *c + polar(k as f64 * std::f64::consts::FRAC_PI_2) * *r;
                    if s.dist(q) < r * 1e-6 + 1e-9 {
                        offer!(q, QUA, 0.1);
                    }
                }
            }
            _ => {}
        }
        if let Some(b) = base {
            let foot = match s {
                Shape::Line(a, c) => {
                    let d = *c - *a;
                    (d.len2() > 1e-18).then(|| *a + d * ((b - *a).dot(d) / d.len2()))
                }
                Shape::Circle(c, r) | Shape::Arc { c, r, .. } => (b - *c).normalized().map(|u| *c + u * *r),
                _ => None,
            };
            if let Some(f) = foot.filter(|f| s.dist(*f) < 1e-6 && f.dist(b) > 1e-9) {
                offer!(f, PER, 0.2);
            }
        }
    }
    for i in 0..near.len() {
        for j in i + 1..near.len() {
            for x in crossings(near[i], near[j]) {
                offer!(x, INT, 0.0);
            }
        }
    }
    let none_yet = cands.is_empty();
    if none_yet && nearest {
        for s in &near {
            if let Some(q) = s.nearest(cursor) {
                offer!(q, NEA, 0.5);
            }
        }
    }
    // Over a circle or arc, its centre (as CADCraft shows it), unless a closer snap is there.
    if modes & (1 << CEN) != 0 {
        cands.extend(centres.into_iter().map(|c| (0.6, Hit { p: c, kind: CEN })));
    }
    cands.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, h)| h)
}

pub fn grid(p: Vec2, step: f64) -> Vec2 {
    if !(step.is_finite() && step > 0.0) {
        return p;
    }
    Vec2::new((p.x / step).round() * step, (p.y / step).round() * step)
}

/// CADCraft's snap markers: a square at an end, a triangle at a middle, a circle at a centre…
pub fn paint_marker(p: &egui::Painter, at: egui::Pos2, hit: &Hit, color: egui::Color32) {
    if !paint_glyph(p, at, hit, color) {
        return;
    }
    let vec2 = egui::vec2;
    let galley = p.layout_no_wrap(hit.name().to_string(), egui::FontId::proportional(11.0), egui::Color32::BLACK);
    let r = egui::Rect::from_min_size(at + vec2(10.0, 10.0), galley.size() + vec2(6.0, 2.0));
    p.rect_filled(r, 2.0, egui::Color32::from_rgb(0xff, 0xff, 0xe1));
    p.galley(r.min + vec2(3.0, 1.0), galley, egui::Color32::BLACK);
}

/// A snap's glyph alone, without its name (the Drafting Settings list has the name beside it).
/// False for the grid's cross, which has no name label.
pub fn paint_glyph(p: &egui::Painter, at: egui::Pos2, hit: &Hit, color: egui::Color32) -> bool {
    use egui::{Shape as S, Stroke, vec2};
    let s = 5.5;
    let st = Stroke::new(2.0, color);
    match hit.kind {
        END => {
            p.rect_stroke(egui::Rect::from_center_size(at, vec2(2.0 * s, 2.0 * s)), 0.0, st, egui::StrokeKind::Middle);
        }
        MID => {
            p.add(S::closed_line(vec![at + vec2(0.0, -s), at + vec2(s, s * 0.8), at + vec2(-s, s * 0.8)], st));
        }
        CEN => {
            p.circle_stroke(at, s, st);
        }
        QUA => {
            p.add(S::closed_line(vec![at + vec2(0.0, -s), at + vec2(s, 0.0), at + vec2(0.0, s), at + vec2(-s, 0.0)], st));
        }
        INT => {
            p.line_segment([at + vec2(-s, -s), at + vec2(s, s)], st);
            p.line_segment([at + vec2(s, -s), at + vec2(-s, s)], st);
        }
        PER => {
            p.line(vec![at + vec2(-s, -s), at + vec2(-s, s), at + vec2(s, s)], st);
            p.line(vec![at + vec2(-s, 0.0), at + vec2(0.0, 0.0), at + vec2(0.0, s)], st);
        }
        NEA => {
            p.line(vec![at + vec2(-s, -s), at + vec2(s, -s), at + vec2(-s, s), at + vec2(s, s), at + vec2(-s, -s)], st);
        }
        _ => {
            p.line_segment([at + vec2(-s * 0.6, 0.0), at + vec2(s * 0.6, 0.0)], Stroke::new(1.0, color));
            p.line_segment([at + vec2(0.0, -s * 0.6), at + vec2(0.0, s * 0.6)], Stroke::new(1.0, color));
            return false;
        }
    }
    true
}

thread_local! {
    /// The model's snap points in the world (corners, edges' middles, circles' centres), kept
    /// while the model stays the same.
    static WORLD_POINTS: std::cell::RefCell<Option<(usize, Arc<Vec<(Vec3, u8)>>)>> = const { std::cell::RefCell::new(None) };
}

/// The centre of a closed polyline when all its points lie on one circle.
fn circle_centre3(pts: &[Vec3]) -> Option<Vec3> {
    if pts.len() < 6 {
        return None;
    }
    let (f, l) = (*pts.first()?, *pts.last()?);
    let n = pts.len() as f64;
    let c = pts.iter().fold(Vec3::ZERO, |a, p| a + *p) * (1.0 / n);
    let r = f.dist(c);
    if r < 1e-9 || f.dist(l) > r * 1e-3 {
        return None;
    }
    let tol = (r * 2e-3).max(1e-4);
    pts.iter().all(|p| (p.dist(c) - r).abs() <= tol).then_some(c)
}

/// The middle of a polyline, half its length along it.
fn middle3(pts: &[Vec3]) -> Option<Vec3> {
    let total: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut left = total * 0.5;
    for w in pts.windows(2) {
        let d = w[0].dist(w[1]);
        if d >= left && d > 0.0 {
            return Some(w[0] + (w[1] - w[0]) * (left / d));
        }
        left -= d;
    }
    pts.first().copied()
}

/// The model's corners, edges' middles and circles' centres, and those of the shown sketches, in
/// the world: what the MODIFY tools snap to outside a sketch.
pub fn world_points(app: &SolveApp) -> Arc<Vec<(Vec3, u8)>> {
    let st = crate::open_meshing::shown_world(app);
    let key = Arc::as_ptr(&st) as usize;
    if let Some(found) = WORLD_POINTS.with(|c| c.borrow().as_ref().filter(|k| k.0 == key).map(|k| k.1.clone())) {
        return found;
    }
    let mut out: Vec<(Vec3, u8)> = Vec::new();
    let mut add = |pts: &[Vec3]| {
        if pts.len() < 2 {
            return;
        }
        if let Some(c) = circle_centre3(pts) {
            out.push((c, CEN));
            return;
        }
        if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
            out.push((*a, END));
            out.push((*b, END));
        }
        if let Some(m) = middle3(pts) {
            out.push((m, MID));
        }
    };
    for b in st.bodies.iter().filter(|b| app.body_visible(&b.name)) {
        let (m, place) = b.mesh_in_place();
        let moved = !solidcraft_engine::doc::is_identity(&place);
        for e in m.edges.iter().take(20_000) {
            let pts: Vec<Vec3> = e.iter().map(|p| if moved { solidcraft_engine::doc::apply_point(&place, *p) } else { *p }).collect();
            add(&pts);
        }
    }
    for id in crate::viewport::visible_sketches(app) {
        let Some(ss) = st.sketch(id) else { continue };
        for e in super::geo::ents(&ss.sketch).iter().take(20_000) {
            let pts: Vec<Vec3> = e.shape.polyline().into_iter().map(|p| ss.plane.to_world(p)).collect();
            add(&pts);
        }
    }
    out.truncate(200_000);
    let out = Arc::new(out);
    WORLD_POINTS.with(|c| *c.borrow_mut() = Some((key, out.clone())));
    out
}

/// The world snap point nearest the pointer on screen, within `aperture` points, of the kinds in
/// `modes`: what the pointer is over, wherever it lies off the work plane.
pub fn world_snap(app: &SolveApp, proj: &Proj, pos: egui::Pos2, aperture: f32, modes: u16) -> Option<(Vec3, u8)> {
    let mut best: Option<(f32, Vec3, u8)> = None;
    for (p, kind) in world_points(app).iter() {
        if modes & (1 << kind) == 0 {
            continue;
        }
        let Some(s) = proj.to_screen(*p) else { continue };
        let d = s.distance(pos);
        // Centres and middles come before the corners that sit almost on them.
        let d = if *kind == END { d } else { d + 0.5 };
        if d <= aperture && best.is_none_or(|b| d < b.0) {
            best = Some((d, *p, *kind));
        }
    }
    best.map(|(_, p, k)| (p, k))
}
