//! The Drawing workspace's sheet: a grid of orthographic views of the visible bodies, each drawn
//! as a wireframe (every edge) or with hidden lines removed, at one common scale, inside a frame
//! with a title. Written as DWG (AutoCAD 2004) or DXF, or returned as preview lines.

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_io::{DxfEntity, DxfLayer, DxfShape};
use solidcraft_render::{Camera, StandardView};

use super::{meshes, projection};
use crate::cmd::CommandSpec;
use crate::params::{bad, str_};
use crate::{EngineError, Result, Session};

const CMD: &str = "drawing.export";

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Export Drawing", export)
    .icon("export")
    .noundo()
    .params("rows?: 1..4 (2); cols?: 1..4 (2); views?: [{view: front|back|left|right|top|bottom|iso|current|none, style?: wireframe|hidden_line}]; camera?: Camera (the current view, perspective allowed; needed by view current) row by row from the top left (default 2×2: front, right / top, iso); sheet?: [width, height] mm (420×297, A3); title?: text; up?: z|y (world up axis, default z); scale?: auto|ratio (paper/model: 0.5 is 1:2, 2 is 2:1; auto fits the largest preferred scale); format?: dwg|dxf (by the path's extension, else dwg); path?: file; preview?: true returns the sheet's lines instead of writing. Visible bodies only, one common scale for every view. Hatch patterns on faces (hatch.apply) come out on layer HATCH: a HATCH entity for a flat face seen whole in an orthographic view, otherwise a block (SC_HATCH_n) of the pattern's visible lines.")];

/// Largest grid.
const MAX_CELLS: usize = 4;
/// Sheet border to the frame (mm).
const MARGIN: f64 = 10.0;
/// Height of the title strip at the bottom of the frame (mm).
const TITLE: f64 = 14.0;
/// Room left under each view for its label (mm).
const LABEL: f64 = 8.0;

/// One cell of the sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub view: Option<StandardView>,
    pub hidden_line: bool,
    /// The view is the `camera` given (the window's current view, perspective or not).
    pub current: bool,
}

/// The default layout: front top left, right top right, top bottom left, home (iso) bottom right.
pub fn default_cells() -> Vec<Cell> {
    [StandardView::Front, StandardView::Right, StandardView::Top, StandardView::Iso]
        .into_iter()
        .map(|v| Cell { view: Some(v), hidden_line: true, current: false })
        .collect()
}

fn view_name(v: StandardView) -> &'static str {
    match v {
        StandardView::Front => "FRONT",
        StandardView::Back => "BACK",
        StandardView::Left => "LEFT",
        StandardView::Right => "RIGHT",
        StandardView::Top => "TOP",
        StandardView::Bottom => "BOTTOM",
        StandardView::Iso => "HOME",
    }
}

fn count(p: &Value, key: &str) -> Result<usize> {
    match p.get(key) {
        None => Ok(2),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=MAX_CELLS).contains(n))
            .ok_or_else(|| bad(CMD, format!("{key} must be 1..{MAX_CELLS}"))),
    }
}

fn cells(p: &Value, n: usize) -> Result<Vec<Cell>> {
    let Some(list) = p.get("views") else {
        let mut d = default_cells();
        d.resize(n, Cell { view: None, hidden_line: true, current: false });
        d.truncate(n);
        return Ok(d);
    };
    let list = list.as_array().ok_or_else(|| bad(CMD, "views must be a list"))?;
    if list.len() > n {
        return Err(bad(CMD, "more views than cells"));
    }
    let mut out = Vec::with_capacity(n);
    for v in list {
        let name = v.get("view").and_then(Value::as_str).unwrap_or("none");
        let current = name == "current";
        let view = match name {
            "none" | "" => None,
            "current" => Some(StandardView::Iso),
            other => Some(StandardView::parse(other).ok_or_else(|| bad(CMD, format!("unknown view {other:?}")))?),
        };
        let hidden_line = match v.get("style").and_then(Value::as_str) {
            None | Some("hidden_line") | Some("hidden") => true,
            Some("wireframe") => false,
            Some(_) => return Err(bad(CMD, "style must be wireframe or hidden_line")),
        };
        out.push(Cell { view, hidden_line, current });
    }
    out.resize(n, Cell { view: None, hidden_line: true, current: false });
    Ok(out)
}

/// The projected lines of one view in the view's own millimetres: the bodies' edges and
/// outlines, and the shown sketches' curves (model lines), hidden ones only in a wireframe.
fn view_lines(scene: &meshes::Scene, parts: &[super::sketch::Part], cam: &Camera, hidden_line: bool) -> Result<Vec<(Vec2, Vec2)>> {
    let d = projection::drawing(scene, parts, cam).map_err(|e| bad(CMD, e))?;
    let mut out: Vec<(Vec2, Vec2)> = d.lines.into_iter().filter(|l| !(hidden_line && l.hidden)).map(|l| (l.a, l.b)).collect();
    for path in d.sketch.into_iter().filter(|p| !(hidden_line && p.hidden)) {
        out.extend(path.points.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))));
    }
    Ok(out)
}

