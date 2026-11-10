//! The window's shell, laid out as in CADCraft: the application bar (the logo's menu, a quick
//! access bar the user customises, the window buttons), the menu bar above the workspace ribbon
//! (File, Edit, Inspect, Window, Help), the document tabs under the ribbon and the Properties
//! window.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::icons;
use crate::theme::Tokens;
use crate::toolbar::{FileAction, design_shown};
use crate::ui_kit::BackingTooltipExt as _;
use crate::ui_kit::{Container, Edge, Surface};

/// Height of the menu bar and of the document tab bar.
pub const MENU_BAR_HEIGHT: f32 = 26.0;
pub const TABS_BAR_HEIGHT: f32 = 30.0;

/// What the quick access bar can hold: (id, label, icon).
pub const QUICK: &[(&str, &str, &str)] = &[
    ("new", "New design", "new"),
    ("open", "Open…", "open"),
    ("save", "Save", "save"),
    ("save_as", "Save As…", "save"),
    ("undo", "Undo", "undo"),
    ("redo", "Redo", "redo"),
    ("export_step", "Export STEP…", "export"),
    ("export_stl", "Export STL…", "mesh_export"),
    ("measure", "Measure", "measure"),
    ("properties", "Properties", "list"),
    ("home", "Start page", "home"),
    ("preferences", "Preferences", "settings"),
];

pub fn default_quick() -> Vec<String> {
    ["save", "undo", "redo"].iter().map(|s| s.to_string()).collect()
}

/// The menu bar's menus.
const MENUS: [&str; 5] = ["File", "Edit", "Inspect", "Window", "Help"];

#[derive(Default)]
pub struct State {
    /// The quick access bar's items, in order (ids from [`QUICK`]).
    pub quick: Vec<String>,
    /// The Properties window is shown.
    pub properties: bool,
    /// The logo's menu is open.
    logo_open: bool,
    /// The Customize panel window is open.
    customize: bool,
    /// The open menu bar menu and where it drops from.
    menu: Option<(&'static str, Pos2)>,
    /// The open Undo (true) or Redo history list and where it drops from.
    history: Option<(bool, Pos2)>,
    /// The pass a popup was opened on (its own click doesn't close it).
    opened: u64,
    /// Properties: the field being typed into (feature, input index) and its text.
    pub(crate) editing: Option<(u64, usize, String)>,
}

fn quick_label(id: &str) -> &'static str {
    QUICK.iter().find(|q| q.0 == id).map_or("", |q| q.1)
}

fn quick_icon(id: &str) -> &'static str {
    QUICK.iter().find(|q| q.0 == id).map_or("", |q| q.2)
}

fn quick_enabled(app: &SolveApp, id: &str) -> bool {
    let idle = !app.file_load.busy();
    let shown = design_shown(app) && idle;
    match id {
        "undo" => shown && !app.session.undo.is_empty(),
        "redo" => shown && !app.session.redo.is_empty(),
        "new" | "open" | "home" | "preferences" | "properties" => idle,
        _ => shown,
    }
}

fn run_quick(app: &mut SolveApp, id: &str) {
    match id {
        "new" => FileAction::New.run(app),
        "open" => FileAction::Open.run(app),
        "save" => FileAction::Save.run(app),
        "save_as" => FileAction::SaveAs.run(app),
        "undo" => FileAction::Undo.run(app),
        "redo" => FileAction::Redo.run(app),
        "export_step" => FileAction::Command("file.export_step").run(app),
        "export_stl" => FileAction::Command("file.export_stl").run(app),
        "measure" => app.start("inspect.measure"),
        "properties" => app.shell_ui.properties = !app.shell_ui.properties,
        "home" => app.home.show(app.session.revision),
        "preferences" => crate::prefs::open_at(app, 0),
        _ => {}
    }
}

fn pass(ctx: &egui::Context) -> u64 {
    ctx.cumulative_pass_nr()
}

// ---- The application bar --------------------------------------------------------------------

