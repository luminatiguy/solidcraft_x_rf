//! The sketch's drawing and editing tools, after CADCraft's (Line, Polyline, Circle… Move, Trim,
//! Fillet…). They are interactive: the window runs their prompts (clicks and typed values) and
//! makes the geometry with the sketch's own commands (`sketch.line`, `sketch.trim`…). Run here they
//! only say so.

use serde_json::Value;

use super::CommandSpec;
use crate::params::bad;
use crate::{Result, Session};

fn in_sketch(s: &Session) -> std::result::Result<(), String> {
    if s.active_sketch.is_some() { Ok(()) } else { Err("open a sketch first (a form tool on the MODEL tab)".into()) }
}

fn interactive(_: &mut Session, _: &Value) -> Result<Value> {
    Err(bad("cad", "interactive only: the sketch tools run in the window; use the sketch.* commands instead"))
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cad.line", "Line", interactive)
        .icon("cad:line")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Line: points one after another (Close, Undo)"),
    CommandSpec::new("cad.pline", "Polyline", interactive)
        .icon("cad:pline")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Polyline: lines and arcs end to end (Arc, Line, Close, Undo)"),
    CommandSpec::new("cad.circle", "Circle", interactive)
        .icon("cad:circle")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Circle: centre and radius (Diameter, 3P, 2P, Ttr)"),
    CommandSpec::new("cad.rectang", "Rectangle", interactive)
        .icon("cad:rectang")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Rectangle: two opposite corners (Dimensions, Chamfer, Fillet)"),
    CommandSpec::new("cad.arc", "Arc", interactive)
        .icon("cad:arc")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Arc: three points (Center, End)"),
    CommandSpec::new("cad.spline", "Spline", interactive)
        .icon("cad:spline")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Spline: fit points (Close, Undo, CV)"),
    CommandSpec::new("cad.xline", "Construction Line", interactive)
        .icon("cad:xline")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Construction line through two points (Hor, Ver, Ang)"),
    CommandSpec::new("cad.ellipse", "Ellipse", interactive)
        .icon("cad:ellipse")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Ellipse: axis end points and the other half-axis (Center)"),
    CommandSpec::new("cad.polygon", "Polygon", interactive)
        .icon("cad:polygon")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Polygon: sides, centre, Inscribed/Circumscribed radius (Edge)"),
    CommandSpec::new("cad.point", "Multiple Point", interactive)
        .icon("cad:point")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Points, one per click"),
    CommandSpec::new("cad.donut", "Donut", interactive)
        .icon("cad:donut")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Donut: inside and outside diameters, then centres"),
    CommandSpec::new("cad.revcloud", "Revision Cloud", interactive)
        .icon("cad:revcloud")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Revision cloud: corners of the outline, arcs along it"),
    CommandSpec::new("cad.move", "Move", interactive)
        .icon("cad:move")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Move: objects, base point, second point"),
    CommandSpec::new("cad.copy", "Copy", interactive)
        .icon("cad:copy")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Copy: objects, base point, second points (Array, Undo)"),
    CommandSpec::new("cad.rotate", "Rotate", interactive)
        .icon("cad:rotate")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Rotate: objects, base point, angle (Copy, Reference)"),
    CommandSpec::new("cad.trim", "Trim", interactive)
        .icon("cad:trim")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Trim: click the pieces to cut away"),
    CommandSpec::new("cad.fillet", "Fillet", interactive)
        .icon("cad:fillet")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Fillet: two objects (Radius)"),
    CommandSpec::new("cad.arrayrect", "Rectangular Array", interactive)
        .icon("cad:arrayrect")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Rectangular array: objects, rows, columns and spacing"),
    CommandSpec::new("cad.mirror", "Mirror", interactive)
        .icon("cad:mirror")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Mirror: objects, two points of the mirror line, keep or erase the source"),
    CommandSpec::new("cad.scale", "Scale", interactive)
        .icon("cad:scale")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Scale: objects, base point, factor (Copy, Reference)"),
    CommandSpec::new("cad.extend", "Extend", interactive)
        .icon("cad:extend")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Extend: click near the end to extend"),
    CommandSpec::new("cad.offset", "Offset", interactive)
        .icon("cad:offset")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Offset: distance (Through), object, side"),
    CommandSpec::new("cad.arraypolar", "Polar Array", interactive)
        .icon("cad:arraypolar")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Polar array: objects, centre, items and angle to fill"),
    CommandSpec::new("cad.stretch", "Stretch", interactive)
        .icon("cad:stretch")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Stretch: crossing window, base point, second point"),
    CommandSpec::new("cad.chamfer", "Chamfer", interactive)
        .icon("cad:chamfer")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Chamfer: two lines (Distance)"),
    CommandSpec::new("cad.explode", "Explode", interactive)
        .icon("cad:explode")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Explode: text and linked geometry into plain lines and arcs"),
    CommandSpec::new("cad.break", "Break", interactive)
        .icon("cad:break")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Break: object, two break points (First point)"),
    CommandSpec::new("cad.join", "Join", interactive)
        .icon("cad:join")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Join: lines on one line, arcs on one circle, into one"),
    CommandSpec::new("cad.erase", "Erase", interactive)
        .icon("cad:erase")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Erase: objects"),
    CommandSpec::new("cad.lengthen", "Lengthen", interactive)
        .icon("cad:lengthen")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Lengthen: DElta, Percent, Total or DYnamic, then the ends to change"),
    CommandSpec::new("cad.matchprop", "Match Properties", interactive)
        .icon("cad:matchprop")
        .noundo()
        .doc_only()
        .enabled(in_sketch)
        .params("interactive: Match properties: source object, then objects to take its line type"),
];
