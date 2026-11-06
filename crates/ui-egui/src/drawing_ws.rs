//! The Drawing workspace: a sheet with a grid of views of the model (2×2 by default: Front top
//! left, Right top right, Top bottom left, Home bottom right), each drawn as a wireframe or with
//! hidden lines removed. The sheet is previewed here and saved as DWG (or DXF) through the
//! `drawing.export` command.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use serde_json::{Value, json};

use crate::SolveApp;

/// The views a cell can show, as (command value, label).
pub const VIEWS: [(&str, &str); 8] = [
    ("front", "Front"),
    ("back", "Back"),
    ("left", "Left"),
    ("right", "Right"),
    ("top", "Top"),
    ("bottom", "Bottom"),
    ("iso", "Home"),
    ("none", "Empty"),
];

/// One cell: an index into [`VIEWS`] and whether hidden lines are removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub view: usize,
    pub hidden_line: bool,
}

pub struct State {
    pub open: bool,
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<Cell>,
    pub title: String,
    preview: Option<Preview>,
    key: Option<String>,
    error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        State { open: false, rows: 2, cols: 2, cells: default_cells(4), title: String::new(), preview: None, key: None, error: None }
    }
}

struct Preview {
    size: (f64, f64),
    scale: String,
    paths: Vec<(bool, Vec<[f64; 2]>)>,
    texts: Vec<([f64; 2], f64, String)>,
}

fn view_index(name: &str) -> usize {
    VIEWS.iter().position(|(v, _)| *v == name).unwrap_or(VIEWS.len() - 1)
}

/// The default cells, row by row: Front, Right / Top, Home, then empty cells.
pub fn default_cells(n: usize) -> Vec<Cell> {
    let order = ["front", "right", "top", "iso"];
    (0..n).map(|i| Cell { view: view_index(order.get(i).copied().unwrap_or("none")), hidden_line: true }).collect()
}

/// Open or close the workspace.
pub fn set_open(app: &mut SolveApp, open: bool) {
    app.drawing_ws.open = open;
    app.drawing_ws.key = None;
    app.drawing_ws.preview = None;
    app.drawing_ws.error = None;
    if open && app.drawing_ws.title.is_empty() {
        app.drawing_ws.title = app.session.doc.name.clone();
    }
}

pub fn active(app: &SolveApp) -> bool {
    app.drawing_ws.open
}

/// The `drawing.export` parameters for the current layout.
pub fn params(app: &SolveApp) -> Value {
    let st = &app.drawing_ws;
    let views: Vec<Value> = st
        .cells
        .iter()
        .take(st.rows * st.cols)
        .map(|c| json!({"view": VIEWS.get(c.view).map_or("none", |v| v.0), "style": if c.hidden_line {"hidden_line"} else {"wireframe"}}))
        .collect();
    let up = if matches!(app.cam.up, solidcraft_engine::render::UpAxis::Y) { "y" } else { "z" };
    json!({"rows": st.rows, "cols": st.cols, "views": views, "title": st.title, "up": up})
}