/// The application bar: the logo (its menu), the quick access bar, the window buttons.
pub fn app_bar(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    if app.shell_ui.quick.is_empty() && !app.shell_ui.customize {
        app.shell_ui.quick = default_quick();
    }
    let edge = Container::new(ui.ctx(), Surface::ApplicationBar).edge("sc_appbar", Edge::Top);
    let frame = Container::new(ui.ctx(), Surface::ApplicationBar).edge_frame(t.app_bar);
    Container::new(ui.ctx(), Surface::ApplicationBar).show_edge(
        ui,
        edge.exact_size(crate::toolbar::APP_BAR_HEIGHT).show_separator_line(false),
        frame,
        |ui| {
            let r = ui.max_rect();
            if app.custom_titlebar || app.integrated_titlebar {
                crate::titlebar::drag_area(ui, r);
            }
            // Right-click on the bar: Customize panel.
            let bg = ui.interact(r, ui.id().with("sc_appbar_bg"), Sense::click());
            if bg.secondary_clicked() {
                app.shell_ui.menu = Some(("Customize", bg.interact_pointer_pos().unwrap_or(r.left_bottom())));
                app.shell_ui.opened = pass(ui.ctx());
            }
            let left = r.left() + if app.integrated_titlebar { crate::titlebar::TRAFFIC_LIGHTS } else { 0.0 };
            // The logo: a button opening the application menu.
            let logo = Rect::from_min_size(pos2(left + 4.0, r.top() + 3.0), vec2(132.0, r.height() - 6.0));
            let lresp = ui.interact(logo, ui.id().with("sc_logo"), Sense::click());
            if lresp.hovered() || app.shell_ui.logo_open {
                ui.painter().rect_filled(logo, t.radius_sm, t.app_bar_hover_button);
            }
            crate::icons::glyphs::brand_mark(ui.painter(), pos2(logo.left() + 16.0, r.center().y));
            ui.painter().text(
                pos2(logo.left() + 32.0, r.center().y),
                Align2::LEFT_CENTER,
                "SolidCraft",
                crate::fonts::semibold(t.font_heading),
                t.app_bar_text,
            );
            arrow(ui.painter(), pos2(logo.right() - 10.0, r.center().y), t.app_bar_text);
            crate::scenario::publish_target("appbar:logo", logo);
            if lresp.on_hover_text("SolidCraft menu").clicked() {
                app.shell_ui.logo_open = !app.shell_ui.logo_open;
                app.shell_ui.opened = pass(ui.ctx());
            }
            let mut x = logo.right() + 10.0;
            ui.painter().vline(x, r.y_range().shrink(8.0), Stroke::new(1.0, t.app_bar_separator));
            x += 8.0;
            // The quick access bar.
            for id in app.shell_ui.quick.clone() {
                let history = matches!(id.as_str(), "undo" | "redo");
                let w = if history { 44.0 } else { 32.0 };
                let br = Rect::from_min_size(pos2(x, r.center().y - 16.0), vec2(w, 32.0));
                let enabled = quick_enabled(app, &id);
                let resp = ui.interact(br, ui.id().with(("sc_quick", &id)), if enabled { Sense::click() } else { Sense::hover() });
                crate::scenario::publish_target(&format!("appbar:{id}"), br);
                if enabled && resp.hovered() {
                    ui.painter().rect_filled(br.shrink(3.0), t.radius_sm, t.app_bar_hover_button);
                }
                let icon_rect = Rect::from_center_size(pos2(br.left() + 16.0, br.center().y), vec2(18.0, 18.0));
                icons::paint_app_bar(ui.painter(), icon_rect, quick_icon(&id), if enabled { 1.0 } else { 0.35 });
                let tip = quick_label(&id).to_string();
                if history {
                    // The arrow drops the list of what Undo (Redo) would take back.
                    let drop = Rect::from_min_max(pos2(br.left() + 30.0, br.top()), br.max);
                    arrow(ui.painter(), drop.center(), if enabled { t.app_bar_text } else { t.app_bar_text.gamma_multiply(0.35) });
                    let pointer = ui.ctx().pointer_interact_pos();
                    let clicked = resp.clone().backing_on_hover_text(&tip).clicked();
                    if clicked && enabled {
                        if pointer.is_some_and(|p| drop.contains(p)) {
                            app.shell_ui.history = Some((id == "undo", br.left_bottom()));
                            app.shell_ui.opened = pass(ui.ctx());
                        } else {
                            run_quick(app, &id);
                        }
                    }
                } else if resp.backing_on_hover_text(&tip).clicked() && enabled {
                    run_quick(app, &id);
                }
                x += w + 2.0;
            }
            if app.custom_titlebar {
                crate::titlebar::caption_buttons(app, ui, r);
            }
        },
    );
    let ctx = ui.ctx().clone();
    logo_menu(app, &ctx);
    history_list(app, &ctx);
    customize(app, &ctx);
}

fn arrow(p: &egui::Painter, c: Pos2, ink: Color32) {
    p.add(egui::Shape::convex_polygon(vec![c + vec2(-3.5, -1.5), c + vec2(3.5, -1.5), c + vec2(0.0, 2.5)], ink, Stroke::NONE));
}

/// A popup frame at `at`; returns whether a press outside it (or Escape) closes it.
fn popup(ctx: &egui::Context, id: &str, at: Pos2, opened: u64, add: impl FnOnce(&mut egui::Ui)) -> bool {
    let resp = Container::new(ctx, Surface::Popup)
        .area(egui::Id::new(id))
        .fixed_pos(at)
        .constrain(true)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| crate::context_menu::menu_frame(ui, add));
    let outside = ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer() && pass(ctx) != opened;
    outside || ctx.input(|i| i.key_pressed(egui::Key::Escape))
}

