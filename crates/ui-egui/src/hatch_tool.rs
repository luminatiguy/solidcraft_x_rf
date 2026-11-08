//! The SURFACE tab's Hatch Pattern tool: choose a pattern (SolidCraft's own, or those of a
//! folder of `.pat` files set in Preferences ▸ Hatch Patterns), its scale and angle, hover a face
//! to preview the pattern on it and click to apply it (`hatch.apply`); Shift+click takes a face's
//! pattern away (`hatch.remove`). Esc or the button again ends the tool.

use egui::{Color32, Pos2, Stroke};
use serde_json::{Value, json};
use solidcraft_engine::hatch::{self, Hatch, Pattern};

use crate::SolveApp;
use crate::viewport::Hit;

pub const COMMAND: &str = "hatch.pattern";

#[derive(Debug)]
pub struct State {
    pub active: bool,
    /// Folder of `.pat` files (Preferences ▸ Hatch Patterns).
    pub folder: String,
    pub pattern: String,
    pub scale: f64,
    pub angle: f64,
    patterns: Vec<Pattern>,
    loaded: Option<String>,
    /// The face under the pointer: body, face, point.
    hover: Option<(String, usize, solidcore_record::Vec3)>,
    /// A select.clear owed after a click (the viewport selects what was clicked that frame).
    clear_selection: bool,
    /// Whole-object picking, off while the tool lights single faces: given back at the end.
    restore_pick_bodies: Option<bool>,
}

impl Default for State {
    fn default() -> Self {
        State {
            active: false,
            folder: String::new(),
            pattern: "SC_CROSS".into(),
            scale: 1.0,
            angle: 0.0,
            patterns: Vec::new(),
            loaded: None,
            hover: None,
            clear_selection: false,
            restore_pick_bodies: None,
        }
    }
}

pub fn active(app: &SolveApp) -> bool {
    app.hatch.active
}

/// The patterns: SolidCraft's own, then the folder's (read again when the folder changes).
pub fn patterns(app: &mut SolveApp) -> &[Pattern] {
    if app.hatch.loaded.as_deref() != Some(app.hatch.folder.as_str()) || app.hatch.patterns.is_empty() {
        let mut all = hatch::builtin();
        let folder = app.hatch.folder.trim().to_string();
        if !folder.is_empty() {
            for p in solidcraft_engine::cmd::hatch::folder_patterns(&folder) {
                if !all.iter().any(|x| x.name.eq_ignore_ascii_case(&p.name)) {
                    all.push(p);
                }
            }
        }
        app.hatch.patterns = all;
        app.hatch.loaded = Some(app.hatch.folder.clone());
    }
    &app.hatch.patterns
}

fn current(app: &mut SolveApp) -> Option<Pattern> {
    let name = app.hatch.pattern.clone();
    let all = patterns(app);
    all.iter().find(|p| p.name == name).or(all.first()).cloned()
}

pub fn start_hook(app: &mut SolveApp, id: &str) -> bool {
    if id != COMMAND {
        return false;
    }
    if app.hatch.active {
        stop(app, "Hatch Pattern ended");
        return true;
    }
    if app.session.active_sketch.is_some() {
        app.set_status("Hatch Pattern: finish the sketch first", true);
        return true;
    }
    crate::dialogs::cancel(app);
    app.dialog = None;
    app.tool = None;
    app.hatch.active = true;
    let _ = app.session.execute("select.clear", &json!({}));
    app.hatch.restore_pick_bodies = Some(app.ui.pick_bodies);
    app.ui.pick_bodies = false;
    let _ = patterns(app);
    if current(app).is_some_and(|p| p.name != app.hatch.pattern)
        && let Some(p) = current(app)
    {
        app.hatch.pattern = p.name;
    }
    app.set_status("Hatch Pattern: click a face to apply the pattern; Shift+click takes it away; Esc ends", false);
    true
}

fn stop(app: &mut SolveApp, why: &str) {
    app.hatch.active = false;
    app.hatch.hover = None;
    if let Some(p) = app.hatch.restore_pick_bodies.take() {
        app.ui.pick_bodies = p;
    }
    app.set_status(why, false);
}

pub fn pressed(app: &SolveApp, id: &str) -> bool {
    id == COMMAND && app.hatch.active
}