/// A view's camera: a standard view, or the camera given for the current view.
fn camera(cell: &Cell, view: StandardView, up: solidcraft_render::UpAxis, current: Option<&Camera>) -> Camera {
    if cell.current
        && let Some(c) = current
    {
        return *c;
    }
    let mut cam = Camera { up, fov: 0.0, ..Camera::default() };
    cam.set_view(view);
    cam
}

/// The window's view (`camera`), when one is given and valid.
fn current_camera(p: &Value) -> Result<Option<Camera>> {
    let Some(v) = p.get("camera") else { return Ok(None) };
    let cam: Camera = serde_json::from_value(v.clone()).map_err(|_| bad(CMD, "invalid camera"))?;
    let ok = cam.is_valid()
        && (0.0..std::f64::consts::PI).contains(&cam.fov)
        && cam.pitch.abs() <= std::f64::consts::FRAC_PI_2
        && cam.yaw.abs() <= 1e6
        && cam.roll.abs() <= 1e6;
    if ok { Ok(Some(cam)) } else { Err(bad(CMD, "invalid camera")) }
}

/// A hatch as one view draws it: a HATCH (a flat face seen whole in an orthographic view), or
/// the pattern's visible lines (a curved face, a perspective view, a face partly hidden).
enum HatchOut {
    Pattern { loops: Vec<Vec<Vec2>>, hatch: crate::hatch::Hatch, m: [[f64; 2]; 2], t: Vec2 },
    Lines(Vec<(Vec2, Vec2)>),
}

/// Most pattern lines one hatch puts in a drawing.
const MAX_HATCH_LINES: usize = 40_000;

/// The hatches of the visible bodies, each with its body tessellated as the drawing's hidden
/// lines see it (so the pattern lies on the same surface that hides it).
fn hatch_meshes(s: &Session, tolerance: f64) -> Result<Vec<(crate::hatch::Hatch, std::sync::Arc<solidcraft_geom::Mesh>)>> {
    let hatches = crate::hatch::hatches(&s.doc.metadata);
    if hatches.is_empty() {
        return Ok(Vec::new());
    }
    let st = s.world_state()?;
    let mut meshes: Vec<(String, std::sync::Arc<solidcraft_geom::Mesh>)> = Vec::new();
    let mut out = Vec::new();
    for h in hatches {
        if !s.body_visible(&h.body) {
            continue;
        }
        let Some(b) = st.bodies.iter().find(|b| b.name == h.body) else { continue };
        let mesh = match meshes.iter().find(|(n, _)| *n == h.body) {
            Some((_, m)) => m.clone(),
            None => {
                let tol = solidcraft_kernel::MeshTolerance { chord: tolerance, angle: Some(solidcraft_kernel::DISPLAY_ANGLE) };
                let m = std::sync::Arc::new(b.body.mesh(&tol).map_err(|e| bad(CMD, e.to_string()))?);
                meshes.push((h.body.clone(), m.clone()));
                m
            }
        };
        out.push((h, mesh));
    }
    Ok(out)
}