fn refresh(app: &mut SolveApp) {
    let p = params(app);
    let key = format!("{p}|{}", app.session.revision);
    if app.drawing_ws.key.as_deref() == Some(key.as_str()) {
        return;
    }
    app.drawing_ws.key = Some(key);
    let mut q = p;
    q["preview"] = json!(true);
    match app.run("drawing.export", q) {
        Ok(v) => {
            let f = |v: &Value| v.as_f64().unwrap_or(0.0);
            let size = (v["sheet"].get(0).map_or(420.0, f), v["sheet"].get(1).map_or(297.0, f));
            let paths = v["paths"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|p| {
                            let pts = p["points"]
                                .as_array()
                                .map(|pts| pts.iter().map(|q| [q.get(0).map_or(0.0, f), q.get(1).map_or(0.0, f)]).collect())
                                .unwrap_or_default();
                            (p["layer"].as_str() == Some("FRAME"), pts)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let texts = v["texts"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|t| {
                            (
                                [t.get(0).map_or(0.0, f), t.get(1).map_or(0.0, f)],
                                t.get(2).map_or(3.5, f),
                                t.get(3).and_then(Value::as_str).unwrap_or("").to_string(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            app.drawing_ws.preview = Some(Preview { size, scale: v["scale"].as_str().unwrap_or("").to_string(), paths, texts });
            app.drawing_ws.error = None;
        }
        Err(e) => {
            app.drawing_ws.preview = None;
            app.drawing_ws.error = Some(e);
        }
    }
}

/// Save the sheet: asks for a file and writes DWG (or DXF by the extension).
pub fn export(app: &mut SolveApp) {
    let p = params(app);
    let name = format!("{}-drawing.dwg", app.session.doc.name);
    app.pick_save(&name, &["dwg", "dxf"], move |app, path| {
        let Some(path) = path else { return };
        let mut q = p;
        q["path"] = json!(path);
        match app.run("drawing.export", q) {
            Ok(v) => app.set_status(format!("Drawing saved: {}", v["path"].as_str().unwrap_or("")), false),
            Err(e) => app.set_status(e, true),
        }
    });
}

fn resize(st: &mut State, rows: usize, cols: usize) {
    let old = std::mem::take(&mut st.cells);
    let (or, oc) = (st.rows, st.cols);
    let defaults = default_cells(rows * cols);
    st.cells = (0..rows * cols)
        .map(|i| {
            let (r, c) = (i / cols, i % cols);
            if r < or && c < oc {
                old.get(r * oc + c).copied().unwrap_or(Cell { view: VIEWS.len() - 1, hidden_line: true })
            } else {
                defaults.get(i).copied().unwrap_or(Cell { view: VIEWS.len() - 1, hidden_line: true })
            }
        })
        .collect();
    st.rows = rows;
    st.cols = cols;
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let full = ui.max_rect();
    app.viewport.rect = None;
    let tk = crate::theme::Tokens::get();
    ui.painter().rect_filled(full, 0.0, tk.viewport_bottom);
    let panel = Rect::from_min_size(full.min, Vec2::new(280.0_f32.min(full.width() * 0.4), full.height()));
    let sheet_area = Rect::from_min_max(Pos2::new(panel.right(), full.top()), full.max).shrink(16.0);
    let mut action = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(panel.shrink(10.0)), |ui| {
        egui::ScrollArea::vertical().id_salt("drawing_ws_panel").show(ui, |ui| {
            ui.heading("Drawing");
            ui.add_space(6.0);
            ui.label("Title");
            ui.text_edit_singleline(&mut app.drawing_ws.title);
            ui.add_space(6.0);
            ui.label("Grid");
            let (mut rows, mut cols) = (app.drawing_ws.rows, app.drawing_ws.cols);
            ui.horizontal(|ui| {
                ui.label("Rows");
                ui.add(egui::DragValue::new(&mut rows).range(1..=4));
                ui.label("Columns");
                ui.add(egui::DragValue::new(&mut cols).range(1..=4));
            });
            ui.horizontal_wrapped(|ui| {
                for (r, c) in [(1, 1), (1, 2), (2, 1), (2, 2), (2, 3), (3, 3)] {
                    if ui.selectable_label(rows == r && cols == c, format!("{r}×{c}")).clicked() {
                        (rows, cols) = (r, c);
                    }
                }
            });
            if (rows, cols) != (app.drawing_ws.rows, app.drawing_ws.cols) {
                resize(&mut app.drawing_ws, rows, cols);
            }
            if ui.button("Default layout").clicked() {
                app.drawing_ws.rows = 2;
                app.drawing_ws.cols = 2;
                app.drawing_ws.cells = default_cells(4);
            }
            ui.add_space(8.0);
            ui.label("Views");
            let cols = app.drawing_ws.cols;
            for i in 0..app.drawing_ws.cells.len() {
                let Some(mut cell) = app.drawing_ws.cells.get(i).copied() else { continue };
                ui.horizontal(|ui| {
                    ui.label(format!("{}·{}", i / cols + 1, i % cols + 1));
                    egui::ComboBox::from_id_salt(("drawing_view", i)).width(80.0).selected_text(VIEWS.get(cell.view).map_or("", |v| v.1)).show_ui(
                        ui,
                        |ui| {
                            for (k, (_, label)) in VIEWS.iter().enumerate() {
                                ui.selectable_value(&mut cell.view, k, *label);
                            }
                        },
                    );
                    egui::ComboBox::from_id_salt(("drawing_style", i))
                        .width(96.0)
                        .selected_text(if cell.hidden_line { "Hidden line" } else { "Wireframe" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut cell.hidden_line, false, "Wireframe");
                            ui.selectable_value(&mut cell.hidden_line, true, "Hidden line");
                        });
                });
                if let Some(c) = app.drawing_ws.cells.get_mut(i) {
                    *c = cell;
                }
            }
            ui.add_space(12.0);
            if let Some(p) = &app.drawing_ws.preview {
                ui.label(format!("Sheet {:.0}×{:.0} mm, scale {}", p.size.0, p.size.1, p.scale));
            }
            if let Some(e) = &app.drawing_ws.error {
                ui.colored_label(tk.error, e.as_str());
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(app.drawing_ws.preview.is_some(), egui::Button::new("Export DWG…")).clicked() {
                    action = Some(true);
                }
                if ui.button("Close").clicked() {
                    action = Some(false);
                }
            });
        });
    });
    match action {
        Some(true) => export(app),
        Some(false) => {
            set_open(app, false);
            return;
        }
        None => {}
    }
    refresh(app);
    let painter = ui.painter_at(sheet_area);
    let Some(p) = &app.drawing_ws.preview else {
        painter.text(
            sheet_area.center(),
            Align2::CENTER_CENTER,
            "Nothing to draw: the design has no visible bodies.",
            FontId::proportional(14.0),
            tk.text,
        );
        return;
    };
    let (w, h) = (p.size.0.max(1.0), p.size.1.max(1.0));
    let k = (sheet_area.width() as f64 / w).min(sheet_area.height() as f64 / h);
    let size = Vec2::new((w * k) as f32, (h * k) as f32);
    let paper = Rect::from_center_size(sheet_area.center(), size);
    ui.interact(paper, ui.id().with("drawing_sheet"), Sense::hover());
    painter.rect_filled(paper.translate(Vec2::splat(3.0)), 0.0, Color32::from_black_alpha(60));
    painter.rect_filled(paper, 0.0, Color32::WHITE);
    let to = |q: [f64; 2]| Pos2::new(paper.left() + (q[0] * k) as f32, paper.bottom() - (q[1] * k) as f32);
    // The cell grid, as a guide only (not exported).
    let (st_rows, st_cols) = (app.drawing_ws.rows, app.drawing_ws.cols);
    let (x0, x1, y0, y1) = (10.0, w - 10.0, 10.0 + 14.0, h - 10.0);
    let guide = Stroke::new(1.0, Color32::from_rgb(170, 200, 235));
    for c in 1..st_cols {
        let x = x0 + (x1 - x0) * c as f64 / st_cols as f64;
        painter.line_segment([to([x, y0]), to([x, y1])], guide);
    }
    for r in 1..st_rows {
        let y = y0 + (y1 - y0) * r as f64 / st_rows as f64;
        painter.line_segment([to([x0, y]), to([x1, y])], guide);
    }
    for (frame, pts) in &p.paths {
        let stroke = Stroke::new(if *frame { 1.4 } else { 1.0 }, Color32::BLACK);
        let line: Vec<Pos2> = pts.iter().map(|q| to(*q)).collect();
        painter.add(egui::Shape::line(line, stroke));
    }
    for (at, height, text) in &p.texts {
        let px = ((height * k) as f32 * 1.35).clamp(6.0, 40.0);
        painter.text(to(*at), Align2::CENTER_BOTTOM, text, FontId::proportional(px), Color32::BLACK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cells_are_front_right_top_home() {
        let c = default_cells(4);
        let names: Vec<&str> = c.iter().map(|c| VIEWS[c.view].0).collect();
        assert_eq!(names, ["front", "right", "top", "iso"]);
    }

    #[test]
    fn resizing_keeps_cells_in_place() {
        let mut st = State::default();
        st.cells[1].hidden_line = false;
        resize(&mut st, 2, 3);
        assert_eq!(st.cells.len(), 6);
        assert!(!st.cells[1].hidden_line);
        assert_eq!(VIEWS[st.cells[3].view].0, "top");
        resize(&mut st, 1, 1);
        assert_eq!(VIEWS[st.cells[0].view].0, "front");
    }

    #[test]
    fn the_sheet_previews_and_saves_as_dwg() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":40,"width":30,"height":10})).unwrap();
        h.call("ui.workspace", json!({"workspace":"drawing"}));
        assert!(active(&h.app));
        h.frames(2);
        let p = h.app.drawing_ws.preview.as_ref().expect("preview");
        assert!(p.paths.len() > 4, "{}", p.paths.len());
        assert!(p.texts.iter().any(|t| t.2 == "FRONT"));
        let path = h.dir.join("sheet.dwg");
        let mut q = params(&h.app);
        q["path"] = json!(path.to_string_lossy());
        h.app.run("drawing.export", q).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"AC10"));
        h.call("ui.workspace", json!({"workspace":"design"}));
        assert!(!active(&h.app));
    }
}
