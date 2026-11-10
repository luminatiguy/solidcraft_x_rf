//! History as a list of commands: the steps the session took, oldest first, then the steps that
//! were taken back. It is not a construction sequence: nothing rolls back, plays or reorders,
//! so re-editing a body never conflicts with the history. A click on a step undoes (or redoes)
//! every step after it, like the Undo drop-down.

use egui::{Rect, Sense, pos2, vec2};
use serde_json::json;

use crate::SolveApp;
use crate::theme::Tokens;

/// One step in the list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Step {
    pub label: String,
    /// Taken back (on the redo stack).
    pub undone: bool,
    /// Undo (`true`) or redo (`false`) this many times to make this the last step done.
    pub undo: bool,
    pub times: usize,
}

/// The most steps the list shows; older ones scroll off.
const MAX_STEPS: usize = 200;

/// The session's steps: the done ones oldest first, then the taken-back ones in the order a Redo
/// would take them.
pub(crate) fn steps(app: &SolveApp) -> Vec<Step> {
    let done = &app.session.undo;
    let undone = &app.session.redo;
    let mut out = Vec::new();
    let skip = done.len().saturating_sub(MAX_STEPS);
    for (i, s) in done.iter().enumerate().skip(skip) {
        // Undoing the steps after this one leaves it the last done.
        out.push(Step { label: label(&s.label), undone: false, undo: true, times: done.len() - 1 - i });
    }
    for (i, s) in undone.iter().rev().take(MAX_STEPS).enumerate() {
        out.push(Step { label: label(&s.label), undone: true, undo: false, times: i + 1 });
    }
    out
}

fn label(text: &str) -> String {
    if text.trim().is_empty() { "(step)".to_string() } else { text.to_string() }
}

fn go(app: &mut SolveApp, step: &Step) {
    let command = if step.undo { "edit.undo" } else { "edit.redo" };
    for _ in 0..step.times {
        if app.run(command, json!({})).is_err() {
            break;
        }
    }
}

const TIP: &str = "A command this session ran. Click to go back (or forward) to just after it";

/// The bottom History strip: one chip per step, scrolling sideways.
pub(crate) fn strip(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    let t = Tokens::get();
    let steps = steps(app);
    let mut pick = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.style_mut().always_scroll_the_only_direction = true;
        egui::ScrollArea::horizontal().auto_shrink([false, false]).stick_to_right(true).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                if steps.is_empty() {
                    ui.label(egui::RichText::new("No commands yet").color(t.text_dim));
                }
                for (i, step) in steps.iter().enumerate() {
                    let font = egui::FontId::proportional(t.font_label);
                    let colour = if step.undone { t.text_dim } else { t.text };
                    let galley = ui.painter().layout_no_wrap(step.label.clone(), font, colour);
                    let size = vec2(galley.size().x + 14.0, 22.0);
                    let (br, resp) = ui.allocate_exact_size(size, Sense::click());
                    let fill = if resp.hovered() { t.hover } else { t.timeline_item };
                    ui.painter().rect(br, 3.0, fill, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                    ui.painter().galley(pos2(br.left() + 7.0, br.center().y - galley.size().y * 0.5), galley, colour);
                    crate::scenario::publish_handle(&format!("history:step:{i}"), br.center());
                    if resp.on_hover_text(TIP).clicked() {
                        pick = Some(step.clone());
                    }
                }
            });
        });
    });
    if let Some(step) = pick {
        go(app, &step);
    }
}

/// The vertical History (left, or under the browser): one row per step.
pub(crate) fn column(app: &mut SolveApp, ui: &mut egui::Ui, names: bool) {
    let t = Tokens::get();
    let steps = steps(app);
    let mut pick = None;
    if steps.is_empty() && names {
        ui.label(egui::RichText::new("No commands yet").color(t.text_dim));
    }
    for (i, step) in steps.iter().enumerate() {
        let colour = if step.undone { t.text_dim } else { t.text };
        let text = if names { step.label.clone() } else { format!("{}", i + 1) };
        let (br, resp) = ui.allocate_exact_size(vec2(ui.available_width(), crate::ui_kit::TREE_ROW_HEIGHT), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(br, 3.0, t.hover);
        }
        ui.painter().text(
            pos2(br.left() + t.space_8, br.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(t.font_label),
            colour,
        );
        crate::scenario::publish_handle(&format!("history:step:{i}"), br.center());
        let tip = if names { TIP.to_string() } else { format!("{}\n{TIP}", step.label) };
        if resp.on_hover_text(tip).clicked() {
            pick = Some(step.clone());
        }
    }
    if let Some(step) = pick {
        go(app, &step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_lists_commands_and_steps_back_to_one() {
        let mut h = crate::scenario::Harness::new();
        let app = &mut h.app;
        let before = app.session.undo.len();
        for p in [[[0.0, 0.0], [10.0, 0.0]], [[0.0, 5.0], [10.0, 5.0]], [[0.0, 9.0], [10.0, 9.0]]] {
            if app.session.execute("line", &json!({ "points": p })).is_err() {
                return;
            }
        }
        let list = steps(app);
        assert_eq!(list.len(), before + 3);
        assert!(list.iter().all(|s| !s.undone));
        // Clicking the first new line takes back the other two.
        let first = list.get(before).cloned().unwrap_or_else(|| Step { label: String::new(), undone: false, undo: true, times: 0 });
        assert_eq!(first.times, 2);
        go(app, &first);
        let list = steps(app);
        assert_eq!(list.iter().filter(|s| s.undone).count(), 2);
        // The first taken-back step redoes once.
        let redo = list.iter().find(|s| s.undone).cloned();
        assert_eq!(redo.map(|s| s.times), Some(1));
    }
}
