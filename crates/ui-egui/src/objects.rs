//! The MODIFY tab: whole objects (bodies) moved, copied, rotated, mirrored, joined, unjoined and
//! cut. Objects stay separate until joined: the form tools make each form a body of its own.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{FeatureKind, Operation};

use crate::SolveApp;
use crate::dialogs::Kind;

/// `start`'s hook for the `obj.*` commands.
pub fn start_hook(app: &mut SolveApp, id: &str) -> bool {
    match id {
        "obj.move" => open_move(app, crate::dialogs_move::POINT_TO_POINT, false),
        "obj.copy" => open_move(app, crate::dialogs_move::POINT_TO_POINT, true),
        "obj.rotate" => open_move(app, crate::dialogs_move::ROTATE, false),
        "obj.mirror" => {
            app.start("solid.mirror");
            keep_tab(app);
        }
        "obj.join" => open_combine(app, 1, false),
        "obj.cut" => open_combine(app, 2, true),
        "obj.unjoin" => unjoin(app),
        _ => return false,
    }
    true
}

fn keep_tab(app: &mut SolveApp) {
    app.ui.tab = "MODIFY".into();
}

fn open_move(app: &mut SolveApp, mode: usize, copy: bool) {
    app.start("solid.move");
    let doc = app.session.doc.clone();
    if let Some(d) = app.dialog.as_mut().filter(|d| matches!(d.kind, Kind::Move { .. })) {
        let picked = d.inputs.first().map(|i| i.items.clone()).unwrap_or_default();
        d.kind = crate::dialogs_move::kind(mode, &doc);
        d.inputs = crate::dialogs_move::inputs(mode);
        if let Some(first) = d.inputs.first_mut() {
            first.items = picked;
        }
        d.active = d.inputs.iter().position(|i| i.items.is_empty()).unwrap_or(0);
        if copy {
            d.extra.insert("copy".into(), json!(true));
        }
    }
    keep_tab(app);
}

fn open_combine(app: &mut SolveApp, operation: usize, keep: bool) {
    app.start("solid.combine");
    if let Some(d) = app.dialog.as_mut()
        && let Kind::Combine { operation: op, keep_tools } = &mut d.kind
    {
        *op = operation;
        *keep_tools = keep;
    }
    keep_tab(app);
}

/// A double-click on an object: its sketch opens for editing (the first one for a Blend or a
/// Swept Blend; a Sweep's path). The form follows the sketch when it is finished. False when the
/// object was not made from a sketch.
pub fn edit_object(app: &mut SolveApp, body: &str) -> bool {
    edit_object_sketch(app, body, None)
}

/// [`edit_object`] with the sketch to open (one of the object's; `None`: its first).
pub fn edit_object_sketch(app: &mut SolveApp, body: &str, which: Option<u64>) -> bool {
    let st = crate::open_meshing::session_world(&app.session);
    let Some(b) = st.body(body) else { return false };
    let feature = b.feature;
    drop(st);
    // Going to another sketch of the object being edited keeps the edit going.
    if app.forms.chain.as_ref().is_some_and(|c| c.feature == feature) && app.session.active_sketch.is_some() {
        return crate::forms::edit_chain(app, body, feature, which);
    }
    // An open sketch is finished first.
    if app.session.active_sketch.is_some() {
        app.finish_sketch();
        if app.session.active_sketch.is_some() {
            return false;
        }
    }
    let Some(f) = app.session.doc.feature(feature) else { return false };
    // While the sketch changes, the object takes every closed region, so removing a loop (a
    // hole) doesn't break it; its regions are chosen again when the sketch is finished.
    if matches!(f.kind, FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. }) {
        let _ = app.run("timeline.set_profiles", serde_json::json!({"feature": feature, "profiles": "all"}));
    }
    app.forms.after_edit = None;
    crate::forms::edit_chain(app, body, feature, which)
}

/// The objects a joined object is made of: (the object's name before the join, the feature
/// that made it), in the order they were made; one entry for an object that was never joined.
pub fn join_members(app: &SolveApp, body: &str) -> Vec<(String, u64)> {
    let doc = &app.session.doc;
    // The joins into each name, followed back through joins of joins.
    let mut names: Vec<String> = vec![body.to_string()];
    let mut i = 0;
    while let Some(name) = names.get(i).cloned() {
        i += 1;
        if names.len() > 500 {
            break;
        }
        for f in &doc.features {
            if let FeatureKind::Combine { target, tools, operation: Operation::Join, .. } = &f.kind
                && target.0 == name
            {
                for t in tools {
                    if !names.contains(&t.0) {
                        names.push(t.0.clone());
                    }
                }
            }
        }
    }
    let maker = |name: &str| {
        doc.features
            .iter()
            .find(|f| {
                matches!(f.kind, FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. } | FeatureKind::Loft { .. } | FeatureKind::Sweep { .. })
                    && (f.body_names.iter().any(|n| n == name) || f.name == name)
            })
            .map(|f| f.id)
    };
    let mut out: Vec<(String, u64)> = names.into_iter().filter_map(|n| maker(&n).map(|id| (n, id))).collect();
    out.sort_by_key(|(_, id)| doc.features.iter().position(|f| f.id == *id).unwrap_or(usize::MAX));
    out
}

/// Edit one object of a joined object again (its row in the browser): its sketches open in
/// turn, and the joined object follows; nothing is unjoined.
pub fn edit_member(app: &mut SolveApp, name: &str, feature: u64) -> bool {
    if app.session.active_sketch.is_some() {
        app.finish_sketch();
        if app.session.active_sketch.is_some() {
            return false;
        }
    }
    if app.session.doc.feature(feature).is_some_and(|f| matches!(f.kind, FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. })) {
        let _ = app.run("timeline.set_profiles", json!({"feature": feature, "profiles": "all"}));
    }
    crate::forms::edit_chain(app, name, feature, None)
}

/// Unjoin: the last join that made the selected object goes, giving its objects back.
fn unjoin(app: &mut SolveApp) {
    let names: Vec<String> = app
        .session
        .selection
        .iter()
        .filter_map(|s| match s {
            Sel::Body { name } | Sel::Face { body: name, .. } | Sel::Edge { body: name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    if names.is_empty() {
        app.set_status("Unjoin: select a joined object first, then Unjoin", true);
        return;
    }
    let found = app.session.doc.features.iter().rev().find_map(|f| match &f.kind {
        FeatureKind::Combine { target, tools, operation: Operation::Join, .. }
            if names.iter().any(|n| *n == target.0 || tools.iter().any(|t| t.0 == *n)) =>
        {
            Some(f.id)
        }
        _ => None,
    });
    match found {
        Some(id) => match app.run("timeline.delete", json!({"feature": id})) {
            Ok(_) => app.set_status("Unjoined: the objects are separate again", false),
            Err(e) => app.set_status(e, true),
        },
        None => app.set_status("Unjoin: that object was not made by a join", true),
    }
}