/// A row of the logo's menu: a large icon, a title and a line saying what it does.
fn big_row(ui: &mut egui::Ui, icon: &str, title: &str, hint: &str, enabled: bool) -> bool {
    let t = Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(300.0, 44.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    let fade = if enabled { 1.0 } else { 0.4 };
    icons::paint_icon(ui.painter(), Rect::from_center_size(pos2(r.left() + 24.0, r.center().y), vec2(28.0, 28.0)), icon, fade);
    ui.painter().text(
        pos2(r.left() + 48.0, r.top() + 13.0),
        Align2::LEFT_CENTER,
        title,
        crate::fonts::semibold(t.font_button),
        t.text.gamma_multiply(fade),
    );
    ui.painter().text(
        pos2(r.left() + 48.0, r.top() + 31.0),
        Align2::LEFT_CENTER,
        hint,
        FontId::proportional(t.font_small),
        t.text_dim.gamma_multiply(fade),
    );
    enabled && resp.clicked()
}

fn logo_menu(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.shell_ui.logo_open {
        return;
    }
    let shown = design_shown(app) && !app.file_load.busy();
    let mut pick: Option<&str> = None;
    let close = popup(ctx, "sc_logo_menu", pos2(4.0, crate::toolbar::APP_BAR_HEIGHT), app.shell_ui.opened, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let rows: [(&str, &str, &str, &str, bool); 13] = [
            ("new", "new", "New design", "Start an empty design (Ctrl+N)", true),
            ("open", "open", "Open…", "A design or an exchange file (Ctrl+O)", true),
            ("save", "save", "Save", "Save the design (Ctrl+S)", shown),
            ("save_as", "save", "Save As…", "Save under another name", shown),
            ("home", "home", "Start page", "Recent designs and samples", true),
            ("drawing", "ws_drawing", "Drawing", "Lay out views and export DWG", shown),
            ("properties", "list", "Properties", "The selected object's dimensions", true),
            ("preferences", "settings", "Preferences", "Units, theme, navigation (Ctrl+,)", true),
            ("shortcuts", "list", "Keyboard shortcuts", "Every key, and set your own", true),
            ("help", "help", "Help", "Documentation, updates, about", true),
            ("report", "bug_report", "Report a bug", "A screenshot and a draft issue", true),
            ("version", "update", "Release notes", "What's new in this version", true),
            ("quit", "close", "Exit", "Close SolidCraft", true),
        ];
        for (id, icon, title, hint, on) in rows {
            if big_row(ui, icon, title, hint, on) {
                pick = Some(id);
            }
            if matches!(id, "save_as" | "properties" | "report") {
                ui.add_space(3.0);
                ui.separator();
                ui.add_space(3.0);
            }
        }
    });
    if let Some(id) = pick {
        app.shell_ui.logo_open = false;
        match id {
            "drawing" => {
                let _ = crate::render_workspace::enter(app, false);
                crate::drawing_ws::set_open(app, true);
            }
            "shortcuts" => FileAction::Shortcuts.run(app),
            "help" => {
                app.help.menu = true;
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("sc_help_opened"), true);
                    d.insert_temp(egui::Id::new("sc_help_anchor"), 320.0_f32);
                });
            }
            "report" => crate::help::run(app, ctx, "report"),
            "version" => crate::help::run(app, ctx, "version"),
            "quit" => FileAction::Quit.run(app),
            other => run_quick(app, other),
        }
    } else if close {
        app.shell_ui.logo_open = false;
    }
}

/// The Undo (Redo) drop-down: the steps it would take back, newest first; a pick undoes every
/// step down to it.
fn history_list(app: &mut SolveApp, ctx: &egui::Context) {
    let Some((undo, at)) = app.shell_ui.history else { return };
    let labels: Vec<String> = if undo { &app.session.undo } else { &app.session.redo }.iter().rev().take(100).map(|s| s.label.clone()).collect();
    let mut pick = None;
    let hover_id = egui::Id::new("sc_history_hover");
    let hovered: Option<usize> = ctx.data(|d| d.get_temp(hover_id));
    let mut now_hovered = None;
    let close = popup(ctx, "sc_history_list", at, app.shell_ui.opened, |ui| {
        ui.set_min_width(240.0);
        if labels.is_empty() {
            ui.label("Nothing to take back");
        }
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (i, label) in labels.iter().enumerate() {
                let lit = hovered.is_some_and(|h| i <= h);
                let item = crate::context_menu::Item::action("history", if label.is_empty() { "(step)" } else { label }, "");
                let resp = crate::context_menu::list_row_lit(ui, &item, lit);
                if ui.rect_contains_pointer(ui.min_rect()) && ui.ctx().pointer_hover_pos().is_some_and(|p| p.y <= ui.min_rect().bottom()) {
                    now_hovered = Some(i);
                }
                if resp.is_some() {
                    pick = Some(i);
                }
            }
        });
        let n = hovered.map_or(0, |h| h + 1);
        ui.separator();
        ui.label(egui::RichText::new(format!("{} {n} step{}", if undo { "Undo" } else { "Redo" }, if n == 1 { "" } else { "s" })).small());
    });
    ctx.data_mut(|d| d.insert_temp(hover_id, now_hovered));
    if let Some(i) = pick {
        app.shell_ui.history = None;
        let command = if undo { "edit.undo" } else { "edit.redo" };
        for _ in 0..=i {
            if app.run(command, json!({})).is_err() {
                break;
            }
        }
    } else if close {
        app.shell_ui.history = None;
    }
}

