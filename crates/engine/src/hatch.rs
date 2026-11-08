//! Hatch patterns on faces: the patterns (read from `.pat` files or SolidCraft's own), which
//! face of which body carries which pattern at what scale and angle (kept in the design's
//! metadata under [`KEY`]), the pattern's lines laid on a face in the model, and the same lines
//! as a drawing sees them (a HATCH on a flat face, a block of lines on a curved one).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

/// The design metadata key holding the hatches.
pub const KEY: &str = "org.solidcraft.hatches";

/// Most lines a pattern may have, segments one face may get, and lines one family may lay.
const MAX_LINES: usize = 64;
const MAX_SEGMENTS: usize = 400_000;
const MAX_FAMILY_LINES: usize = 20_000;

/// One line family of a pattern, as a `.pat` file gives it: its angle (degrees), a point on its
/// first line, the step to the next line (along the line, then across it) and its dashes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatLine {
    pub angle: f64,
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    #[serde(default)]
    pub dashes: Vec<f64>,
}

/// A hatch pattern.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub lines: Vec<PatLine>,
}

/// A pattern on a face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hatch {
    pub body: String,
    pub face: usize,
    /// A point on the face, which finds it again when the body is rebuilt.
    pub at: [f64; 3],
    pub pattern: Pattern,
    pub scale: f64,
    /// Degrees, added to the pattern's own angles.
    pub angle: f64,
}

/// Read the patterns of a `.pat` file: `*NAME, description` then a line per family,
/// `angle, x, y, dx, dy[, dash, …]`; `;` starts a comment. Malformed lines are skipped.
pub fn parse_pat(text: &str) -> Vec<Pattern> {
    let mut out: Vec<Pattern> = Vec::new();
    for raw in text.lines().take(200_000) {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(head) = line.strip_prefix('*') {
            let (name, description) = head.split_once(',').unwrap_or((head, ""));
            let name = name.trim();
            if !name.is_empty() && out.len() < 10_000 {
                out.push(Pattern {
                    name: name.chars().take(64).collect(),
                    description: description.trim().chars().take(200).collect(),
                    lines: Vec::new(),
                });
            }
            continue;
        }
        let Some(p) = out.last_mut() else { continue };
        let nums: Vec<f64> = line.split(',').map(|v| v.trim().parse::<f64>()).collect::<std::result::Result<Vec<_>, _>>().unwrap_or_default();
        if nums.len() < 5 || p.lines.len() >= MAX_LINES || nums.iter().any(|v| !v.is_finite()) {
            continue;
        }
        let g = |i: usize| nums.get(i).copied().unwrap_or(0.0);
        p.lines.push(PatLine { angle: g(0), x: g(1), y: g(2), dx: g(3), dy: g(4), dashes: nums.iter().skip(5).take(32).copied().collect() });
    }
    out.retain(|p| !p.lines.is_empty() && p.lines.iter().all(|l| l.dy.abs() > 1e-9));
    out
}

/// SolidCraft's own patterns, always available.
pub fn builtin() -> Vec<Pattern> {
    let text = "\
*SC_LINES, Diagonal lines
45, 0,0, 0,3
*SC_CROSS, Crossed diagonal lines
45, 0,0, 0,3
135, 0,0, 0,3
*SC_GRID, Square grid
0, 0,0, 0,4
90, 0,0, 0,4
*SC_BRICK, Running bond brickwork
0, 0,0, 0,5
90, 0,0, 5,10, 5,-5
90, 5,0, 5,10, -5,5
*SC_DASH, Dashed diagonal lines
45, 0,0, 2,3, 3,-1.5
*SC_DOTS, Dots on a square grid
0, 0,0, 0,3, 0,-3
*SC_CONCRETE, Short strokes of two directions
30, 0,0, 3,5, 1.5,-4.5
120, 1,1, 4,6, 1,-5
";
    parse_pat(text)
}

/// The hatches kept in the design.
pub fn hatches(meta: &solidcraft_doc::Metadata) -> Vec<Hatch> {
    meta.0.get(KEY).and_then(|v| v.get("hatches")).and_then(|v| serde_json::from_value::<Vec<Hatch>>(v.clone()).ok()).unwrap_or_default()
}