/// The hatches, as the view `cam` sees them (in the view's millimetres).
fn view_hatches(
    hatches: &[(crate::hatch::Hatch, std::sync::Arc<solidcraft_geom::Mesh>)],
    scene: &meshes::Scene,
    cam: &Camera,
) -> Result<Vec<HatchOut>> {
    use super::sketch::Part;
    use solidcraft_geom::prelude::*;
    let (right, up, back) = cam.basis();
    let mut out = Vec::new();
    for (h, mesh) in hatches {
        let h = h.clone();
        let Some(face) = crate::hatch::resolve_face(&mesh, &h) else { continue };
        let tris: Vec<[solidcore_record::Vec3; 3]> =
            mesh.triangles.iter().zip(&mesh.tri_face).filter(|(_, f)| **f as usize == face).filter_map(|(t, _)| mesh.tri(t)).collect();
        let size = mesh.positions.iter().fold(0.0f64, |m, p| m.max(p.x.abs().max(p.y.abs()).max(p.z.abs())));
        let lift = (size * 1e-4).max(1e-4);
        let flat_frame = crate::hatch::frame(&tris).filter(|(n, _, _)| crate::hatch::is_flat(&tris, *n));
        if let Some((n, u, v)) = flat_frame
            && cam.fov == 0.0
            && n.dot(back) > 1e-6
        {
            // Seen whole? Its outline, lifted off the face, is nowhere hidden.
            let loops3 = crate::hatch::face_loops(&mesh, face);
            let parts: Vec<Part> = loops3.iter().map(|l| Part { points: l.iter().map(|p| *p + n * lift).collect(), on_top: false }).collect();
            let d = projection::drawing(scene, &parts, cam).map_err(|e| bad(CMD, e))?;
            // Edges shared with the faces around it may read as hidden here and there where they
            // meet: a face whose outline is all but whole is seen whole.
            let length = |p: &projection::Path| p.points.windows(2).filter_map(|w| Some((*w.get(1)? - *w.first()?).len())).sum::<f64>();
            let hidden: f64 = d.sketch.iter().filter(|p| p.hidden).map(length).sum();
            let total: f64 = d.sketch.iter().map(length).sum();
            if !loops3.is_empty() && total > 0.0 && hidden <= total * 0.02 {
                let xy = |p: solidcore_record::Vec3| {
                    let d = p - cam.target;
                    Vec2::new(d.dot(right), d.dot(up))
                };
                let loops: Vec<Vec<Vec2>> = loops3.iter().map(|l| l.iter().map(|p| xy(*p)).collect()).collect();
                let c = tris.first().map_or(0.0, |t| t[0].dot(n));
                let m = [[u.dot(right), v.dot(right)], [u.dot(up), v.dot(up)]];
                let t = xy(n * c);
                out.push(HatchOut::Pattern { loops, hatch: h, m, t });
                continue;
            }
        }
        // Otherwise the pattern's lines, hidden where the bodies cover them.
        let segs = crate::hatch::segments(&mesh, &h, lift);
        let parts: Vec<Part> = segs.into_iter().take(MAX_HATCH_LINES).map(|(a, b)| Part { points: vec![a, b], on_top: false }).collect();
        if parts.is_empty() {
            continue;
        }
        let d = projection::drawing(scene, &parts, cam).map_err(|e| bad(CMD, e))?;
        let lines: Vec<(Vec2, Vec2)> = d
            .sketch
            .into_iter()
            .filter(|p| !p.hidden)
            .flat_map(|p| p.points.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect::<Vec<_>>())
            .collect();
        if !lines.is_empty() {
            out.push(HatchOut::Lines(lines));
        }
    }
    Ok(out)
}

fn bounds(lines: &[(Vec2, Vec2)]) -> Option<(Vec2, Vec2)> {
    let mut it = lines.iter().flat_map(|(a, b)| [*a, *b]).filter(|p| p.x.is_finite() && p.y.is_finite());
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y)))))
}

/// The largest preferred drawing scale (1:1, 1:2, 1:5, 2:1, …) at or below `s`.
fn nice_scale(s: f64) -> f64 {
    if !(s.is_finite() && s > 0.0) {
        return 1.0;
    }
    let k = s.log10().floor();
    let base = 10f64.powf(k);
    let m = s / base;
    let step = if m >= 5.0 {
        5.0
    } else if m >= 2.0 {
        2.0
    } else {
        1.0
    };
    step * base
}

fn scale_text(s: f64) -> String {
    let fmt = |v: f64| {
        let r = (v * 1000.0).round() / 1000.0;
        if (r - r.round()).abs() < 1e-9 { format!("{}", r.round()) } else { format!("{r}") }
    };
    if s >= 1.0 { format!("{}:1", fmt(s)) } else { format!("1:{}", fmt(1.0 / s)) }
}

/// The finished sheet.
pub struct Sheet {
    pub layers: Vec<DxfLayer>,
    pub entities: Vec<DxfEntity>,
    pub scale: f64,
    /// The scale that fits every view in its cell.
    pub fit: f64,
    pub size: (f64, f64),
}

