//! The Drawing workspace: a sheet with a grid of views of the model (2×2 by default: Front top
//! left, Right top right, Top bottom left, Home bottom right), each drawn as a wireframe or with
//! hidden lines removed. The sheet is previewed here and saved as DWG (or DXF) through the
//! `drawing.export` command.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use serde_json::{Value, json};

use crate::SolveApp;

/// The Drawing workspace's identity colour (sea green, beside Design's blue and Render's pink).
pub const ACCENT: Color32 = Color32::from_rgb(0, 170, 160);

/// The views a cell can show, as (command value, label).
pub const VIEWS: [(&str, &str); 9] = [
    ("front", "Front"),
    ("back", "Back"),
    ("left", "Left"),
    ("right", "Right"),
    ("top", "Top"),
    ("bottom", "Bottom"),
    ("iso", "Home"),
    // The Design workspace's view as it was left, in perspective when it was.
    ("current", "Current view"),
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
    /// The drawing scale (paper / model); `None` fits the views (Auto).
    pub scale: Option<f64>,
    /// The sheet size, an index into [`SHEETS`].
    pub sheet: usize,
    /// The sheet view: zoom (1 = the whole sheet fits) and pan in screen points.
    zoom: f32,
    pan: Vec2,
    /// The Project Browser's folds before Drawing folded it: given back on return to Design.
    browser_before: Option<(bool, bool)>,
    /// The loading spinner was shown: the sheet is worked out this frame.
    spinner_shown: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            open: false,
            rows: 2,
            cols: 2,
            cells: default_cells(4),
            title: String::new(),
            preview: None,
            key: None,
            error: None,
            zoom: 1.0,
            pan: Vec2::ZERO,
            scale: None,
            sheet: 1,
            browser_before: None,
            spinner_shown: false,
        }
    }
}

/// Sheet sizes (landscape, mm).
pub const SHEETS: [(&str, f64, f64); 5] =
    [("A4", 297.0, 210.0), ("A3", 420.0, 297.0), ("A2", 594.0, 420.0), ("A1", 841.0, 594.0), ("A0", 1189.0, 841.0)];

/// Preferred scales offered in the list (paper / model).
const SCALES: [f64; 15] = [10.0, 5.0, 2.0, 1.0, 0.5, 0.2, 0.1, 0.05, 0.02, 0.01, 0.005, 0.002, 0.001, 0.0005, 0.0002];

fn scale_label(k: f64) -> String {
    let n = |v: f64| {
        let r = (v * 1000.0).round() / 1000.0;
        if (r - r.round()).abs() < 1e-9 { format!("{}", r.round()) } else { format!("{r}") }
    };
    if k >= 1.0 { format!("{}:1", n(k)) } else { format!("1:{}", n(1.0 / k)) }
}

struct Preview {
    size: (f64, f64),
    scale: String,
    fit: String,
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
    // The sheet takes the room of the Project Browser: it folds on the way in and comes back as
    // it was on the way out.
    if open && !app.drawing_ws.open {
        app.drawing_ws.browser_before = Some((app.preferences.browser_collapsed, app.preferences.timeline_left_browser_collapsed));
        app.preferences.browser_collapsed = true;
        app.preferences.timeline_left_browser_collapsed = true;
    } else if !open
        && app.drawing_ws.open
        && let Some((a, b)) = app.drawing_ws.browser_before.take()
    {
        app.preferences.browser_collapsed = a;
        app.preferences.timeline_left_browser_collapsed = b;
    }
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
    let (_, w, h) = SHEETS.get(st.sheet).copied().unwrap_or(SHEETS[1]);
    let scale = st.scale.map_or(json!("auto"), |k| json!(k));
    let mut p = json!({"rows": st.rows, "cols": st.cols, "views": views, "title": st.title, "up": up, "scale": scale, "sheet": [w, h]});
    if st.cells.iter().take(st.rows * st.cols).any(|c| VIEWS.get(c.view).is_some_and(|v| v.0 == "current")) {
        let mut cam = app.cam;
        cam.fov = app.view_fov();
        p["camera"] = json!(cam);
    }
    p
}

/// Is the sheet to be worked out again (the design or the layout changed)?
fn stale(app: &SolveApp) -> bool {
    let key = format!("{}|{}", params(app), app.session.revision);
    app.drawing_ws.key.as_deref() != Some(key.as_str())
}