/// The metadata value: `{version: 1, hatches: [...]}`.
pub fn to_value(h: &[Hatch]) -> Value {
    json!({"version": 1, "hatches": serde_json::to_value(h).unwrap_or_else(|_| json!([]))})
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

/// The triangles of face `face`.
fn face_tris(mesh: &Mesh, face: usize) -> Vec<[Vec3; 3]> {
    mesh.triangles
        .iter()
        .zip(&mesh.tri_face)
        .filter(|(_, f)| **f as usize == face)
        .filter_map(|(t, _)| {
            let p = |i: u32| mesh.positions.get(i as usize).copied();
            Some([p(t[0])?, p(t[1])?, p(t[2])?])
        })
        .collect()
}

fn closest_on_tri(p: Vec3, t: &[Vec3; 3]) -> Vec3 {
    // Real-Time Collision Detection, closest point on a triangle.
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// The face the hatch is on: its own face when the point is still on it, else the face nearest
/// the point (the body was rebuilt and its faces numbered anew).
pub fn resolve_face(mesh: &Mesh, h: &Hatch) -> Option<usize> {
    let at = v3(h.at);
    let size = mesh.positions.iter().fold(0.0f64, |m, p| m.max(p.len()));
    let tol = (size * 1e-6).max(1e-6);
    let dist = |f: usize| face_tris(mesh, f).iter().map(|t| (closest_on_tri(at, t) - at).len()).fold(f64::INFINITY, f64::min);
    if dist(h.face) <= tol {
        return Some(h.face);
    }
    // One pass over every triangle: the face of the nearest.
    let mut best: Option<(usize, f64)> = None;
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let p = |i: u32| mesh.positions.get(i as usize).copied();
        let (Some(a), Some(b), Some(c)) = (p(t[0]), p(t[1]), p(t[2])) else { continue };
        let d = (closest_on_tri(at, &[a, b, c]) - at).len();
        if d.is_finite() && best.is_none_or(|(_, e)| d < e) {
            best = Some((*f as usize, d));
        }
    }
    best.map(|(f, _)| f)
}

/// The face's frame for the pattern: its mean normal `n` and, on it, `u` (level on a wall,
/// along X on a floor) and `v`. The pattern's origin is the world origin seen along `n`.
pub fn frame(tris: &[[Vec3; 3]]) -> Option<(Vec3, Vec3, Vec3)> {
    let mut n = Vec3::new(0.0, 0.0, 0.0);
    for t in tris {
        n = n + (t[1] - t[0]).cross(t[2] - t[0]);
    }
    let n = n.normalized()?;
    let z = Vec3::new(0.0, 0.0, 1.0);
    let u = if n.dot(z).abs() > 0.9 { (Vec3::new(1.0, 0.0, 0.0) - n * n.x).normalized()? } else { z.cross(n).normalized()? };
    let v = n.cross(u);
    Some((n, u, v))
}

/// Whether the face is flat (every triangle square to the mean normal).
pub fn is_flat(tris: &[[Vec3; 3]], n: Vec3) -> bool {
    tris.iter().all(|t| (t[1] - t[0]).cross(t[2] - t[0]).normalized().is_none_or(|m| m.dot(n) > 1.0 - 1e-6))
}

/// The pattern's families after the hatch's scale and angle, in the face's (u, v) frame.
pub fn families(h: &Hatch) -> Vec<PatLine> {
    let s = if h.scale.is_finite() && h.scale > 0.0 { h.scale } else { 1.0 };
    let (sa, ca) = h.angle.to_radians().sin_cos();
    h.pattern
        .lines
        .iter()
        .take(MAX_LINES)
        .map(|l| PatLine {
            angle: l.angle + h.angle,
            x: (l.x * ca - l.y * sa) * s,
            y: (l.x * sa + l.y * ca) * s,
            dx: l.dx * s,
            dy: l.dy * s,
            dashes: l.dashes.iter().map(|d| d * s).collect(),
        })
        .collect()
}

/// The dashes of one line between `s0` and `s1` (along it, measured from its own origin).
pub fn dashed(dashes: &[f64], s0: f64, s1: f64, out: &mut Vec<(f64, f64)>) {
    if dashes.is_empty() {
        out.push((s0, s1));
        return;
    }
    let period: f64 = dashes.iter().map(|d| d.abs()).sum();
    if !(period > 1e-9) {
        out.push((s0, s1));
        return;
    }
    let mut start = (s0 / period).floor() * period;
    let mut guard = 0;
    while start < s1 && guard < 100_000 {
        let mut pos = start;
        for d in dashes {
            let len = d.abs();
            if *d >= 0.0 {
                // A dot is a very short dash.
                let end = pos + if *d == 0.0 { period * 0.08 } else { len };
                let (a, b) = (pos.max(s0), end.min(s1));
                if b > a {
                    out.push((a, b));
                }
            }
            pos += len;
        }
        start += period;
        guard += 1;
    }
}

/// The 2D segments of the families inside a triangle given in (u, v).
pub fn tri_segments(fams: &[PatLine], tri: [Vec2; 3], out: &mut Vec<(Vec2, Vec2)>) {
    for f in fams {
        if f.dy.abs() < 1e-12 {
            continue;
        }
        let (sn, cs) = f.angle.to_radians().sin_cos();
        let d = Vec2::new(cs, sn);
        let p = Vec2::new(-sn, cs);
        let o = Vec2::new(f.x, f.y);
        // Line k: through o + k·(dx·d + dy·p), along d.
        let t: Vec<f64> = tri.iter().map(|q| (*q - o).dot(p)).collect();
        let (tmin, tmax) = t.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
        let (k0, k1) = ((tmin / f.dy).min(tmax / f.dy).ceil(), (tmin / f.dy).max(tmax / f.dy).floor());
        if !(k0.is_finite() && k1.is_finite()) || k1 - k0 > MAX_FAMILY_LINES as f64 {
            continue;
        }
        let mut k = k0;
        while k <= k1 {
            let origin = o + d * (k * f.dx) + p * (k * f.dy);
            let tk = (origin - o).dot(p);
            // Where the line crosses the triangle's sides, along d from `origin`.
            let mut hits: Vec<f64> = Vec::with_capacity(3);
            for i in 0..3 {
                let (a, b) = (tri[i], tri[(i + 1) % 3]);
                let (ta, tb) = ((a - o).dot(p) - tk, (b - o).dot(p) - tk);
                if (ta > 0.0) != (tb > 0.0) || ta == 0.0 {
                    let w = if (ta - tb).abs() < 1e-15 { 0.0 } else { ta / (ta - tb) };
                    let q = a + (b - a) * w;
                    hits.push((q - origin).dot(d));
                }
            }
            if hits.len() >= 2 {
                let (s0, s1) = hits.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
                let mut spans = Vec::new();
                dashed(&f.dashes, s0, s1, &mut spans);
                for (a, b) in spans {
                    out.push((origin + d * a, origin + d * b));
                }
            }
            k += 1.0;
            if out.len() > MAX_SEGMENTS {
                return;
            }
        }
    }
}

/// The hatch's lines on its face, in the world, lifted `lift` along the face's normal (so
/// the lines aren't hidden in the face).
pub fn segments(mesh: &Mesh, h: &Hatch, lift: f64) -> Vec<(Vec3, Vec3)> {
    let Some(face) = resolve_face(mesh, h) else { return Vec::new() };
    let tris = face_tris(mesh, face);
    let Some(map) = Unroll::of(&tris) else { return Vec::new() };
    let fams = families(h);
    let mut out = Vec::new();
    for t in &tris {
        let uv = map.tri(t);
        let mut segs = Vec::new();
        tri_segments(&fams, uv, &mut segs);
        // Back onto the triangle: barycentric in (u, v).
        let (a, b, c) = (uv[0], uv[1], uv[2]);
        let det = (b - a).cross(c - a);
        if det.abs() < 1e-18 {
            continue;
        }
        let Some(tn) = (t[1] - t[0]).cross(t[2] - t[0]).normalized() else { continue };
        let lift_v = tn * lift;
        let lifted = |q: Vec2| {
            let l1 = (q - a).cross(c - a) / det;
            let l2 = (b - a).cross(q - a) / det;
            t[0] + (t[1] - t[0]) * l1 + (t[2] - t[0]) * l2 + lift_v
        };
        out.extend(segs.into_iter().map(|(p, q)| (lifted(p), lifted(q))));
        if out.len() > MAX_SEGMENTS {
            break;
        }
    }
    out
}

/// How a face is laid flat for its pattern: a flat (or gently curved) face seen along its
/// normal; a cylinder or cone unrolled about its axis (round the axis by arc length, along it
/// by length); anything else, each triangle seen along the axis nearest its normal.
enum Unroll {
    Plane { u: Vec3, v: Vec3 },
    Cylinder { axis: Vec3, centre: Vec3, x: Vec3, y: Vec3, radius: f64 },
    Triplanar,
}

impl Unroll {
    fn of(tris: &[[Vec3; 3]]) -> Option<Unroll> {
        let plane = frame(tris);
        if let Some((n, u, v)) = plane
            && is_flat(tris, n)
        {
            return Some(Unroll::Plane { u, v });
        }
        // The normals' spread: a cylinder's lie square to one axis (the least spread).
        let mut m = [[0.0f64; 3]; 3];
        let (mut area, mut sum, mut centre) = (0.0, Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0));
        for t in tris {
            let c = (t[1] - t[0]).cross(t[2] - t[0]);
            let w = c.len();
            let Some(nn) = c.normalized() else { continue };
            let k = [nn.x, nn.y, nn.z];
            for (i, row) in m.iter_mut().enumerate() {
                for (j, cell) in row.iter_mut().enumerate() {
                    *cell += w * k[i] * k[j];
                }
            }
            area += w;
            sum = sum + c;
            centre = centre + (t[0] + t[1] + t[2]) * (w / 3.0);
        }
        if !(area > 0.0) {
            return None;
        }
        let centre = centre / area;
        // The least eigenvector of m: power iteration on (trace·I − m).
        let tr = m[0][0] + m[1][1] + m[2][2];
        let mut axis = Vec3::new(0.3, 0.5, 0.8);
        for _ in 0..64 {
            let a = [axis.x, axis.y, axis.z];
            let row = |i: usize| tr * a[i] - (m[i][0] * a[0] + m[i][1] * a[1] + m[i][2] * a[2]);
            axis = Vec3::new(row(0), row(1), row(2)).normalized()?;
        }
        let worst = tris.iter().filter_map(|t| (t[1] - t[0]).cross(t[2] - t[0]).normalized()).map(|nn| nn.dot(axis).abs()).fold(0.0f64, f64::max);
        if worst < 0.35 {
            let x = axis.any_perp().normalized()?;
            let y = axis.cross(x);
            let radius = tris
                .iter()
                .flat_map(|t| t.iter())
                .map(|p| {
                    let d = *p - centre;
                    (d - axis * d.dot(axis)).len()
                })
                .fold(0.0, f64::max);
            if radius > 0.0 {
                return Some(Unroll::Cylinder { axis, centre, x, y, radius });
            }
        }
        // Gently curved: seen along the mean normal, as a flat face.
        if let Some((_, u, v)) = plane
            && sum.len() / area > 0.85
        {
            return Some(Unroll::Plane { u, v });
        }
        Some(Unroll::Triplanar)
    }

    fn tri(&self, t: &[Vec3; 3]) -> [Vec2; 3] {
        match self {
            Unroll::Plane { u, v } => t.map(|p| Vec2::new(p.dot(*u), p.dot(*v))),
            Unroll::Cylinder { axis, centre, x, y, radius } => {
                let angle = |p: Vec3| {
                    let d = p - *centre;
                    d.dot(*y).atan2(d.dot(*x))
                };
                let mut a = t.map(angle);
                // One side of the seam: angles within half a turn of the first.
                for k in 1..3 {
                    while a[k] - a[0] > std::f64::consts::PI {
                        a[k] -= std::f64::consts::TAU;
                    }
                    while a[0] - a[k] > std::f64::consts::PI {
                        a[k] += std::f64::consts::TAU;
                    }
                }
                [0, 1, 2].map(|k| Vec2::new(a[k] * radius, (t[k] - *centre).dot(*axis)))
            }
            Unroll::Triplanar => {
                let n = (t[1] - t[0]).cross(t[2] - t[0]);
                let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
                if az >= ax && az >= ay {
                    t.map(|p| Vec2::new(p.x, p.y))
                } else if ax >= ay {
                    t.map(|p| Vec2::new(p.y, p.z))
                } else {
                    t.map(|p| Vec2::new(p.x, p.z))
                }
            }
        }
    }
}

