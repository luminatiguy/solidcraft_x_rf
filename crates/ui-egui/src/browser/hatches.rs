//! The browser's Hatch Patterns folder: each hatch on a face, named after its pattern and its
//! body. A click selects it (its face lights as a selected body does) and shows it in
//! Properties, where its pattern, scale and angle change; the row's cross, Delete or Properties'
//! Remove takes it away.

use serde_json::json;
use solidcraft_engine::Sel;

use super::{Actions, Row, draw_row, is_open, toggle};
use crate::SolveApp;

/// The design's hatches, in the order they were made.
pub(crate) fn list(app: &SolveApp) -> Vec<solidcraft_engine::hatch::Hatch> {
    solidcraft_engine::hatch::hatches(&app.session.doc.metadata)
}

/// A hatch's row name: its pattern and its body (and face).
pub(crate) fn label(h: &solidcraft_engine::hatch::Hatch) -> String {
    format!("{} · {} (face {})", h.pattern.name, h.body, h.face + 1)
}

/// Select hatch (body, face): its face is selected, so it lights in the view.
pub(crate) fn select(app: &mut SolveApp, h: &solidcraft_engine::hatch::Hatch) {
    app.hatch.selected = Some((h.body.clone(), h.face));
    let sel = Sel::Face { body: h.body.clone(), index: h.face, point: solidcore_record::Vec3::new(h.at[0], h.at[1], h.at[2]) };
    let _ = app.run("select.set", json!({ "items": [sel] }));
    app.shell_ui.properties = true;
}

/// The selected hatch, while its face is still what is selected.
pub(crate) fn selected(app: &SolveApp) -> Option<solidcraft_engine::hatch::Hatch> {
    let (body, face) = app.hatch.selected.clone()?;
    let still = matches!(app.session.selection.as_slice(), [Sel::Face { body: b, index, .. }] if *b == body && *index == face);
    if !still {
        return None;
    }
    list(app).into_iter().find(|h| h.body == body && h.face == face)
}

/// Remove the hatch on (body, face).
pub(crate) fn remove(app: &mut SolveApp, body: &str, face: usize) {
    match app.run("hatch.remove", json!({"body": body, "face": face})) {
        Ok(_) => {
            app.hatch.selected = None;
            let _ = app.run("select.clear", json!({}));
            app.set_status("Hatch pattern removed", false);
        }
        Err(e) => app.set_status(e, true),
    }
}

pub(super) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize, _acts: &mut Actions) {
    let all = list(app);
    if all.is_empty() {
        return;
    }
    let key = "hatches";
    let open = is_open(app, key, true);
    let count = all.len().to_string();
    let r = draw_row(
        ui,
        ui.id().with(key),
        &Row { depth, fold: Some(open), icon: "folder", label: "Hatch Patterns", count: &count, ..Default::default() },
    );
    if r.fold || r.clicked {
        toggle(app, key, true);
    }
    if !open {
        return;
    }
    let current = selected(app).map(|h| (h.body, h.face));
    let mut pick = None;
    let mut gone = None;
    for (i, h) in all.iter().enumerate() {
        let name = label(h);
        let is = current.as_ref().is_some_and(|(b, f)| *b == h.body && *f == h.face);
        let r = draw_row(
            ui,
            ui.id().with(("hatch", i)),
            &Row {
                depth: depth + 1,
                icon: "hatch_pattern",
                label: &name,
                name: true,
                selected: is,
                action: Some(("close", "Remove")),
                ..Default::default()
            },
        );
        if let Some(rect) = r.rect {
            crate::scenario::publish_target(&format!("browser:hatch:{i}"), rect);
        }
        if r.action {
            gone = Some((h.body.clone(), h.face));
        } else if r.clicked {
            pick = Some(h.clone());
        }
    }
    if let Some((body, face)) = gone {
        remove(app, &body, face);
    } else if let Some(h) = pick {
        select(app, &h);
    }
}

/// Properties of the selected hatch: its pattern, scale and angle, and Remove.
pub(crate) fn properties_ui(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    let Some(h) = selected(app) else { return false };
    ui.label(egui::RichText::new(label(&h)).strong());
    ui.label(egui::RichText::new("Hatch pattern on a face").small().weak());
    ui.add_space(4.0);
    let names: Vec<String> = crate::hatch_tool::patterns(app).iter().map(|p| p.name.clone()).collect();
    let mut pattern = h.pattern.name.clone();
    let (mut scale, mut angle) = (h.scale, h.angle);
    let mut changed = false;
    egui::Grid::new("sc_hatch_props").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Pattern");
        egui::ComboBox::from_id_salt("sc_hatch_props_pattern").width(160.0).selected_text(pattern.clone()).show_ui(ui, |ui| {
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for n in &names {
                    if ui.selectable_label(*n == pattern, n.as_str()).clicked() {
                        pattern = n.clone();
                        changed = true;
                    }
                }
            });
        });
        ui.end_row();
        ui.label("Scale");
        changed |= ui.add(egui::DragValue::new(&mut scale).speed(0.05).range(0.001..=100_000.0).max_decimals(4)).changed();
        ui.end_row();
        ui.label("Angle");
        changed |= ui.add(egui::DragValue::new(&mut angle).speed(1.0).range(-360.0..=360.0).suffix("°").max_decimals(2)).changed();
        ui.end_row();
    });
    if changed {
        // The pattern by name when it is in the library, else the one the design keeps.
        let pat = if names.contains(&pattern) && pattern != h.pattern.name {
            json!(pattern)
        } else {
            serde_json::to_value(&h.pattern).unwrap_or(json!(pattern))
        };
        let mut params = json!({"body": h.body, "face": h.face, "at": h.at, "pattern": pat, "scale": scale, "angle": angle});
        if !app.hatch.folder.trim().is_empty()
            && let Some(m) = params.as_object_mut()
        {
            m.insert("folder".into(), json!(app.hatch.folder));
        }
        if let Err(e) = app.run("hatch.apply", params) {
            app.set_status(e, true);
        }
    }
    ui.add_space(6.0);
    if ui.button("Remove hatch").clicked() {
        remove(app, &h.body, h.face);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hatch picked in the list is named after its body, lights its face, and Delete takes the
    /// hatch, not the body.
    #[test]
    fn a_hatch_is_listed_selected_and_deleted() {
        let mut h = crate::scenario::Harness::new();
        h.app.session.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let body = h.app.session.model.state().bodies[0].name.clone();
        h.app.session.execute("hatch.apply", &json!({"body": body, "face": 0, "pattern": "SC_GRID"})).unwrap();
        let all = list(&h.app);
        assert_eq!(all.len(), 1);
        assert!(label(&all[0]).contains(&body), "{}", label(&all[0]));
        select(&mut h.app, &all[0]);
        assert!(selected(&h.app).is_some());
        assert!(matches!(h.app.session.selection.as_slice(), [Sel::Face { .. }]));
        crate::delete::delete_selection(&mut h.app);
        h.frames(3);
        assert!(list(&h.app).is_empty(), "the hatch went");
        assert_eq!(h.app.session.model.state().bodies.len(), 1, "the body stays");
    }
}