/// Lay the views out on the sheet.
fn compose(s: &Session, p: &Value) -> Result<Sheet> {
    let rows = count(p, "rows")?;
    let cols = count(p, "cols")?;
    let cells = cells(p, rows * cols)?;
    let (w, h) = match p.get("sheet") {
        None => (420.0, 297.0),
        Some(v) => {
            let a = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(CMD, "sheet must be [width, height]"))?;
            let n = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| (50.0..=5000.0).contains(x));
            match (n(0), n(1)) {
                (Some(w), Some(h)) => (w, h),
                _ => return Err(bad(CMD, "sheet sides must be 50..5000 mm")),
            }
        }
    };
    let up = match p.get("up").and_then(Value::as_str) {
        Some("y") | Some("Y") => solidcraft_render::UpAxis::Y,
        _ => solidcraft_render::UpAxis::Z,
    };
    let title = p.get("title").and_then(Value::as_str).unwrap_or("SolidCraft drawing").chars().take(120).collect::<String>();
    if !super::sketch::exportable(s) {
        return Err(bad(CMD, "no visible bodies to draw"));
    }
    let scene = meshes::prepare(s, 0.01)?;
    let parts = super::sketch::parts(s, 0.01).map_err(|e| bad(CMD, e))?;
    let hatch_meshes = hatch_meshes(s, 0.01)?;
    if scene.placements.is_empty() && parts.is_empty() {
        return Err(bad(CMD, "no visible bodies to draw"));
    }
    // The frame and the cells inside it.
    let (fx0, fy0, fx1, fy1) = (MARGIN, MARGIN, w - MARGIN, h - MARGIN);
    let area_y0 = fy0 + TITLE;
    let cw = (fx1 - fx0) / cols as f64;
    let ch = (fy1 - area_y0) / rows as f64;
    let current = current_camera(p)?;
    let mut views = Vec::with_capacity(cells.len());
    let mut scale = f64::INFINITY;
    for cell in &cells {
        let Some(v) = cell.view else {
            views.push(None);
            continue;
        };
        if cell.current && current.is_none() {
            return Err(bad(CMD, "a current view needs `camera`"));
        }
        let cam = camera(cell, v, up, current.as_ref());
        let lines = view_lines(&scene, &parts, &cam, cell.hidden_line)?;
        if let Some((lo, hi)) = bounds(&lines) {
            let (bw, bh) = ((hi.x - lo.x).max(1e-6), (hi.y - lo.y).max(1e-6));
            scale = scale.min((cw * 0.8) / bw).min((ch - LABEL) * 0.8 / bh);
            let hatches = view_hatches(&hatch_meshes, &scene, &cam)?;
            let name = if cell.current { "CURRENT VIEW" } else { view_name(v) };
            views.push(Some((name, lines, lo, hi, hatches)));
        } else {
            views.push(None);
        }
    }
    let fit = nice_scale(if scale.is_finite() { scale } else { 1.0 });
    let scale = match p.get("scale") {
        None => fit,
        Some(Value::String(t)) if t == "auto" => fit,
        Some(v) => v
            .as_f64()
            .filter(|k| k.is_finite() && (1e-4..=1e4).contains(k))
            .ok_or_else(|| bad(CMD, "scale must be auto or a ratio 0.0001..10000 (0.5 is 1:2)"))?,
    };
    let mut entities = Vec::new();
    let line = |layer: &str, a: Vec2, b: Vec2| DxfEntity { layer: layer.into(), shape: DxfShape::Line(a, b) };
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| DxfEntity {
        layer: "FRAME".into(),
        shape: DxfShape::Polyline { points: vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1), Vec2::new(x0, y0)] },
    };
    entities.push(rect(fx0, fy0, fx1, fy1));
    entities.push(line("FRAME", Vec2::new(fx0, area_y0), Vec2::new(fx1, area_y0)));
    let text = |at: Vec2, height: f64, text: String| DxfEntity { layer: "TEXT".into(), shape: DxfShape::Text { at, height, angle_deg: 0.0, text } };
    entities.push(text(Vec2::new((fx0 + fx1) / 2.0, fy0 + TITLE / 2.0 - 2.5), 5.0, title));
    entities.push(text(Vec2::new(fx1 - 30.0, fy0 + TITLE / 2.0 - 1.75), 3.5, format!("SCALE {}", scale_text(scale))));
    let mut blocks = 0usize;
    for (i, item) in views.into_iter().enumerate() {
        let Some((name, lines, lo, hi, hatches)) = item else { continue };
        let (r, c) = (i / cols, i % cols);
        let cx = fx0 + cw * (c as f64 + 0.5);
        let cy = fy1 - ch * (r as f64 + 0.5) + LABEL / 2.0;
        let mid = Vec2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        let place = |p: Vec2| Vec2::new(cx + (p.x - mid.x) * scale, cy + (p.y - mid.y) * scale);
        let placed: Vec<(Vec2, Vec2)> = lines.into_iter().map(|(a, b)| (place(a), place(b))).collect();
        for chain in chains(&placed) {
            for shape in curves(&chain) {
                entities.push(DxfEntity { layer: "VISIBLE".into(), shape });
            }
        }
        for hout in hatches {
            match hout {
                HatchOut::Pattern { loops, hatch, m, t } => {
                    let loops = loops.into_iter().map(|l| l.into_iter().map(place).collect()).collect();
                    let m2 = [[m[0][0] * scale, m[0][1] * scale], [m[1][0] * scale, m[1][1] * scale]];
                    let t2 = place(t);
                    let lines = crate::hatch::mapped_families(&hatch, m2, t2);
                    entities
                        .push(DxfEntity { layer: "HATCH".into(), shape: DxfShape::Hatch { loops, pattern: hatch_name(&hatch.pattern.name), lines } });
                }
                HatchOut::Lines(segs) => {
                    blocks += 1;
                    let shapes = segs.into_iter().map(|(a, b)| DxfShape::Line(place(a), place(b))).collect();
                    entities.push(DxfEntity { layer: "HATCH".into(), shape: DxfShape::Block { name: format!("SC_HATCH_{blocks}"), shapes } });
                }
            }
        }
        let bottom = cy - (hi.y - lo.y) * scale / 2.0;
        entities.push(text(Vec2::new(cx, (bottom - LABEL + 1.0).max(area_y0 + 1.0)), 3.5, name.to_string()));
    }
    let layers = vec![DxfLayer::new("VISIBLE", 7), DxfLayer::new("FRAME", 7), DxfLayer::new("TEXT", 7), DxfLayer::new("HATCH", 8)];
    Ok(Sheet { layers, entities, scale, fit, size: (w, h) })
}

