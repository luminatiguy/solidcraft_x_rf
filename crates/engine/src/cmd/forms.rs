//! Form tools of the MODEL tab (Extrusion, Blend, Revolve, Sweep, Swept Blend, their void
//! versions), the work plane and Model Line. They are interactive: the window starts a sketch on
//! the work plane and builds the feature from it when the sketch is finished. Run here (over the
//! control channel or a script) they only say so: the features themselves are `solid.extrude`,
//! `solid.loft`, `solid.revolve` and `solid.sweep`, and a sketch is `sketch.create`.

use serde_json::Value;

use super::CommandSpec;
use crate::params::bad;
use crate::{Result, Session};

fn interactive(cmd: &'static str, instead: &'static str) -> Result<Value> {
    Err(bad(cmd, format!("interactive only: it opens a sketch on the work plane in the window; run {instead} instead")))
}

fn extrusion(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.extrusion", "sketch.create and solid.extrude")
}
fn blend(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.blend", "sketch.create twice and solid.loft")
}
fn revolve(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.revolve", "sketch.create and solid.revolve")
}
fn sweep(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.sweep", "sketch.create twice and solid.sweep")
}
fn swept_blend(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.swept_blend", "sketch.create three times and solid.loft")
}
fn void_extrusion(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.void.extrusion", "solid.extrude with operation cut")
}
fn void_blend(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.void.blend", "solid.loft with operation cut")
}
fn void_revolve(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.void.revolve", "solid.revolve with operation cut")
}
fn void_sweep(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.void.sweep", "solid.sweep with operation cut")
}
fn void_swept_blend(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("form.void.swept_blend", "solid.loft with operation cut")
}
fn set_work_plane(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("workplane.set", "sketch.create with the plane")
}
fn show_work_plane(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("workplane.show", "view.grid")
}
fn model_line(_: &mut Session, _: &Value) -> Result<Value> {
    interactive("model.line", "sketch.create and sketch.line")
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("form.extrusion", "Extrusion", extrusion)
        .icon("extrude")
        .noundo()
        .doc_only()
        .params("interactive: sketch closed loops on the work plane, Finish extrudes them"),
    CommandSpec::new("form.blend", "Blend", blend)
        .icon("loft")
        .noundo()
        .doc_only()
        .params("interactive: sketch the base loop on the work plane, then the top loop on a parallel plane; Finish blends them"),
    CommandSpec::new("form.revolve", "Revolve", revolve)
        .icon("revolve")
        .noundo()
        .doc_only()
        .params("interactive: sketch closed loops and an axis line (centerline) on the work plane; Finish revolves them"),
    CommandSpec::new("form.sweep", "Sweep", sweep)
        .icon("sweep")
        .noundo()
        .doc_only()
        .params("interactive: sketch the path on the work plane, then the profile across its start; Finish sweeps it"),
    CommandSpec::new("form.swept_blend", "Swept Blend", swept_blend)
        .icon("pipe")
        .noundo()
        .doc_only()
        .params("interactive: sketch the path, then a profile across each end; Finish blends them along the path"),
    CommandSpec::new("form.void.extrusion", "Void Extrusion", void_extrusion)
        .icon("hole")
        .noundo()
        .doc_only()
        .params("interactive: as form.extrusion, cutting"),
    CommandSpec::new("form.void.blend", "Void Blend", void_blend).icon("loft").noundo().doc_only().params("interactive: as form.blend, cutting"),
    CommandSpec::new("form.void.revolve", "Void Revolve", void_revolve)
        .icon("revolve")
        .noundo()
        .doc_only()
        .params("interactive: as form.revolve, cutting"),
    CommandSpec::new("form.void.sweep", "Void Sweep", void_sweep).icon("sweep").noundo().doc_only().params("interactive: as form.sweep, cutting"),
    CommandSpec::new("form.void.swept_blend", "Void Swept Blend", void_swept_blend)
        .icon("pipe")
        .noundo()
        .doc_only()
        .params("interactive: as form.swept_blend, cutting"),
    CommandSpec::new("workplane.set", "Set Work Plane", set_work_plane)
        .icon("plane")
        .noundo()
        .doc_only()
        .params("interactive: pick an origin or construction plane or any planar face; the form tools and Model Line sketch on it"),
    CommandSpec::new("workplane.show", "Show Work Plane", show_work_plane)
        .icon("eye")
        .noundo()
        .doc_only()
        .params("interactive: show or hide the work plane's grid"),
    CommandSpec::new("model.line", "Model Line", model_line)
        .icon("line")
        .noundo()
        .doc_only()
        .params("interactive: draw lines on the work plane (a sketch no form uses)"),
];
