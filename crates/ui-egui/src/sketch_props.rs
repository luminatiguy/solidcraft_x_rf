//! Properties for what is selected in the open sketch: a line's ends, length and angle; a
//! circle's or an arc's centre and radius. Typed values apply on Enter through the sketch
//! commands the drawing tools use (`sketch.move_point`, `sketch.move`, `sketch.scale`).

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::CurveKind;

use crate::SolveApp;

/// One row: its label, its value as shown, and what typing a new value does.
struct Row {
    label: &'static str,
    value: f64,
    unit: &'static str,
    edit: Edit,
}

#[derive(Clone)]
enum Edit {
    /// Move point `id` to `(x, y)` with this coordinate replaced (0: x, 1: y).
    Point {
        id: String,
        at: Vec2,
        axis: usize,
    },
    /// A line from `a` (fixed) to point `b`: the new length or angle (degrees).
    Length {
        b: String,
        a: Vec2,
        dir: Vec2,
    },
    Angle {
        b: String,
        a: Vec2,
        len: f64,
    },
    /// A circle or arc around `c`: the whole curve moves (centre coordinate) or scales (radius).
    Centre {
        curve: String,
        c: Vec2,
        axis: usize,
    },
    Radius {
        curve: String,
        c: Vec2,
        r: f64,
    },
    None,
}