/// Customize panel: which commands the quick access bar holds, and their order.
fn customize(app: &mut SolveApp, ctx: &egui::Context) {
    if let Some(("Customize", at)) = app.shell_ui.menu {
        let mut open_panel = false;
        let close = popup(ctx, "sc_appbar_menu", at, app.shell_ui.opened, |ui| {
            if crate::context_menu::list_row(ui, &crate::context_menu::Item::action("customize", "Customize panel…", "settings")).is_some() {
                open_panel = true;
            }
        });
        if open_panel {
            app.shell_ui.customize = true;
            app.shell_ui.menu = None;
        } else if close {
            app.shell_ui.menu = None;
        }
    }
    if !app.shell_ui.customize {
        return;
    }
    let mut open = true;
    egui::Window::new("Customize panel").open(&mut open).collapsible(false).resizable(false).default_width(300.0).show(ctx, |ui| {
        ui.label("Commands on the application bar, left to right:");
        ui.add_space(4.0);
        let quick = &mut app.shell_ui.quick;
        let mut swap = None;
        let n = quick.len();
        for (i, id) in quick.iter().enumerate() {
            ui.horizontal(|ui| {
                icons::paint_icon(ui.painter(), Rect::from_min_size(ui.cursor().min, vec2(18.0, 18.0)), quick_icon(id), 1.0);
                ui.add_space(24.0);
                ui.label(quick_label(id));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Remove").clicked() {
                        swap = Some((i, usize::MAX));
                    }
                    if ui.add_enabled(i + 1 < n, egui::Button::new("▼").small()).clicked() {
                        swap = Some((i, i + 1));
                    }
                    if ui.add_enabled(i > 0, egui::Button::new("▲").small()).clicked() {
                        swap = Some((i, i.saturating_sub(1)));
                    }
                });
            });
        }
        match swap {
            Some((i, usize::MAX)) if i < quick.len() => {
                quick.remove(i);
            }
            Some((i, j)) if i < quick.len() && j < quick.len() => quick.swap(i, j),
            _ => {}
        }
        ui.separator();
        ui.label("Add:");
        ui.horizontal_wrapped(|ui| {
            for (id, label, _) in QUICK {
                if !quick.iter().any(|q| q == id) && ui.button(*label).clicked() {
                    quick.push((*id).to_string());
                }
            }
        });
        ui.separator();
        if ui.button("Reset").clicked() {
            *quick = default_quick();
        }
    });
    if !open {
        app.shell_ui.customize = false;
    }
}

// ---- The menu bar -----------------------------------------------------------------------------

/// The menu bar above the workspace ribbon: File, Edit, Inspect, Window, Help.
pub fn menu_bar(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let edge = Container::new(ui.ctx(), Surface::ApplicationBar).edge("sc_menu_bar", Edge::Top);
    let frame = Container::new(ui.ctx(), Surface::ApplicationBar).edge_frame(t.app_bar);
    Container::new(ui.ctx(), Surface::ApplicationBar).show_edge(ui, edge.exact_size(MENU_BAR_HEIGHT).show_separator_line(false), frame, |ui| {
        let r = ui.max_rect();
        let mut x = r.left() + 6.0;
        let idle = !app.file_load.busy();
        for name in MENUS {
            let galley = ui.painter().layout_no_wrap(name.to_string(), FontId::proportional(t.font_button), t.app_bar_text);
            let br = Rect::from_min_size(pos2(x, r.top() + 2.0), vec2(galley.size().x + 16.0, r.height() - 4.0));
            let resp = ui.interact(br, ui.id().with(("sc_menu", name)), if idle { Sense::click() } else { Sense::hover() });
            let open = app.shell_ui.menu.is_some_and(|(m, _)| m == name) || (name == "Help" && app.help.menu);
            if resp.hovered() || open {
                ui.painter().rect_filled(br, t.radius_sm, t.app_bar_hover_button);
            }
            ui.painter().galley(pos2(br.left() + 8.0, br.center().y - galley.size().y / 2.0), galley, t.app_bar_text);
            crate::scenario::publish_target(&format!("menu:{}", name.to_ascii_lowercase()), br);
            // Moving along the bar with a menu open opens the one under the pointer.
            let any_open = app.shell_ui.menu.is_some_and(|(m, _)| MENUS.contains(&m)) || app.help.menu;
            if (resp.clicked() || (any_open && resp.hovered() && !open)) && idle {
                let toggle_off = resp.clicked() && open;
                app.shell_ui.menu = None;
                app.help.menu = false;
                if !toggle_off {
                    if name == "Help" {
                        app.help.menu = true;
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(egui::Id::new("sc_help_opened"), true);
                            d.insert_temp(egui::Id::new("sc_help_anchor"), br.left() + 14.0);
                            d.insert_temp(egui::Id::new("sc_help_y"), br.bottom());
                        });
                    } else if name == "File" {
                        // The File menu is the one the app has always had. (The pass number is
                        // read before: the context is locked inside `data_mut`.)
                        let now = pass(ui.ctx());
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(egui::Id::new("sc_file_menu"), true);
                            d.insert_temp(egui::Id::new("sc_file_menu_opened"), now);
                            d.insert_temp(egui::Id::new("sc_file_menu_x"), br.left());
                            d.insert_temp(egui::Id::new("sc_file_menu_y"), br.bottom());
                        });
                        app.shell_ui.menu = Some(("File", br.left_bottom()));
                    } else {
                        app.shell_ui.menu = Some((name, br.left_bottom()));
                    }
                    app.shell_ui.opened = pass(ui.ctx());
                }
            }
            x = br.right() + 2.0;
        }
        ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, t.app_bar_separator));
    });
    let ctx = ui.ctx().clone();
    // The File menu closes itself; follow it.
    if app.shell_ui.menu.is_some_and(|(m, _)| m == "File") && !ctx.data(|d| d.get_temp::<bool>(egui::Id::new("sc_file_menu")).unwrap_or(false)) {
        app.shell_ui.menu = None;
    }
    crate::toolbar::file_menu_public(app, &ctx);
    let help_x = ctx.data(|d| d.get_temp::<f32>(egui::Id::new("sc_help_anchor")).unwrap_or(300.0));
    let help_y = ctx.data(|d| d.get_temp::<f32>(egui::Id::new("sc_help_y")).unwrap_or(crate::toolbar::APP_BAR_HEIGHT));
    crate::help::show(app, &ctx, pos2(help_x - 14.0, help_y));
    menus(app, &ctx);
}

