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
    let st = crate::open_meshing::session_world(&app.session);
    let Some(b) = st.body(body) else { return false };
    let Some(f) = app.session.doc.feature(b.feature) else { return false };
    let feature = f.id;
    let reopen = matches!(f.kind, FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. });
    let sketch = match &f.kind {
        FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } => Some(sketch.0),
        FeatureKind::Sweep { path_sketch, .. } => Some(path_sketch.0),
        FeatureKind::Loft { sections, .. } => sections.first().map(|s| s.sketch.0),
        _ => None,
    };
    drop(st);
    let Some(sketch) = sketch else { return false };
    app.edit_feature(sketch);
    app.forms.after_edit = reopen.then_some((sketch, feature, false));
    app.set_status("Editing the object's sketch: change it, then Finish; the depth comes next", false);
    true
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