/// The selected curves of the open sketch.
fn selected(app: &SolveApp) -> Vec<String> {
    let Some(active) = app.session.active_sketch else { return Vec::new() };
    app.session
        .selection
        .iter()
        .filter_map(|s| match s {
            Sel::SketchCurve { id, sketch } if sketch.is_none_or(|k| k == active) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

fn rows(app: &SolveApp, curve: &str) -> Option<(&'static str, Vec<Row>)> {
    let sid = app.session.active_sketch?;
    let st = crate::open_meshing::shown_world(app);
    let sk = &st.sketch(sid)?.sketch;
    let c = sk.curves.get(sk.curve_index(curve)?)?;
    let pid = |k: usize| sk.points.get(k).map(|p| p.id.clone());
    Some(match c.kind {
        CurveKind::Line { a, b } => {
            let (pa, pb) = (sk.point(a)?, sk.point(b)?);
            let d = pb - pa;
            let len = d.len();
            let dir = if len > 1e-12 { d * (1.0 / len) } else { Vec2::new(1.0, 0.0) };
            let (ia, ib) = (pid(a)?, pid(b)?);
            (
                "Line",
                vec![
                    Row { label: "Start X", value: pa.x, unit: "mm", edit: Edit::Point { id: ia.clone(), at: pa, axis: 0 } },
                    Row { label: "Start Y", value: pa.y, unit: "mm", edit: Edit::Point { id: ia, at: pa, axis: 1 } },
                    Row { label: "End X", value: pb.x, unit: "mm", edit: Edit::Point { id: ib.clone(), at: pb, axis: 0 } },
                    Row { label: "End Y", value: pb.y, unit: "mm", edit: Edit::Point { id: ib.clone(), at: pb, axis: 1 } },
                    Row { label: "Length", value: len, unit: "mm", edit: Edit::Length { b: ib.clone(), a: pa, dir } },
                    Row { label: "Angle", value: d.y.atan2(d.x).to_degrees().rem_euclid(360.0), unit: "°", edit: Edit::Angle { b: ib, a: pa, len } },
                ],
            )
        }
        CurveKind::Circle { c: k, r } => {
            let pc = sk.point(k)?;
            (
                "Circle",
                vec![
                    Row { label: "Centre X", value: pc.x, unit: "mm", edit: Edit::Centre { curve: curve.into(), c: pc, axis: 0 } },
                    Row { label: "Centre Y", value: pc.y, unit: "mm", edit: Edit::Centre { curve: curve.into(), c: pc, axis: 1 } },
                    Row { label: "Radius", value: r, unit: "mm", edit: Edit::Radius { curve: curve.into(), c: pc, r } },
                    Row { label: "Diameter", value: 2.0 * r, unit: "mm", edit: Edit::None },
                    Row { label: "Length", value: std::f64::consts::TAU * r, unit: "mm", edit: Edit::None },
                ],
            )
        }
        CurveKind::Arc { c: k, a, b } => {
            let (pc, pa, pb) = (sk.point(k)?, sk.point(a)?, sk.point(b)?);
            let r = pa.dist(pc);
            let a0 = (pa - pc).y.atan2((pa - pc).x);
            let a1 = (pb - pc).y.atan2((pb - pc).x);
            let sweep = (a1 - a0).rem_euclid(std::f64::consts::TAU);
            (
                "Arc",
                vec![
                    Row { label: "Centre X", value: pc.x, unit: "mm", edit: Edit::Centre { curve: curve.into(), c: pc, axis: 0 } },
                    Row { label: "Centre Y", value: pc.y, unit: "mm", edit: Edit::Centre { curve: curve.into(), c: pc, axis: 1 } },
                    Row { label: "Radius", value: r, unit: "mm", edit: Edit::Radius { curve: curve.into(), c: pc, r } },
                    Row { label: "Start angle", value: a0.to_degrees().rem_euclid(360.0), unit: "°", edit: Edit::None },
                    Row { label: "Sweep", value: sweep.to_degrees(), unit: "°", edit: Edit::None },
                    Row { label: "Length", value: sweep * r, unit: "mm", edit: Edit::None },
                ],
            )
        }
        CurveKind::Ellipse { .. } => ("Ellipse", Vec::new()),
        CurveKind::Spline { .. } => ("Spline", Vec::new()),
        CurveKind::Conic { .. } => ("Conic", Vec::new()),
    })
}

fn fmt(v: f64) -> String {
    let r = (v * 1e4).round() / 1e4;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

/// Properties for the sketch's selection. True when it drew something.
pub fn ui(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    let curves = selected(app);
    if curves.is_empty() {
        return false;
    }
    let mut apply: Option<(Edit, f64)> = None;
    if let [one] = curves.as_slice() {
        let Some((kind, rows)) = rows(app, one) else { return false };
        let construction = {
            let st = crate::open_meshing::shown_world(app);
            app.session
                .active_sketch
                .and_then(|s| st.sketch(s))
                .and_then(|ss| ss.sketch.curves.iter().find(|c| &c.id == one))
                .is_some_and(|c| c.construction)
        };
        ui.label(egui::RichText::new(if construction { format!("{kind} (construction)") } else { kind.to_string() }).strong());
        egui::Grid::new("sc_sketch_props").num_columns(3).spacing([10.0, 6.0]).show(ui, |ui| {
            for (k, row) in rows.iter().enumerate() {
                ui.label(row.label);
                let key = 2000 + k;
                let shown = fmt(row.value);
                if matches!(row.edit, Edit::None) {
                    ui.label(&shown);
                } else {
                    let editing = app.shell_ui.editing.as_ref().filter(|(f, i, _)| *f == 0 && *i == key).map(|e| e.2.clone());
                    let mut text = editing.unwrap_or(shown.clone());
                    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(110.0));
                    crate::scenario::publish_target(&format!("sketchprops:{}", row.label), r.rect);
                    if r.changed() || r.has_focus() {
                        app.shell_ui.editing = Some((0, key, text.clone()));
                    }
                    if r.lost_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && text.trim() != shown
                            && let Some(v) = text
                                .trim()
                                .trim_end_matches(row.unit)
                                .trim()
                                .trim_end_matches("mm")
                                .trim()
                                .parse::<f64>()
                                .ok()
                                .filter(|v| v.is_finite())
                        {
                            apply = Some((row.edit.clone(), v));
                        }
                        app.shell_ui.editing = None;
                    }
                }
                ui.label(egui::RichText::new(row.unit).weak());
                ui.end_row();
            }
        });
    } else {
        let total: f64 =
            curves.iter().filter_map(|c| rows(app, c)).filter_map(|(_, r)| r.iter().find(|x| x.label == "Length").map(|x| x.value)).sum();
        ui.label(egui::RichText::new(format!("{} curves", curves.len())).strong());
        egui::Grid::new("sc_sketch_props_many").num_columns(2).show(ui, |ui| {
            ui.label("Total length");
            ui.label(format!("{} mm", fmt(total)));
            ui.end_row();
        });
    }
    ui.label(egui::RichText::new("Type a value and press Enter.").small().weak());
    if let Some((edit, v)) = apply {
        let done = run_edit(app, &edit, v);
        match done {
            Ok(()) => app.set_status("Applied", false),
            Err(e) => app.set_status(e, true),
        }
    }
    true
}

fn xy(p: Vec2) -> Value {
    json!([p.x, p.y])
}

fn run_edit(app: &mut SolveApp, edit: &Edit, v: f64) -> Result<(), String> {
    match edit {
        Edit::Point { id, at, axis } => {
            let to = if *axis == 0 { Vec2::new(v, at.y) } else { Vec2::new(at.x, v) };
            app.run("sketch.move_point", json!({"point": id, "to": xy(to)})).map(|_| ())
        }
        Edit::Length { b, a, dir } => {
            if v <= 1e-9 {
                return Err("The length must be above zero".into());
            }
            app.run("sketch.move_point", json!({"point": b, "to": xy(*a + *dir * v)})).map(|_| ())
        }
        Edit::Angle { b, a, len } => {
            let t = v.to_radians();
            app.run("sketch.move_point", json!({"point": b, "to": xy(*a + Vec2::new(t.cos(), t.sin()) * *len)})).map(|_| ())
        }
        Edit::Centre { curve, c, axis } => {
            let d = if *axis == 0 { [v - c.x, 0.0] } else { [0.0, v - c.y] };
            app.run("sketch.move", json!({"entities": [curve], "translate": d})).map(|_| ())
        }
        Edit::Radius { curve, c, r } => {
            if v <= 1e-9 || *r <= 1e-12 {
                return Err("The radius must be above zero".into());
            }
            app.run("sketch.scale", json!({"entities": [curve], "base": xy(*c), "factor": v / r})).map(|_| ())
        }
        Edit::None => Ok(()),
    }
}