fn menus(app: &mut SolveApp, ctx: &egui::Context) {
    let Some((name, at)) = app.shell_ui.menu else { return };
    if !matches!(name, "Edit" | "Inspect" | "Window") {
        return;
    }
    let mut run: Option<Box<dyn FnOnce(&mut SolveApp)>> = None;
    let row = |ui: &mut egui::Ui, label: &str, key: &str, icon: &str, on: bool| {
        crate::context_menu::list_row(ui, &crate::context_menu::Item::action(label, label, icon).key(key).on_if(on)).is_some()
    };
    let shown = design_shown(app);
    let close = popup(ctx, "sc_menu_popup", at, app.shell_ui.opened, |ui| {
        ui.set_min_width(220.0);
        match name {
            "Edit" => {
                if row(ui, "Undo", "Ctrl+Z", "undo", shown && !app.session.undo.is_empty()) {
                    run = Some(Box::new(|app| FileAction::Undo.run(app)));
                }
                if row(ui, "Redo", "Ctrl+Y", "redo", shown && !app.session.redo.is_empty()) {
                    run = Some(Box::new(|app| FileAction::Redo.run(app)));
                }
                ui.separator();
                for (id, label, key, icon) in [
                    ("edit.copy", "Copy", "Ctrl+C", "copy"),
                    ("edit.paste", "Paste", "Ctrl+V", "paste"),
                    ("edit.delete", "Delete", "Del", "delete"),
                    ("select.all", "Select All", "Ctrl+A", "select"),
                ] {
                    if solidcraft_engine::cmd::find_command(id).is_some() && row(ui, label, key, icon, shown) {
                        run = Some(Box::new(move |app| app.start(id)));
                    }
                }
                ui.separator();
                if row(ui, "Preferences…", "Ctrl+,", "settings", true) {
                    run = Some(Box::new(|app| crate::prefs::open_at(app, 0)));
                }
            }
            "Inspect" => {
                let mut panel = "";
                for section in crate::workspace::SECTIONS.iter().filter(|s| s.tab == "INSPECT") {
                    if !panel.is_empty() && panel != section.panel {
                        ui.separator();
                    }
                    panel = section.panel;
                    for id in section.ids {
                        let Some(spec) = solidcraft_engine::cmd::find_command(id) else { continue };
                        let id: &'static str = spec.id;
                        if row(ui, spec.label, "", spec.icon, shown) {
                            run = Some(Box::new(move |app| app.start(id)));
                        }
                    }
                }
            }
            "Window" => {
                let check = |on: bool| if on { "✓" } else { "" };
                let items: [(&str, bool); 6] = [
                    ("Browser", app.ui.show_browser),
                    ("History", app.ui.show_timeline),
                    ("Properties", app.shell_ui.properties),
                    ("Navigation bar", app.ui.show_navigation_bar),
                    ("Grid", app.ui.show_grid),
                    ("Assistant", false),
                ];
                for (label, on) in items {
                    if label == "Assistant" && !crate::agent_chat::app_bar_button_shown(app) {
                        continue;
                    }
                    if row(ui, label, check(on), "", true) {
                        run = Some(Box::new(move |app| match label {
                            "Browser" => app.ui.show_browser = !app.ui.show_browser,
                            "History" => app.ui.show_timeline = !app.ui.show_timeline,
                            "Properties" => app.shell_ui.properties = !app.shell_ui.properties,
                            "Navigation bar" => app.ui.show_navigation_bar = !app.ui.show_navigation_bar,
                            "Grid" => app.ui.show_grid = !app.ui.show_grid,
                            _ => FileAction::AgentChat.run(app),
                        }));
                    }
                }
                ui.separator();
                if row(ui, "Drafting Settings…", "", "snap", true) {
                    run = Some(Box::new(|app| app.cad.settings.dialog = true));
                }
                if row(ui, "Customize application bar…", "", "settings", true) {
                    run = Some(Box::new(|app| app.shell_ui.customize = true));
                }
            }
            _ => {}
        }
    });
    if let Some(f) = run {
        app.shell_ui.menu = None;
        f(app);
    } else if close {
        app.shell_ui.menu = None;
    }
}

// ---- The document tabs ----------------------------------------------------------------------------

/// The open designs' tabs, in their own bar under the workspace ribbon.
pub fn tabs_bar(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let edge = Container::new(ui.ctx(), Surface::ApplicationBar).edge("sc_doc_tabs_bar", Edge::Top);
    let frame = Container::new(ui.ctx(), Surface::ApplicationBar).edge_frame(t.app_bar);
    Container::new(ui.ctx(), Surface::ApplicationBar).show_edge(ui, edge.exact_size(TABS_BAR_HEIGHT).show_separator_line(false), frame, |ui| {
        let r = ui.max_rect();
        let (x0, x1) = (r.left() + 6.0, r.right() - 6.0);
        let mut tabs_ui = ui.new_child(egui::UiBuilder::new().max_rect(r));
        tabs_ui.set_clip_rect(r.intersect(ui.clip_rect()));
        crate::documents::tabs(app, &mut tabs_ui, r, x0, x1);
    });
}

// ---- Properties -----------------------------------------------------------------------------------