/// The face's boundary loops in the world (its edges chained end to end).
pub fn face_loops(mesh: &Mesh, face: usize) -> Vec<Vec<Vec3>> {
    let mut pieces: Vec<Vec<Vec3>> = mesh
        .edges
        .iter()
        .zip(&mesh.edge_faces)
        .filter(|(_, fs)| fs.iter().any(|f| *f as usize == face))
        .map(|(e, _)| e.clone())
        .filter(|e| e.len() >= 2)
        .collect();
    let size = mesh.positions.iter().fold(0.0f64, |m, p| m.max(p.len()));
    let tol = (size * 1e-7).max(1e-7);
    let mut loops = Vec::new();
    while let Some(mut cur) = pieces.pop() {
        let mut guard = 0;
        loop {
            guard += 1;
            let (Some(&first), Some(&last)) = (cur.first(), cur.last()) else { break };
            if (first - last).len() <= tol && cur.len() > 2 {
                break;
            }
            let next =
                pieces.iter().position(|p| p.first().is_some_and(|q| (*q - last).len() <= tol) || p.last().is_some_and(|q| (*q - last).len() <= tol));
            let Some(i) = next else { break };
            let mut p = pieces.swap_remove(i);
            if p.first().is_some_and(|q| (*q - last).len() > tol) {
                p.reverse();
            }
            cur.extend(p.into_iter().skip(1));
            if guard > 100_000 {
                break;
            }
        }
        loops.push(cur);
    }
    loops
}