/// A spinning arc in the middle of `area`: the sheet is being worked out.
fn spinner(ui: &egui::Ui, area: Rect) {
    let t = crate::theme::Tokens::get();
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("sc_drawing_spinner")));
    let c = area.center();
    let r = 22.0;
    painter.circle_filled(c, r + 14.0, t.panel.gamma_multiply(0.92));
    painter.circle_stroke(c, r, egui::Stroke::new(4.0, t.text_dim.gamma_multiply(0.35)));
    let start = (ui.input(|i| i.time) * 5.0) as f32;
    let pts: Vec<Pos2> =
        (0..=24).map(|k| start + k as f32 / 24.0 * std::f32::consts::PI * 1.4).map(|a| Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())).collect();
    painter.add(egui::Shape::line(pts, egui::Stroke::new(4.0, t.accent)));
    painter.text(Pos2::new(c.x, c.y + r + 26.0), Align2::CENTER_CENTER, "Making the drawing…", FontId::proportional(13.0), t.text);
    ui.ctx().request_repaint();
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
            app.drawing_ws.preview = Some(Preview {
                size,
                scale: v["scale"].as_str().unwrap_or("").to_string(),
                fit: v["fit"].as_str().unwrap_or("").to_string(),
                paths,
                texts,
            });
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
    // The settings go in the left column under the floating cards there (the workspace switcher,
    // the browser), and scroll when the room left is short.
    let mut top = full.top();
    let mut sheet_top = full.top();
    let mut width = if app.preferences.drawing_panel_width.is_finite() { app.preferences.drawing_panel_width.clamp(200.0, 600.0) } else { 280.0 };
    let column = Rect::from_min_size(full.min, Vec2::new(width, full.height()));
    for r in app.shell.cards.iter().copied().chain(app.shell.browser) {
        if r.left() < column.right() && r.right() > column.left() && r.bottom() < full.bottom() - 120.0 {
            top = top.max(r.bottom() + 8.0);
            // Only a card of the left column widens it (the ribbon and the tabs strip span the window).
            if r.width() < full.width() * 0.4 {
                width = width.max(r.right() - full.left());
            } else {
                sheet_top = sheet_top.max(r.bottom() + 4.0);
            }
        }
    }
    let width = width.min(full.width() * 0.45);
    let panel = Rect::from_min_max(Pos2::new(full.left(), top), Pos2::new(full.left() + width, full.bottom()));
    let sheet_area = Rect::from_min_max(Pos2::new(panel.right(), sheet_top), full.max).shrink(16.0);
    // Its width is dragged at its right edge, as the docked panels' are.
    let (moved, reset) = crate::dock::edge_handle(ui.ctx(), "sc_drawing_panel_width", panel.right(), panel.top(), panel.bottom());
    if moved != 0.0 {
        app.preferences.drawing_panel_width = (panel.width() + moved).clamp(200.0, 600.0);
    }
    if reset {
        app.preferences.drawing_panel_width = 280.0;
    }
    let mut action = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(panel.shrink(10.0)), |ui| {
        egui::ScrollArea::vertical().id_salt("drawing_ws_panel").show(ui, |ui| {
            ui.heading("Drawing");
            ui.add_space(6.0);
            ui.label("Title");
            ui.text_edit_singleline(&mut app.drawing_ws.title);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Sheet");
                let mut sheet = app.drawing_ws.sheet;
                egui::ComboBox::from_id_salt("drawing_sheet_size").width(70.0).selected_text(SHEETS.get(sheet).map_or("", |s| s.0)).show_ui(
                    ui,
                    |ui| {
                        for (k, (name, w, h)) in SHEETS.iter().enumerate() {
                            ui.selectable_value(&mut sheet, k, format!("{name}  {w:.0}×{h:.0}"));
                        }
                    },
                );
                app.drawing_ws.sheet = sheet;
            });
            ui.horizontal(|ui| {
                ui.label("Scale");
                let fit = app.drawing_ws.preview.as_ref().map(|p| p.fit.clone()).unwrap_or_default();
                let mut scale = app.drawing_ws.scale;
                let shown = scale.map_or_else(|| format!("Auto ({fit})"), scale_label);
                egui::ComboBox::from_id_salt("drawing_scale").width(100.0).selected_text(shown).show_ui(ui, |ui| {
                    ui.selectable_value(&mut scale, None, format!("Auto ({fit})"));
                    for k in SCALES {
                        ui.selectable_value(&mut scale, Some(k), scale_label(k));
                    }
                });
                // Any other scale: 1 : n (or n : 1 above 1:1).
                let mut n = scale.map_or(1.0, |k| if k >= 1.0 { k } else { 1.0 / k });
                let big = scale.is_some_and(|k| k > 1.0);
                ui.label(if big { "" } else { "1 :" });
                if ui.add(egui::DragValue::new(&mut n).range(1.0..=10000.0).speed(1.0).max_decimals(2)).changed() {
                    scale = Some(if big { n } else { 1.0 / n });
                }
                if big {
                    ui.label(": 1");
                }
                app.drawing_ws.scale = scale;
            });
            ui.add_space(6.0);
            ui.separator();
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
            ui.add_space(6.0);
            ui.separator();
            ui.label("Views");
            // A table: a line between the rows and between the columns (cell, view, style).
            let cols = app.drawing_ws.cols;
            let line = egui::Stroke::new(1.0, tk.border);
            egui::Frame::new().stroke(line).corner_radius(tk.radius_sm).inner_margin(egui::Margin::symmetric(0, 2)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let header = |ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        ui.add_sized([34.0, 22.0], egui::Label::new(egui::RichText::new("Cell").small().strong()));
                        ui.separator();
                        ui.add_sized([86.0, 22.0], egui::Label::new(egui::RichText::new("View").small().strong()));
                        ui.separator();
                        ui.add_sized([100.0, 22.0], egui::Label::new(egui::RichText::new("Style").small().strong()));
                    });
                };
                header(ui);
                for i in 0..app.drawing_ws.cells.len() {
                    let Some(mut cell) = app.drawing_ws.cells.get(i).copied() else { continue };
                    let r = ui.available_rect_before_wrap();
                    ui.painter().hline(r.x_range(), r.top(), line);
                    ui.horizontal(|ui| {
                        ui.add_sized([34.0, 26.0], egui::Label::new(format!("{}·{}", i / cols + 1, i % cols + 1)));
                        ui.separator();
                        egui::ComboBox::from_id_salt(("drawing_view", i))
                            .width(80.0)
                            .height(360.0)
                            .selected_text(VIEWS.get(cell.view).map_or("", |v| v.1))
                            .show_ui(ui, |ui| {
                                for (k, (_, label)) in VIEWS.iter().enumerate() {
                                    ui.selectable_value(&mut cell.view, k, *label);
                                }
                            });
                        ui.separator();
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
            });
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(12.0);
            if let Some(p) = &app.drawing_ws.preview {
                ui.label(format!("Sheet {:.0}×{:.0} mm, scale {}", p.size.0, p.size.1, p.scale));
            }
            ui.horizontal(|ui| {
                ui.label(format!("Zoom {:.0}%", app.drawing_ws.zoom * 100.0));
                if ui.button("Fit").clicked() {
                    app.drawing_ws.zoom = 1.0;
                    app.drawing_ws.pan = Vec2::ZERO;
                }
            });
            ui.small("Wheel: zoom · middle button: pan");
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
    // A sheet still to work out: the spinner shows first, the work runs on the next frame.
    if stale(app) && !app.drawing_ws.spinner_shown {
        app.drawing_ws.spinner_shown = true;
        spinner(ui, sheet_area);
        return;
    }
    if app.drawing_ws.spinner_shown {
        spinner(ui, sheet_area);
    }
    refresh(app);
    app.drawing_ws.spinner_shown = false;
    let painter = ui.painter_at(sheet_area);
    let Some(sheet_size) = app.drawing_ws.preview.as_ref().map(|p| p.size) else {
        painter.text(
            sheet_area.center(),
            Align2::CENTER_CENTER,
            "Nothing to draw: the design has no visible bodies.",
            FontId::proportional(14.0),
            tk.text,
        );
        return;
    };
    let (w, h) = (sheet_size.0.max(1.0), sheet_size.1.max(1.0));
    // The mouse wheel zooms about the pointer, the middle button drags the sheet, a double
    // click of the middle button fits it again.
    let resp = ui.interact(sheet_area, ui.id().with("drawing_sheet"), Sense::click_and_drag());
    // At 100 % the sheet fits and sits against the settings, centred vertically.
    let fit = (sheet_area.width() as f64 / w).min(sheet_area.height() as f64 / h);
    let anchor = Pos2::new(sheet_area.left() + (w * fit) as f32 / 2.0, sheet_area.center().y);
    let (scroll, pointer, middle_drag, middle_double) = ui.input(|i| {
        let scroll = i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0;
        (
            scroll,
            i.pointer.hover_pos(),
            if i.pointer.button_down(egui::PointerButton::Middle) { i.pointer.delta() } else { Vec2::ZERO },
            i.pointer.button_double_clicked(egui::PointerButton::Middle),
        )
    });
    let st = &mut app.drawing_ws;
    if resp.hovered() {
        if scroll.abs() > 0.0
            && let Some(at) = pointer
        {
            let old = st.zoom;
            st.zoom = (st.zoom * (scroll * 0.0025).exp()).clamp(0.2, 40.0);
            // Keep the sheet point under the pointer where it is.
            let c = anchor + st.pan;
            st.pan += (at - c) * (1.0 - st.zoom / old);
        }
        st.pan += middle_drag;
        if middle_double {
            st.zoom = 1.0;
            st.pan = Vec2::ZERO;
        }
    }
    let (zoom, pan) = (st.zoom, st.pan);
    let Some(p) = &app.drawing_ws.preview else { return };
    let k = (sheet_area.width() as f64 / w).min(sheet_area.height() as f64 / h) * f64::from(zoom);
    let size = Vec2::new((w * k) as f32, (h * k) as f32);
    let paper = Rect::from_center_size(anchor + pan, size);
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