/// The feature that made the selected object (or the object under an edited face or edge).
fn selected_feature(app: &SolveApp) -> Option<(String, u64)> {
    use solidcraft_engine::Sel;
    let body = app.session.selection.iter().find_map(|s| match s {
        Sel::Body { name } | Sel::Face { body: name, .. } | Sel::Edge { body: name, .. } => Some(name.clone()),
        _ => None,
    })?;
    let st = crate::open_meshing::session_world(&app.session);
    let feature = st.body(&body)?.feature;
    Some((body, feature))
}

/// The Properties window: the selected object, the feature that made it and its inputs (depth,
/// angle, …), each typed in and applied on Enter.
pub fn properties(app: &mut SolveApp, ctx: &egui::Context) {
    crate::cad::drafting_settings(app, ctx);
    crate::hatch_tool::window(app, ctx);
    if !app.shell_ui.properties || !design_shown(app) {
        return;
    }
    let mut open = true;
    // The selected object, else the one whose sketch is open for editing.
    let found = selected_feature(app).or_else(|| {
        let feature = app.forms.chain.as_ref().map(|c| c.feature).or(app.forms.after_edit.map(|a| a.1))?;
        let st = crate::open_meshing::session_world(&app.session);
        st.bodies.iter().find(|b| b.feature == feature).map(|b| (b.name.clone(), feature))
    });
    let mut open_sketch: Option<(String, u64)> = None;
    let info = found.as_ref().and_then(|(_, f)| app.session.execute("timeline.inspect", &json!({"feature": f})).ok());
    let mut apply: Option<(u64, usize, String)> = None;
    let mut edit_sketch: Option<String> = None;
    let mut edit_feature: Option<u64> = None;
    let mut blend_apply: Option<(u64, usize, String)> = None;
    let docked = app.preferences.properties_docked && app.preferences.panels_limit_view;
    let mut toggle_dock = false;
    let mut body = |app: &mut SolveApp, ui: &mut egui::Ui| {
        // A form being drawn shows its values; a selection in the open sketch, its curves.
        if crate::browser::hatches::properties_ui(app, ui)
            || crate::sketch_props::ui(app, ui)
            || crate::forms::properties_ui(app, ui)
            || crate::forms::chain_properties_ui(app, ui)
        {
            return;
        }
        let Some((body, feature)) = &found else {
            ui.label(egui::RichText::new("Select an object to see its properties.").color(Tokens::get().text_dim));
            return;
        };
        let Some(info) = &info else {
            ui.label("This object has no editable feature.");
            return;
        };
        egui::Grid::new("sc_props").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
            ui.label("Object");
            ui.label(egui::RichText::new(body).strong());
            ui.end_row();
            ui.label("Made by");
            ui.label(format!("{} ({})", info["type_name"].as_str().unwrap_or(""), info["name"].as_str().unwrap_or("")));
            ui.end_row();
            let inputs = info["inputs"].as_array().cloned().unwrap_or_default();
            for (i, input) in inputs.iter().enumerate() {
                let label = input["label"].as_str().unwrap_or("Value");
                let expr = input["expression"].as_str().unwrap_or("").to_string();
                ui.label(label);
                let editing = app.shell_ui.editing.as_ref().filter(|(f, k, _)| f == feature && *k == i).map(|e| e.2.clone());
                let mut text = editing.clone().unwrap_or(expr.clone());
                let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0));
                if resp.changed() || resp.has_focus() {
                    app.shell_ui.editing = Some((*feature, i, text.clone()));
                }
                if resp.lost_focus() {
                    if ui.input(|k| k.key_pressed(egui::Key::Enter)) && text.trim() != expr {
                        apply = Some((*feature, i, text.clone()));
                    }
                    app.shell_ui.editing = None;
                }
                ui.end_row();
            }
            if let Some(rows) = blend_rows(app, *feature) {
                for (k, (label, value)) in rows.iter().enumerate() {
                    ui.label(*label);
                    let key = 1000 + k;
                    let editing = app.shell_ui.editing.as_ref().filter(|(f, i, _)| f == feature && *i == key).map(|e| e.2.clone());
                    let mut text = editing.unwrap_or(value.clone());
                    let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0));
                    if resp.changed() || resp.has_focus() {
                        app.shell_ui.editing = Some((*feature, key, text.clone()));
                    }
                    if resp.lost_focus() {
                        if ui.input(|k| k.key_pressed(egui::Key::Enter)) && text.trim() != value.trim() {
                            blend_apply = Some((*feature, k, text.clone()));
                        }
                        app.shell_ui.editing = None;
                    }
                    ui.end_row();
                }
            }
            if let Some(e) = info["error"].as_str() {
                ui.label("Error");
                ui.colored_label(Tokens::get().error, e);
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        let sketches = object_sketches(app, *feature);
        if !sketches.is_empty() {
            ui.label("Sketches");
            ui.horizontal_wrapped(|ui| {
                for (label, sketch) in &sketches {
                    let current = app.session.active_sketch == Some(*sketch);
                    let r = ui.add_enabled(!current, egui::Button::new(label.as_str()).selected(current));
                    crate::scenario::publish_target(&format!("props:sketch:{label}"), r.rect);
                    if r.clicked() {
                        open_sketch = Some((body.clone(), *sketch));
                    }
                }
            });
        } else if ui.button("Edit…").clicked() {
            edit_feature = Some(*feature);
        }
        let _ = &mut edit_sketch;
        ui.label(egui::RichText::new("Type a value and press Enter to apply it.").small().color(Tokens::get().text_dim));
    };
    if docked {
        // Docked in the right column, below the sketch's panel while a sketch is open.
        let full = ctx.content_rect();
        let column = crate::dock::right_column(app, full);
        let mut top = column.top();
        if app.session.active_sketch.is_some()
            && let Some(p) = app.viewport.palette_rect
        {
            top = (p.bottom() + crate::shell::SHELL_GAP).min(column.bottom() - 120.0).max(column.top());
        }
        let rect = Rect::from_min_max(pos2(column.left(), top), column.max);
        let t = Tokens::get();
        let host = Container::new(ctx, Surface::Panel);
        let frame = host.frame(ctx, t.panel, crate::frame::Side::Right).inner_margin(8);
        let shown = host.area(egui::Id::new("sc_properties_dock")).fixed_pos(rect.min).show(ctx, |ui| {
            host.show_frame(ui, frame, rect, |ui| {
                let inner = rect.size() - frame.total_margin().sum();
                ui.set_width(inner.x.max(10.0));
                ui.set_min_height(inner.y.max(10.0));
                ui.set_max_height(inner.y.max(10.0));
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Properties").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").on_hover_text("Close").clicked() {
                            open = false;
                        }
                        if ui.small_button("⧉").on_hover_text("Float (undock)").clicked() {
                            toggle_dock = true;
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt("sc_properties_scroll").auto_shrink([false, false]).show(ui, |ui| body(app, ui));
            });
        });
        app.shell.properties = Some(shown.response.rect);
        crate::dock::width_handle(app, ctx, column);
    } else {
        app.shell.properties = None;
        egui::Window::new("Properties")
            .open(&mut open)
            .default_pos(pos2(ctx.content_rect().right() - 320.0, 160.0))
            .default_width(280.0)
            .resizable(true)
            .show(ctx, |ui| {
                if ui.small_button("Dock right").on_hover_text("Dock in the right column").clicked() {
                    toggle_dock = true;
                }
                body(app, ui);
            });
    }
    if toggle_dock {
        app.preferences.properties_docked = !app.preferences.properties_docked;
        app.preferences.panels_limit_view = true;
    }
    if !open {
        app.shell_ui.properties = false;
    }
    if let Some((feature, index, expression)) = apply {
        match app.run("timeline.set_input", json!({"feature": feature, "index": index, "expression": expression})) {
            Ok(_) => app.set_status("Applied", false),
            Err(e) => app.set_status(e, true),
        }
    }
    if let Some((feature, row, text)) = blend_apply {
        match set_blend(app, feature, row, &text) {
            Ok(()) => app.set_status("Applied", false),
            Err(e) => app.set_status(e, true),
        }
    }
    if let Some(body) = edit_sketch {
        crate::objects::edit_object(app, &body);
    }
    if let Some((body, sketch)) = open_sketch {
        crate::objects::edit_object_sketch(app, &body, Some(sketch));
    }
    if let Some(f) = edit_feature {
        app.edit_feature(f);
    }
}