/// The hatch's families seen through a linear map `m` (the face's (u, v) to the drawing) and
/// shifted by `t`: each family stays a family of parallel dashed lines.
pub fn mapped_families(h: &Hatch, m: [[f64; 2]; 2], t: Vec2) -> Vec<solidcraft_io::HatchLine> {
    let apply = |q: Vec2| Vec2::new(m[0][0] * q.x + m[0][1] * q.y, m[1][0] * q.x + m[1][1] * q.y);
    families(h)
        .into_iter()
        .filter_map(|f| {
            let (sn, cs) = f.angle.to_radians().sin_cos();
            let d = Vec2::new(cs, sn);
            let p = Vec2::new(-sn, cs);
            let d2 = apply(d);
            let len = d2.len();
            if len < 1e-12 {
                return None;
            }
            let offset = apply(d * f.dx + p * f.dy);
            Some(solidcraft_io::HatchLine {
                angle_deg: d2.y.atan2(d2.x).to_degrees(),
                base: apply(Vec2::new(f.x, f.y)) + t,
                offset,
                dashes: f.dashes.iter().map(|x| x * len).collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pat_files_are_read() {
        let p = parse_pat("; comment\n*TEST, A test\n45, 0,0, 0,3.175\n*BAD\n0,0\n*TWO, x\n0, 0,0, 0,1, 1,-1\n90, 0,0, 0,1\n");
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].name, "TEST");
        assert_eq!(p[1].lines.len(), 2);
        assert_eq!(p[1].lines[0].dashes, vec![1.0, -1.0]);
        assert!(builtin().len() >= 6);
    }

    #[test]
    fn lines_cross_a_triangle_at_the_spacing() {
        let fams = vec![PatLine { angle: 0.0, x: 0.0, y: 0.0, dx: 0.0, dy: 1.0, dashes: vec![] }];
        let mut out = Vec::new();
        tri_segments(&fams, [Vec2::new(0.0, -0.5), Vec2::new(10.0, -0.5), Vec2::new(0.0, 9.5)], &mut out);
        // Lines y = 0..9 cross it.
        assert_eq!(out.len(), 10);
        assert!(out.iter().all(|(a, b)| (a.y - b.y).abs() < 1e-12));
    }
}