/// The face the tool would hatch: the first face of the hits under the pointer.
pub fn set_hover(app: &mut SolveApp, hits: &[Hit]) {
    app.hatch.hover = hits.iter().find_map(|h| match h {
        Hit::Face { body, index, point } => Some((body.clone(), *index, *point)),
        _ => None,
    });
}

/// Once a frame: Esc, the hovered face, and the click that applies.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if std::mem::take(&mut app.hatch.clear_selection) {
        let _ = app.session.execute("select.clear", &json!({}));
    }
    if !app.hatch.active {
        return;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) || app.session.active_sketch.is_some() || app.dialog.is_some() {
        stop(app, "Hatch Pattern ended");
        return;
    }
    let (click, shift) = ctx.input(|i| (if i.pointer.primary_clicked() { i.pointer.interact_pos() } else { None }, i.modifiers.shift));
    let Some(pos) = click else { return };
    if !app.viewport.rect.is_some_and(|r| r.contains(pos)) {
        return;
    }
    let Some((body, face, at)) = app.hatch.hover.clone() else { return };
    app.hatch.clear_selection = true;
    if shift {
        match app.run("hatch.remove", json!({"body": body, "face": face})) {
            Ok(r) if r["removed"].as_u64().unwrap_or(0) > 0 => app.set_status(format!("Hatch taken off face {face} of {body}"), false),
            Ok(_) => app.set_status("That face has no hatch pattern", false),
            Err(e) => app.set_status(format!("Hatch Pattern: {e}"), true),
        }
        return;
    }
    let Some(pattern) = current(app) else { return };
    let params = json!({
        "body": body, "face": face, "at": [at.x, at.y, at.z],
        "pattern": pattern, "scale": app.hatch.scale, "angle": app.hatch.angle,
    });
    match app.run("hatch.apply", params) {
        Ok(_) => app.set_status(format!("{} applied to face {face} of {body}", pattern.name), false),
        Err(e) => app.set_status(format!("Hatch Pattern: {e}"), true),
    }
}

/// The hatch the tool would apply to the face under the pointer (drawn as a preview).
pub fn preview(app: &SolveApp) -> Option<Hatch> {
    if !app.hatch.active {
        return None;
    }
    let (body, face, at) = app.hatch.hover.as_ref()?;
    let name = &app.hatch.pattern;
    let pattern = app.hatch.patterns.iter().find(|p| &p.name == name).or(app.hatch.patterns.first())?.clone();
    Some(Hatch { body: body.clone(), face: *face, at: [at.x, at.y, at.z], pattern, scale: app.hatch.scale, angle: app.hatch.angle })
}

/// What the preview depends on (the model scene is rebuilt when it changes).
pub fn preview_key(app: &SolveApp) -> Option<(String, usize, String, u64, u64)> {
    let h = preview(app)?;
    Some((h.body, h.face, h.pattern.name, h.scale.to_bits(), h.angle.to_bits()))
}

/// A small picture of a pattern: its lines in a square.
fn swatch(ui: &mut egui::Ui, pattern: &Pattern, size: f32, ink: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    // The pattern laid on a square face 4 units wide (about one or two repeats of most
    // patterns), scaled to the swatch.
    let span = pattern_span(pattern);
    let fams = hatch::families(&Hatch { body: String::new(), face: 0, at: [0.0; 3], pattern: pattern.clone(), scale: 1.0, angle: 0.0 });
    let mut segs = Vec::new();
    let tri = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        [solidcore_record::Vec2::new(a.0, a.1), solidcore_record::Vec2::new(b.0, b.1), solidcore_record::Vec2::new(c.0, c.1)]
    };
    for t in [tri((0.0, 0.0), (span, 0.0), (span, span)), tri((0.0, 0.0), (span, span), (0.0, span))] {
        hatch_segments_2d(&fams, t, &mut segs);
    }
    let k = size as f64 / span;
    let to = |v: solidcore_record::Vec2| Pos2::new(rect.left() + (v.x * k) as f32, rect.bottom() - (v.y * k) as f32);
    for (a, b) in segs.into_iter().take(4000) {
        painter.line_segment([to(a), to(b)], Stroke::new(1.0, ink));
    }
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);
    resp
}

fn pattern_span(p: &Pattern) -> f64 {
    let step = p.lines.iter().map(|l| l.dy.abs().max(l.dashes.iter().map(|d| d.abs()).sum::<f64>())).fold(0.0f64, f64::max);
    if step.is_finite() && step > 0.0 { (step * 3.0).clamp(1e-3, 1e6) } else { 10.0 }
}