/// The sketches an object was made from, each with what it is for.
fn object_sketches(app: &SolveApp, feature: u64) -> Vec<(String, u64)> {
    use solidcraft_engine::doc::FeatureKind;
    let Some(f) = app.session.doc.feature(feature) else { return Vec::new() };
    match &f.kind {
        FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } => vec![("Sketch".into(), sketch.0)],
        FeatureKind::Loft { sections, .. } => {
            let n = sections.len();
            sections
                .iter()
                .enumerate()
                .map(|(i, x)| {
                    let label = match (n, i) {
                        (2, 0) => "Base".to_string(),
                        (2, _) => "Top".to_string(),
                        _ => format!("Section {}", i + 1),
                    };
                    (label, x.sketch.0)
                })
                .collect()
        }
        FeatureKind::Sweep { sketch, path_sketch, .. } => vec![("Path".into(), path_sketch.0), ("Profile".into(), sketch.0)],
        _ => Vec::new(),
    }
}

/// A Blend's two sketches (base, top), when `feature` is a loft between two sketches.
fn blend_sketches(app: &SolveApp, feature: u64) -> Option<(u64, u64)> {
    match &app.session.doc.feature(feature)?.kind {
        solidcraft_engine::doc::FeatureKind::Loft { sections, .. } if sections.len() == 2 => {
            let (a, b) = (sections.first()?.sketch.0, sections.get(1)?.sketch.0);
            (a != b).then_some((a, b))
        }
        _ => None,
    }
}

fn offset_expr(app: &mut SolveApp, sketch: u64) -> String {
    app.session
        .execute("timeline.set_plane_offset", &json!({"sketch": sketch, "read": true}))
        .ok()
        .and_then(|v| v["offset"].as_str().map(str::to_string))
        .unwrap_or_else(|| "0 mm".into())
}