/// A pattern name a CAD program takes: letters, digits, `_` and `-`.
fn hatch_name(name: &str) -> String {
    let n: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c.to_ascii_uppercase() } else { '_' }).take(31).collect();
    if n.is_empty() { "SC_PATTERN".into() } else { n }
}

/// A HATCH's lines inside its loops (even-odd), as a CAD program draws them: for the preview.
fn hatch_fill(loops: &[Vec<Vec2>], lines: &[solidcraft_io::HatchLine]) -> Vec<(Vec2, Vec2)> {
    const MAX: usize = 60_000;
    let mut out = Vec::new();
    let edges: Vec<(Vec2, Vec2)> =
        loops.iter().flat_map(|l| l.iter().zip(l.iter().cycle().skip(1)).take(l.len()).map(|(a, b)| (*a, *b)).collect::<Vec<_>>()).collect();
    for f in lines {
        let (sn, cs) = f.angle_deg.to_radians().sin_cos();
        let d = Vec2::new(cs, sn);
        let n = Vec2::new(-sn, cs);
        let dot = |a: Vec2, b: Vec2| a.x * b.x + a.y * b.y;
        let step = dot(f.offset, n);
        if step.abs() < 1e-9 {
            continue;
        }
        let across: Vec<f64> = edges.iter().flat_map(|(a, b)| [dot(*a - f.base, n), dot(*b - f.base, n)]).collect();
        let (lo, hi) = across.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| (l.min(*v), h.max(*v)));
        let (k0, k1) = ((lo / step).min(hi / step).ceil(), (lo / step).max(hi / step).floor());
        if !(k0.is_finite() && k1.is_finite()) || k1 - k0 > 20_000.0 {
            continue;
        }
        let mut k = k0;
        while k <= k1 && out.len() < MAX {
            let origin = f.base + f.offset * k;
            let mut ts: Vec<f64> = Vec::new();
            for (a, b) in &edges {
                let (ha, hb) = (dot(*a - origin, n), dot(*b - origin, n));
                if (ha > 0.0) != (hb > 0.0) {
                    let q = *a + (*b - *a) * (ha / (ha - hb));
                    ts.push(dot(q - origin, d));
                }
            }
            ts.sort_by(f64::total_cmp);
            for pair in ts.chunks_exact(2) {
                let [t0, t1] = pair else { continue };
                let mut spans = Vec::new();
                crate::hatch::dashed(&f.dashes, *t0, *t1, &mut spans);
                out.extend(spans.into_iter().map(|(s0, s1)| (origin + d * s0, origin + d * s1)));
            }
            k += 1.0;
        }
    }
    out
}

fn points(shape: &DxfShape) -> Vec<Vec2> {
    match shape {
        DxfShape::Line(a, b) => vec![*a, *b],
        DxfShape::Polyline { points } => points.clone(),
        DxfShape::Circle { c, r } => arc_points(*c, *r, 0.0, 360.0),
        DxfShape::Arc { c, r, a0, a1 } => arc_points(*c, *r, *a0, if a1 < a0 { a1 + 360.0 } else { *a1 }),
        DxfShape::Spline { ctrl, .. } => ctrl.clone(),
        _ => Vec::new(),
    }
}

