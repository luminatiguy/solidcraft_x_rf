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
    .params("rows?: 1..4 (2); cols?: 1..4 (2); views?: [{view: front|back|left|right|top|bottom|iso|none, style?: wireframe|hidden_line}] row by row from the top left (default 2×2: front, right / top, iso); sheet?: [width, height] mm (420×297, A3); title?: text; up?: z|y (world up axis, default z); format?: dwg|dxf (by the path's extension, else dwg); path?: file; preview?: true returns the sheet's lines instead of writing. Visible bodies only, one common scale for every view.")];

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
}

/// The default layout: front top left, right top right, top bottom left, home (iso) bottom right.
pub fn default_cells() -> Vec<Cell> {
    [StandardView::Front, StandardView::Right, StandardView::Top, StandardView::Iso]
        .into_iter()
        .map(|v| Cell { view: Some(v), hidden_line: true })
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
        d.resize(n, Cell { view: None, hidden_line: true });
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
        let view = match name {
            "none" | "" => None,
            other => Some(StandardView::parse(other).ok_or_else(|| bad(CMD, format!("unknown view {other:?}")))?),
        };
        let hidden_line = match v.get("style").and_then(Value::as_str) {
            None | Some("hidden_line") | Some("hidden") => true,
            Some("wireframe") => false,
            Some(_) => return Err(bad(CMD, "style must be wireframe or hidden_line")),
        };
        out.push(Cell { view, hidden_line });
    }
    out.resize(n, Cell { view: None, hidden_line: true });
    Ok(out)
}

/// The projected lines of one view in the view's own millimetres.
fn view_lines(scene: &meshes::Scene, view: StandardView, up: solidcraft_render::UpAxis, hidden_line: bool) -> Result<Vec<(Vec2, Vec2)>> {
    let mut cam = Camera { up, fov: 0.0, ..Camera::default() };
    cam.set_view(view);
    let d = projection::drawing(scene, &[], &cam).map_err(|e| bad(CMD, e))?;
    Ok(d.lines.into_iter().filter(|l| !(hidden_line && l.hidden)).map(|l| (l.a, l.b)).collect())
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
    if scene.placements.is_empty() {
        return Err(bad(CMD, "no visible bodies to draw"));
    }
    // The frame and the cells inside it.
    let (fx0, fy0, fx1, fy1) = (MARGIN, MARGIN, w - MARGIN, h - MARGIN);
    let area_y0 = fy0 + TITLE;
    let cw = (fx1 - fx0) / cols as f64;
    let ch = (fy1 - area_y0) / rows as f64;
    let mut views = Vec::with_capacity(cells.len());
    let mut scale = f64::INFINITY;
    for cell in &cells {
        let Some(v) = cell.view else {
            views.push(None);
            continue;
        };
        let lines = view_lines(&scene, v, up, cell.hidden_line)?;
        if let Some((lo, hi)) = bounds(&lines) {
            let (bw, bh) = ((hi.x - lo.x).max(1e-6), (hi.y - lo.y).max(1e-6));
            scale = scale.min((cw * 0.8) / bw).min((ch - LABEL) * 0.8 / bh);
            views.push(Some((v, lines, lo, hi)));
        } else {
            views.push(None);
        }
    }
    let scale = nice_scale(if scale.is_finite() { scale } else { 1.0 });
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
    for (i, item) in views.into_iter().enumerate() {
        let Some((v, lines, lo, hi)) = item else { continue };
        let (r, c) = (i / cols, i % cols);
        let cx = fx0 + cw * (c as f64 + 0.5);
        let cy = fy1 - ch * (r as f64 + 0.5) + LABEL / 2.0;
        let mid = Vec2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        let place = |p: Vec2| Vec2::new(cx + (p.x - mid.x) * scale, cy + (p.y - mid.y) * scale);
        let layer = "VISIBLE";
        let segs: Vec<DxfEntity> = lines.into_iter().map(|(a, b)| line(layer, place(a), place(b))).collect();
        entities.extend(projection::connect(segs));
        let bottom = cy - (hi.y - lo.y) * scale / 2.0;
        entities.push(text(Vec2::new(cx, (bottom - LABEL + 1.0).max(area_y0 + 1.0)), 3.5, view_name(v).to_string()));
    }
    let layers = vec![DxfLayer::new("VISIBLE", 7), DxfLayer::new("FRAME", 7), DxfLayer::new("TEXT", 7)];
    Ok(Sheet { layers, entities, scale, size: (w, h) })
}

fn points(shape: &DxfShape) -> Vec<Vec2> {
    match shape {
        DxfShape::Line(a, b) => vec![*a, *b],
        DxfShape::Polyline { points } => points.clone(),
        _ => Vec::new(),
    }
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let sheet = compose(s, p)?;
    let mut result = json!({"scale": scale_text(sheet.scale), "sheet": [sheet.size.0, sheet.size.1], "entities": sheet.entities.len()});
    if p.get("preview").and_then(Value::as_bool) == Some(true) {
        // Polylines as point lists, texts as [x, y, height, text], all in sheet millimetres.
        let mut paths = Vec::new();
        let mut texts = Vec::new();
        for e in &sheet.entities {
            match &e.shape {
                DxfShape::Text { at, height, text, .. } => texts.push(json!([at.x, at.y, height, text])),
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
    fn the_default_layout_is_front_right_top_home() {
        let c = default_cells();
        assert_eq!(
            c.iter().map(|c| c.view).collect::<Vec<_>>(),
            vec![Some(StandardView::Front), Some(StandardView::Right), Some(StandardView::Top), Some(StandardView::Iso)]
        );
    }
}
