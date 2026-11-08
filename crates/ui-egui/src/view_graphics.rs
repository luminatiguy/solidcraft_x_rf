//! The VIEW tab's GRAPHICS panel: the navigation bar's Graphics menu as ribbon buttons (graphics
//! preset, grid, ground shadow, ambient occlusion, perspective, orthographic faces, tool at
//! cursor, view angles, assistant cursor). Each button toggles its setting and shows pressed
//! while it is on.

use serde_json::json;

use crate::SolveApp;

fn quality(app: &SolveApp) -> bool {
    crate::prefs::graphics_preset(&app.preferences) == "quality"
}

pub fn start_hook(app: &mut SolveApp, id: &str) -> bool {
    match id {
        "view.grid" | "view.ground_shadow" => {
            let _ = app.run(id, json!({}));
        }
        "view.ambient_occlusion" => {
            let _ = app.run(id, json!({"enabled": !app.preferences.ambient_occlusion}));
        }
        "view.graphics_preset" => {
            let preset = if quality(app) { "performance" } else { "quality" };
            let _ = app.run(id, json!({"preset": preset}));
        }
        "view.perspective" => {
            if app.session.active_sketch.is_some() {
                app.set_status(app.tr("nav.perspective.sketch").to_owned(), false);
            } else {
                app.ui.perspective = !app.ui.perspective;
            }
        }
        "view.auto_ortho_faces" => app.ui.auto_ortho_faces = !app.ui.auto_ortho_faces,
        "view.tool_cursor" => app.ui.tool_cursor = !app.ui.tool_cursor,
        "view.agent_cursor" => app.ui.agent_cursor.show = !app.ui.agent_cursor.show,
        "view.angle_readout" => {
            let _ = app.run(id, json!({"enabled": !app.preferences.show_view_angles}));
        }
        _ => return false,
    }
    true
}

pub fn pressed(app: &SolveApp, id: &str) -> bool {
    match id {
        "view.grid" => app.ui.show_grid,
        "view.ground_shadow" => app.ui.ground_shadow,
        "view.ambient_occlusion" => app.preferences.ambient_occlusion,
        "view.graphics_preset" => quality(app),
        "view.perspective" => app.ui.perspective,
        "view.auto_ortho_faces" => app.ui.auto_ortho_faces,
        "view.tool_cursor" => app.ui.tool_cursor,
        "view.agent_cursor" => app.ui.agent_cursor.show,
        "view.angle_readout" => app.preferences.show_view_angles,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_tab_shows_the_graphics_menu_as_buttons_that_toggle() {
        let lists = crate::toolbar_custom::panel_lists("VIEW");
        let (_, cmds, promoted) = lists.iter().find(|(p, ..)| *p == "GRAPHICS").expect("a GRAPHICS panel");
        let ids: Vec<&str> = cmds.iter().map(|c| c.id).collect();
        for id in ["view.grid", "view.ground_shadow", "view.ambient_occlusion", "view.perspective", "view.graphics_preset", "view.angle_readout"] {
            assert!(ids.contains(&id), "{id} in {ids:?}");
        }
        let mut h = crate::scenario::Harness::new();
        // Every button is on the ribbon by default: large first, the rest as small cells.
        assert_eq!(crate::toolbar_custom::buttons(&h.app, "VIEW", "GRAPHICS", cmds, *promoted).len(), cmds.len());
        for id in ids {
            let before = pressed(&h.app, id);
            h.app.start(id);
            h.frames(1);
            assert_ne!(pressed(&h.app, id), before, "{id} toggles");
            h.app.start(id);
            h.frames(1);
            assert_eq!(pressed(&h.app, id), before, "{id} toggles back");
        }
    }
}