fn arc_points(c: Vec2, r: f64, a0: f64, a1: f64) -> Vec<Vec2> {
    let n = (((a1 - a0).abs() / 6.0).ceil() as usize).clamp(2, 120);
    (0..=n)
        .map(|i| {
            let a = (a0 + (a1 - a0) * i as f64 / n as f64).to_radians();
            Vec2::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

/// Sheet tolerance for joining ends and fitting curves (mm).
const JOIN: f64 = 1e-3;
const FIT: f64 = 0.02;

/// Join segments end to end into polylines; a point where more or fewer than two segments
/// meet ends them.
fn chains(segs: &[(Vec2, Vec2)]) -> Vec<Vec<Vec2>> {
    use std::collections::BTreeMap;
    let key = |p: Vec2| [(p.x / JOIN).round() as i64, (p.y / JOIN).round() as i64];
    let segs: Vec<(Vec2, Vec2)> = segs.iter().copied().filter(|(a, b)| key(*a) != key(*b)).collect();
    let mut nodes: BTreeMap<[i64; 2], Vec<usize>> = BTreeMap::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        nodes.entry(key(*a)).or_default().push(i);
        nodes.entry(key(*b)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        if let Some(u) = used.get_mut(i) {
            *u = true;
        }
        let mut points = vec![*a, *b];
        for _ in 0..2 {
            while let Some(end) = points.last().copied() {
                let Some(next_ids) = nodes.get(&key(end)).filter(|n| n.len() == 2) else { break };
                let Some(next) = next_ids.iter().copied().find(|j| used.get(*j) == Some(&false)) else { break };
                let Some((p, q)) = segs.get(next) else { break };
                if let Some(u) = used.get_mut(next) {
                    *u = true;
                }
                points.push(if key(*p) == key(end) { *q } else { *p });
            }
            points.reverse();
        }
        out.push(points);
    }
    out
}

fn sub(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(a.x - b.x, a.y - b.y)
}
fn len(a: Vec2) -> f64 {
    a.x.hypot(a.y)
}

/// A polyline as exact-looking entities: split where it turns sharply, then each piece as a
/// line, an arc or circle when it lies on one, else a cubic spline over its points.
fn curves(points: &[Vec2]) -> Vec<DxfShape> {
    let mut pieces: Vec<Vec<Vec2>> = Vec::new();
    let mut cur: Vec<Vec2> = Vec::new();
    for (i, p) in points.iter().enumerate() {
        cur.push(*p);
        if let (Some(prev), Some(next)) = (i.checked_sub(1).and_then(|j| points.get(j)), points.get(i + 1)) {
            let (u, v) = (sub(*p, *prev), sub(*next, *p));
            let (lu, lv) = (len(u), len(v));
            let turn = if lu > 0.0 && lv > 0.0 { ((u.x * v.x + u.y * v.y) / (lu * lv)).clamp(-1.0, 1.0).acos() } else { 0.0 };
            if turn > 35f64.to_radians() {
                pieces.push(std::mem::take(&mut cur));
                cur.push(*p);
            }
        }
    }
    if cur.len() > 1 {
        pieces.push(cur);
    }
    pieces.into_iter().flat_map(|piece| piece_shapes(&piece)).collect()
}

fn piece_shapes(p: &[Vec2]) -> Vec<DxfShape> {
    let (Some(&a), Some(&b)) = (p.first(), p.last()) else { return Vec::new() };
    if p.len() == 2 {
        return vec![DxfShape::Line(a, b)];
    }
    // Straight: every point near the chord.
    let ab = sub(b, a);
    let l = len(ab);
    if l > 1e-9 && p.iter().all(|q| ((q.x - a.x) * ab.y - (q.y - a.y) * ab.x).abs() / l < FIT) {
        return vec![DxfShape::Line(a, b)];
    }
    if p.len() >= 4
        && let Some(shape) = arc(p)
    {
        return vec![shape];
    }
    if p.len() > 600 {
        return vec![DxfShape::Polyline { points: p.to_vec() }];
    }
    // A clamped cubic (quadratic for three points) B-spline over the points.
    let n = p.len();
    let degree = 3.min(n - 1);
    let spans = n - degree;
    let mut knots = vec![0.0; degree + 1];
    knots.extend((1..spans).map(|k| k as f64 / spans as f64));
    knots.extend(std::iter::repeat_n(1.0, degree + 1));
    vec![DxfShape::Spline { degree: degree as u8, knots, ctrl: p.to_vec(), weights: None }]
}

/// The circle or arc the points lie on (a least-squares circle, every point within the fit).
fn arc(p: &[Vec2]) -> Option<DxfShape> {
    let n = p.len() as f64;
    let (mx, my) = (p.iter().map(|q| q.x).sum::<f64>() / n, p.iter().map(|q| q.y).sum::<f64>() / n);
    let (mut suu, mut svv, mut suv, mut suuu, mut svvv, mut suvv, mut svuu) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for q in p {
        let (u, v) = (q.x - mx, q.y - my);
        suu += u * u;
        svv += v * v;
        suv += u * v;
        suuu += u * u * u;
        svvv += v * v * v;
        suvv += u * v * v;
        svuu += v * u * u;
    }
    let det = suu * svv - suv * suv;
    if det.abs() < 1e-12 {
        return None;
    }
    let (r1, r2) = (0.5 * (suuu + suvv), 0.5 * (svvv + svuu));
    let uc = (r1 * svv - r2 * suv) / det;
    let vc = (r2 * suu - r1 * suv) / det;
    let c = Vec2::new(mx + uc, my + vc);
    let r = (uc * uc + vc * vc + (suu + svv) / n).sqrt();
    if !(r.is_finite() && r > 1e-6 && r < 1e6) || p.iter().any(|q| (len(sub(*q, c)) - r).abs() > FIT) {
        return None;
    }
    let (Some(&a), Some(&b)) = (p.first(), p.last()) else { return None };
    // Turning direction and the swept angle.
    let ang = |q: Vec2| (q.y - c.y).atan2(q.x - c.x);
    let mut sweep = 0.0;
    for w in p.windows(2) {
        if let (Some(&x), Some(&y)) = (w.first(), w.get(1)) {
            let mut d = ang(y) - ang(x);
            while d > std::f64::consts::PI {
                d -= std::f64::consts::TAU;
            }
            while d < -std::f64::consts::PI {
                d += std::f64::consts::TAU;
            }
            sweep += d;
        }
    }
    if sweep.abs() > std::f64::consts::TAU - 1e-3 || (len(sub(a, b)) < JOIN * 10.0 && sweep.abs() > 6.0) {
        return Some(DxfShape::Circle { c, r });
    }
    let (s0, s1) = if sweep > 0.0 { (ang(a), ang(b)) } else { (ang(b), ang(a)) };
    Some(DxfShape::Arc { c, r, a0: s0.to_degrees().rem_euclid(360.0), a1: s1.to_degrees().rem_euclid(360.0) })
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let sheet = compose(s, p)?;
    let mut result = json!({"scale": scale_text(sheet.scale), "ratio": sheet.scale, "fit": scale_text(sheet.fit), "sheet": [sheet.size.0, sheet.size.1], "entities": sheet.entities.len()});
    if p.get("preview").and_then(Value::as_bool) == Some(true) {
        // Polylines as point lists, texts as [x, y, height, text], all in sheet millimetres.
        let mut paths = Vec::new();
        let mut texts = Vec::new();
        for e in &sheet.entities {
            match &e.shape {
                DxfShape::Text { at, height, text, .. } => texts.push(json!([at.x, at.y, height, text])),
                DxfShape::Hatch { loops, lines, .. } => {
                    for (a, b) in hatch_fill(loops, lines) {
                        paths.push(json!({"layer": e.layer, "points": [[a.x, a.y], [b.x, b.y]]}));
                    }
                }
                DxfShape::Block { shapes, .. } => {
                    for shape in shapes {
                        let pts: Vec<[f64; 2]> = points(shape).iter().map(|q| [q.x, q.y]).collect();
                        if pts.len() > 1 {
                            paths.push(json!({"layer": e.layer, "points": pts}));
                        }
                    }
                }
                other => {
                    let pts: Vec<[f64; 2]> = points(other).iter().map(|q| [q.x, q.y]).collect();
                    if pts.len() > 1 {
                        paths.push(json!({"layer": e.layer, "points": pts}));
                    }
                }
            }
        }
        result["paths"] = json!(paths);
        result["texts"] = json!(texts);
        return Ok(result);
    }
    let dxf = solidcraft_io::write_dxf_drawing(&sheet.layers, &sheet.entities)?;
    let path = match p.get("path") {
        None => None,
        Some(_) => Some(str_(p, "path").filter(|v| !v.trim().is_empty() && v.len() < 4096).ok_or_else(|| bad(CMD, "invalid path"))?),
    };
    let format = match p.get("format").and_then(Value::as_str) {
        Some("dxf") => "dxf",
        Some("dwg") => "dwg",
        Some(_) => return Err(bad(CMD, "format must be dwg or dxf")),
        None if path.is_some_and(|p| p.to_ascii_lowercase().ends_with(".dxf")) => "dxf",
        None => "dwg",
    };
    let bytes = if format == "dwg" { solidcraft_io::dwg_out::dxf_to_dwg(dxf.as_bytes()).map_err(|e| bad(CMD, e))? } else { dxf.clone().into_bytes() };
    result["format"] = json!(format);
    result["bytes"] = json!(bytes.len());
    match path {
        Some(path) => {
            solidcraft_io::vfs::check(path).map_err(|e| EngineError::Other(e.to_string()))?;
            solidcraft_io::write_atomic(std::path::Path::new(path), &bytes, false).map_err(|e| EngineError::Other(e.to_string()))?;
            result["path"] = json!(path);
        }
        None if format == "dxf" => result["dxf"] = json!(dxf),
        None => {}
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_are_preferred_values() {
        assert_eq!(nice_scale(0.37), 0.2);
        assert_eq!(nice_scale(0.6), 0.5);
        assert_eq!(nice_scale(3.0), 2.0);
        assert_eq!(nice_scale(1.0), 1.0);
        assert_eq!(scale_text(0.2), "1:5");
        assert_eq!(scale_text(2.0), "2:1");
    }

    #[test]
    fn polylines_become_lines_arcs_circles_and_splines() {
        let circle: Vec<(Vec2, Vec2)> = (0..36)
            .map(|i| {
                let a = |k: usize| (k as f64 * 10.0).to_radians();
                (Vec2::new(5.0 * a(i).cos(), 5.0 * a(i).sin()), Vec2::new(5.0 * a(i + 1).cos(), 5.0 * a(i + 1).sin()))
            })
            .collect();
        let ch = chains(&circle);
        assert_eq!(ch.len(), 1);
        assert!(matches!(curves(&ch[0])[..], [DxfShape::Circle { r, .. }] if (r - 5.0).abs() < 0.01));
        let half: Vec<Vec2> =
            (0..=18).map(|i| Vec2::new(5.0 * (i as f64 * 10f64.to_radians()).cos(), 5.0 * (i as f64 * 10f64.to_radians()).sin())).collect();
        assert!(matches!(curves(&half)[..], [DxfShape::Arc { a0, a1, .. }] if a0.abs() < 0.1 && (a1 - 180.0).abs() < 0.1));
        // A square: four lines.
        let sq = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0), Vec2::new(0.0, 0.0)];
        assert_eq!(curves(&sq).iter().filter(|s| matches!(s, DxfShape::Line(..))).count(), 4);
        // A parabola: a spline.
        let par: Vec<Vec2> = (0..20).map(|i| Vec2::new(i as f64 * 0.5, (i as f64 * 0.5).powi(2) * 0.1)).collect();
        assert!(matches!(curves(&par)[..], [DxfShape::Spline { .. }]));
    }

    #[test]
    fn the_default_layout_is_front_right_top_home() {
        let c = default_cells();
        assert_eq!(
            c.iter().map(|c| c.view).collect::<Vec<_>>(),
            vec![Some(StandardView::Front), Some(StandardView::Right), Some(StandardView::Top), Some(StandardView::Iso)]
        );
    }

    #[test]
    fn a_hatched_face_is_a_hatch_seen_square_and_a_block_otherwise() {
        let mut s = crate::Session::default();
        s.execute("solid.box", &serde_json::json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let st = s.world_state().unwrap();
        let b = &st.bodies[0];
        let mesh = b.mesh();
        // The face looking toward the front view (−Y): every vertex at the box's least y.
        let ymin = mesh.positions.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let faces = mesh.tri_face.iter().copied().max().unwrap() as usize;
        let front = (0..=faces)
            .find(|f| {
                mesh.triangles
                    .iter()
                    .zip(&mesh.tri_face)
                    .filter(|(_, x)| **x as usize == *f)
                    .all(|(t, _)| t.iter().all(|i| (mesh.positions[*i as usize].y - ymin).abs() < 1e-9))
            })
            .unwrap();
        let name = b.name.clone();
        drop(st);
        s.execute("hatch.apply", &serde_json::json!({"body": name, "face": front, "pattern": "SC_LINES", "scale": 2})).unwrap();
        let mut persp = Camera { fov: 0.8, distance: 200.0, ..Camera::default() };
        persp.set_view(StandardView::Iso);
        persp.fov = 0.8;
        persp.distance = 200.0;
        let sheet = compose(
            &s,
            &serde_json::json!({"rows": 1, "cols": 3, "views": [{"view": "front"}, {"view": "iso"}, {"view": "current"}], "camera": persp}),
        )
        .unwrap();
        let hatches: Vec<_> = sheet.entities.iter().filter(|e| matches!(e.shape, DxfShape::Hatch { .. })).collect();
        assert_eq!(hatches.len(), 2, "front and iso (orthographic) see the flat face whole: a HATCH each");
        if let DxfShape::Hatch { loops, lines, pattern } = &hatches[0].shape {
            assert_eq!(pattern, "SC_LINES");
            assert_eq!(loops.len(), 1);
            assert_eq!(lines.len(), 1);
            // 45° lines 3 units apart at scale 2, drawn at the sheet's scale.
            assert!((lines[0].angle_deg - 45.0).abs() < 1e-6, "{}", lines[0].angle_deg);
            let spacing = (lines[0].offset.x.powi(2) + lines[0].offset.y.powi(2)).sqrt();
            assert!((spacing - 6.0 * sheet.scale).abs() < 1e-6, "{spacing}");
        }
        assert!(
            sheet.entities.iter().any(|e| matches!(&e.shape, DxfShape::Block { shapes, .. } if !shapes.is_empty())),
            "the perspective view gets a block of lines"
        );
        // It writes as DWG.
        let dxf = solidcraft_io::write_dxf_drawing(&sheet.layers, &sheet.entities).unwrap();
        assert!(dxf.contains("HATCH") && dxf.contains("SC_HATCH_1"));
    }
}