fn hatch_segments_2d(fams: &[hatch::PatLine], tri: [solidcore_record::Vec2; 3], out: &mut Vec<(solidcore_record::Vec2, solidcore_record::Vec2)>) {
    hatch::tri_segments(fams, tri, out);
}

/// The tool's window: the pattern, scale and angle.
pub fn window(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.hatch.active {
        return;
    }
    let mut open = true;
    let ink = ctx.global_style().visuals.text_color();
    let list: Vec<Pattern> = patterns(app).to_vec();
    // Beside the browser, under the ribbon and the document tabs.
    let anchor = app.viewport.rect.map_or(Pos2::new(280.0, 240.0), |r| Pos2::new(r.left() + 280.0, r.top() + 240.0));
    egui::Window::new("Hatch Pattern")
        .id(egui::Id::new("sc_hatch_tool_window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos(anchor)
        .constrain(true)
        .default_width(260.0)
        .show(ctx, |ui| {
            crate::scenario::publish_target("hatch:window", ui.max_rect());
            ui.horizontal(|ui| {
                if let Some(p) = list.iter().find(|p| p.name == app.hatch.pattern) {
                    swatch(ui, p, 40.0, ink);
                }
                ui.vertical(|ui| {
                    ui.label("Pattern");
                    let shown = app.hatch.pattern.clone();
                    egui::ComboBox::from_id_salt("sc_hatch_pattern").width(170.0).selected_text(shown).show_ui(ui, |ui| {
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for p in &list {
                                ui.horizontal(|ui| {
                                    let r = swatch(ui, p, 22.0, ink);
                                    let text = if p.description.is_empty() { p.name.clone() } else { format!("{}  ·  {}", p.name, p.description) };
                                    let row = ui.selectable_label(app.hatch.pattern == p.name, text);
                                    if row.clicked() || r.clicked() {
                                        app.hatch.pattern = p.name.clone();
                                    }
                                });
                            }
                        });
                    });
                });
            });
            ui.add_space(4.0);
            egui::Grid::new("sc_hatch_values").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Scale");
                let r = ui.add(egui::DragValue::new(&mut app.hatch.scale).speed(0.05).range(0.001..=100_000.0).max_decimals(4));
                crate::scenario::publish_target("hatch:scale", r.rect);
                ui.end_row();
                ui.label("Angle");
                let r = ui.add(egui::DragValue::new(&mut app.hatch.angle).speed(1.0).range(-360.0..=360.0).suffix("°").max_decimals(2));
                crate::scenario::publish_target("hatch:angle", r.rect);
                ui.end_row();
            });
            ui.add_space(4.0);
            let folder = if app.hatch.folder.trim().is_empty() { "SolidCraft patterns only".to_string() } else { app.hatch.folder.clone() };
            ui.label(egui::RichText::new(format!("Library: {folder}")).small().weak());
            ui.horizontal(|ui| {
                if ui.button("Pattern folder…").on_hover_text("Preferences ▸ Hatch Patterns").clicked() {
                    open_preferences(app);
                }
                if ui.button("Reload").clicked() {
                    app.hatch.loaded = None;
                }
            });
            ui.label(egui::RichText::new("Click a face to apply. Shift+click removes. Esc ends.").small().weak());
        });
    if !open {
        stop(app, "Hatch Pattern ended");
    }
}

pub fn open_preferences(app: &mut SolveApp) {
    crate::prefs::open_at(app, crate::prefs::PAGE_HATCH);
}