/// A length expression's value in mm, when it is a plain number with or without `mm`.
fn plain_mm(e: &str) -> Option<f64> {
    e.trim().trim_end_matches("mm").trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The distance between a Blend's sketch planes, along the base's normal.
fn blend_distance(app: &SolveApp, base: u64, top: u64) -> Option<f64> {
    let st = crate::open_meshing::session_world(&app.session);
    let (b, t) = (st.sketch(base)?.plane, st.sketch(top)?.plane);
    Some((t.origin - b.origin).dot(b.normal()))
}

/// Start, End and Distance of a Blend (like an Extrusion's), for the Properties window.
fn blend_rows(app: &mut SolveApp, feature: u64) -> Option<Vec<(&'static str, String)>> {
    let (base, top) = blend_sketches(app, feature)?;
    let start = offset_expr(app, base);
    let end = offset_expr(app, top);
    let d = blend_distance(app, base, top)?;
    let fmt = |v: f64| {
        let r = (v * 1e4).round() / 1e4;
        format!("{} mm", if r == 0.0 { 0.0 } else { r })
    };
    Some(vec![("Start", start), ("End", end), ("Distance", fmt(d.abs()))])
}

fn set_blend(app: &mut SolveApp, feature: u64, row: usize, text: &str) -> Result<(), String> {
    let (base, top) = blend_sketches(app, feature).ok_or("not a Blend")?;
    match row {
        0 => app.run("timeline.set_plane_offset", json!({"sketch": base, "offset": text})).map(|_| ()),
        1 => app.run("timeline.set_plane_offset", json!({"sketch": top, "offset": text})).map(|_| ()),
        _ => {
            let want = plain_mm(text).filter(|v| *v > 0.0).ok_or("Distance: a length above zero, in mm")?;
            let now = blend_distance(app, base, top).ok_or("the Blend's sketches are gone")?;
            if now.abs() < 1e-9 {
                return Err("the Blend's sketches lie on one plane: set End instead".into());
            }
            // The top moves along the base's normal, keeping its side.
            let delta = (want - now.abs()) * now.signum();
            let end = offset_expr(app, top);
            // The top's offset runs along its own base plane's normal, which may face the
            // other way from the Blend's base.
            let st = crate::open_meshing::session_world(&app.session);
            let same = match (st.sketch(base), st.sketch(top)) {
                (Some(b), Some(t)) => t.plane.normal().dot(b.plane.normal()) >= 0.0,
                _ => true,
            };
            drop(st);
            let delta = if same { delta } else { -delta };
            let next = match plain_mm(&end) {
                Some(v) => format!("{} mm", ((v + delta) * 1e6).round() / 1e6),
                None => format!("({end}) + {delta} mm"),
            };
            app.run("timeline.set_plane_offset", json!({"sketch": top, "offset": next})).map(|_| ())
        }
    }
}

#[cfg(test)]
pub(crate) fn blend_rows_for_tests(app: &mut SolveApp, feature: u64) -> Option<Vec<(&'static str, String)>> {
    blend_rows(app, feature)
}

#[cfg(test)]
pub(crate) fn set_blend_for_tests(app: &mut SolveApp, feature: u64, row: usize, text: &str) -> Result<(), String> {
    set_blend(app, feature, row, text)
}

/// Saved with the preferences.
pub fn prefs(app: &SolveApp) -> Value {
    json!({"quick": app.shell_ui.quick, "properties": app.shell_ui.properties, "drafting": app.cad.settings.to_json(), "hatch": crate::hatch_tool::to_json(app)})
}

pub fn load_prefs(app: &mut SolveApp, v: &Value) {
    if let Some(q) = v.get("quick").and_then(Value::as_array) {
        app.shell_ui.quick = q.iter().filter_map(Value::as_str).filter(|id| QUICK.iter().any(|x| x.0 == *id)).map(str::to_string).collect();
    }
    if let Some(p) = v.get("properties").and_then(Value::as_bool) {
        app.shell_ui.properties = p;
    }
    if let Some(d) = v.get("drafting") {
        app.cad.settings.load(d);
    }
    if let Some(h) = v.get("hatch") {
        crate::hatch_tool::load(app, h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_input_is_set_from_properties() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 10})).unwrap();
        let f = h.app.session.doc.features.last().map(|f| f.id).unwrap();
        let info = h.app.session.execute("timeline.inspect", &json!({"feature": f})).unwrap();
        assert!(!info["inputs"].as_array().unwrap().is_empty(), "{info}");
        h.app.run("timeline.set_input", json!({"feature": f, "index": 0, "expression": "55 mm"})).unwrap();
        let info = h.app.session.execute("timeline.inspect", &json!({"feature": f})).unwrap();
        assert_eq!(info["inputs"][0]["expression"], "55 mm", "{info}");
        assert!(h.app.run("timeline.set_input", json!({"feature": f, "index": 99, "expression": "1"})).is_err());
    }

    #[test]
    fn undo_history_and_quick_bar_defaults() {
        assert_eq!(default_quick(), vec!["save", "undo", "redo"]);
        assert!(default_quick().iter().all(|q| QUICK.iter().any(|x| x.0 == q)));
    }

    fn click(h: &mut crate::scenario::Harness, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        h.frame_with(vec![egui::Event::PointerMoved(at)]);
        h.frame_with(vec![button(true)]);
        h.frame_with(vec![button(false)]);
        h.frames(3);
    }

    #[test]
    fn every_menu_opens_and_closes_without_hanging() {
        let mut h = crate::scenario::Harness::new();
        h.frames(3);
        for name in ["file", "edit", "inspect", "window", "help"] {
            let at = crate::scenario::handle_at(&format!("menu:{name}")).unwrap_or_else(|| panic!("menu:{name}"));
            click(&mut h, at);
            click(&mut h, egui::pos2(700.0, 500.0));
        }
        let at = crate::scenario::handle_at("appbar:logo").unwrap();
        click(&mut h, at);
        click(&mut h, egui::pos2(700.0, 500.0));
        assert!(crate::scenario::rect_of("tabs:card").is_some() || crate::scenario::handles("tab:design:").len() > 0);
    }
}