/// Preferences ▸ Hatch Patterns.
pub fn prefs_page(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::ui_kit::settings_page(ui, |ui| {
        crate::ui_kit::settings_heading(ui, app.tr("prefs.hatch.title"), app.tr("prefs.hatch.note"));
        ui.horizontal(|ui| {
            ui.label(app.tr("prefs.hatch.folder"));
            let r = ui.add(egui::TextEdit::singleline(&mut app.hatch.folder).desired_width(320.0).hint_text("C:\\Patterns"));
            crate::scenario::publish_target("prefs:hatch_folder", r.rect);
            if r.lost_focus() {
                app.hatch.loaded = None;
            }
            if ui.button(app.tr("prefs.hatch.choose")).clicked() {
                app.pick_file("Pick any .pat file in the folder", &["pat"], |app, path| {
                    if let Some(path) = path {
                        let p = std::path::Path::new(&path);
                        let dir = if p.is_dir() { Some(p) } else { p.parent() };
                        if let Some(dir) = dir {
                            app.hatch.folder = dir.to_string_lossy().into_owned();
                            app.hatch.loaded = None;
                        }
                    }
                });
            }
            if ui.button(app.tr("prefs.hatch.clear")).clicked() {
                app.hatch.folder.clear();
                app.hatch.loaded = None;
            }
        });
        let list: Vec<Pattern> = patterns(app).to_vec();
        let own = hatch::builtin().len();
        ui.label(crate::i18n::format("prefs.hatch.count", &[("own", &own.to_string()), ("folder", &list.len().saturating_sub(own).to_string())]));
        let ink = ui.visuals().text_color();
        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for p in &list {
                    ui.vertical(|ui| {
                        ui.set_width(72.0);
                        swatch(ui, p, 48.0, ink).on_hover_text(&p.description);
                        ui.label(egui::RichText::new(&p.name).small());
                    });
                }
            });
        });
    });
}

pub fn to_json(app: &SolveApp) -> Value {
    json!({"folder": app.hatch.folder, "pattern": app.hatch.pattern, "scale": app.hatch.scale, "angle": app.hatch.angle})
}

pub fn load(app: &mut SolveApp, v: &Value) {
    if let Some(f) = v.get("folder").and_then(Value::as_str) {
        app.hatch.folder = f.chars().take(4096).collect();
    }
    if let Some(p) = v.get("pattern").and_then(Value::as_str) {
        app.hatch.pattern = p.chars().take(64).collect();
    }
    if let Some(s) = v.get("scale").and_then(Value::as_f64).filter(|s| s.is_finite() && *s > 0.0) {
        app.hatch.scale = s.min(100_000.0);
    }
    if let Some(a) = v.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite()) {
        app.hatch.angle = a.clamp(-360.0, 360.0);
    }
    app.hatch.loaded = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    fn hatch_lines(app: &SolveApp) -> usize {
        crate::viewport::build_scene(app)
            .merged()
            .lines
            .as_chunks::<{ crate::gpu::LINE_SIZE }>()
            .0
            .iter()
            .filter(|line| line.get(28..32) == Some(0.7f32.to_le_bytes().as_slice()))
            .count()
    }

    #[test]
    fn the_surface_tab_has_only_hatch_pattern_and_a_hatch_draws_on_its_face() {
        let lists = crate::toolbar_custom::panel_lists("SURFACE");
        let ids: Vec<&str> = lists.iter().flat_map(|(_, cmds, _)| cmds.iter().map(|c| c.id)).collect();
        assert_eq!(ids, [COMMAND]);
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.frames(2);
        assert_eq!(hatch_lines(&h.app), 0);
        let body = h.app.session.model.state().bodies[0].name.clone();
        h.app.run("hatch.apply", json!({"body": body, "face": 0, "pattern": "SC_CROSS", "scale": 2, "angle": 10})).unwrap();
        let n = hatch_lines(&h.app);
        assert!(n > 10, "{n}");
        h.app.ui.visual_style = 2;
        assert_eq!(hatch_lines(&h.app), 0, "Wireframe shows no hatches");
        h.app.ui.visual_style = 4;
        assert!(hatch_lines(&h.app) > 10, "Hidden line shows them");
        // The tool starts and ends from its button; its settings round-trip.
        h.app.start(COMMAND);
        assert!(h.app.hatch.active);
        h.app.hatch.scale = 3.5;
        let saved = to_json(&h.app);
        h.app.start(COMMAND);
        assert!(!h.app.hatch.active);
        h.app.hatch.scale = 1.0;
        load(&mut h.app, &saved);
        assert_eq!(h.app.hatch.scale, 3.5);
    }

    #[test]
    fn a_folder_of_pat_files_adds_its_patterns() {
        let dir = std::env::temp_dir().join(format!("sc_hatch_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mine.pat"), "*MYBRICK, my bricks\n0, 0,0, 0,6\n90, 0,0, 6,6, 6,-6\n").unwrap();
        let mut h = Harness::new();
        let own = patterns(&mut h.app).len();
        h.app.hatch.folder = dir.to_string_lossy().into_owned();
        let all = patterns(&mut h.app).to_vec();
        assert_eq!(all.len(), own + 1);
        assert!(all.iter().any(|p| p.name == "MYBRICK"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
