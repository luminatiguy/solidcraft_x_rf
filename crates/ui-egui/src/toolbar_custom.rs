//! Toolbar customisation: which commands each panel shows as buttons, per workspace and tab.
//! A command's menu (right-click on a toolbar button, a panel drop-down row or an S box row)
//! pins it to the toolbar, removes it, or pins it to the S box's favourites; buttons are dragged
//! to reorder them within and between panels; a drop-down row dragged onto the toolbar pins it
//! there, a button dragged off the toolbar (or back into the drop-down) leaves it; the pin icon
//! on a hovered drop-down row pins or unpins; the toolbar's own menu (the plus after the tabs,
//! or right-click its empty space or a tab) shows or hides each tab, and has Customize Toolbar…
//! (a window listing every command of the tab with a check box) and Reset Toolbar; Reset Toolbar
//! restores the tab. The buttons are kept with the preferences and exported or imported with the
//! keyboard shortcuts; hidden tabs are kept in the `hidden_toolbar_tabs` preference.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_engine::{CommandSpec, command_specs};

use crate::SolveApp;
use crate::context_menu::Item;

/// The only workspace so far.
pub const WORKSPACE: &str = "DESIGN";

#[derive(Default, Clone, Debug, PartialEq)]
pub struct ToolbarCustom {
    /// `"<workspace>/<tab>/<panel>"` → the panel's buttons, in order (only panels the user
    /// changed).
    pub panels: BTreeMap<String, Vec<String>>,
    /// `"<workspace>/<tab>"` → gallery commands moved to full-size buttons.
    pub gallery_buttons: BTreeMap<String, Vec<String>>,
    /// `"<tab>/<command>"` → "small" or "large": the size the user gave a button.
    pub sizes: BTreeMap<String, String>,
    /// The Customize Toolbar window is open (not kept).
    pub customizing: bool,
}

impl ToolbarCustom {
    /// Whether a command still uses a compact gallery cell in this panel.
    pub fn in_gallery(&self, tab: &str, panel: &str, id: &str) -> bool {
        match self.sizes.get(&format!("{tab}/{id}")).map(String::as_str) {
            Some("small") => return true,
            Some("large") => return false,
            _ => {}
        }
        if crate::workspace::small_tool(tab, panel, id) {
            return true;
        }
        tab == "SKETCH"
            && panel == "CONSTRAIN"
            && crate::workspace::CONSTRAINT_GALLERY.contains(&id)
            && !self.gallery_buttons.get(&format!("{WORKSPACE}/{tab}")).is_some_and(|ids| ids.iter().any(|c| c == id))
    }

    pub fn prefs(&self) -> Value {
        json!({"version": 3, "panels": self.panels, "gallery_buttons": self.gallery_buttons, "sizes": self.sizes})
    }
    pub fn load(&mut self, v: &Value) {
        let version = v.get("version").and_then(Value::as_u64).unwrap_or(0);
        let current = version >= 2;
        let panels = if current { v.get("panels").unwrap_or(&Value::Null) } else { v };
        self.panels = panels
            .as_object()
            .map(|o| {
                o.iter()
                    .take(500)
                    .filter_map(|(k, v)| {
                        let ids: Vec<String> = v.as_array()?.iter().take(100).filter_map(|x| x.as_str().map(str::to_string)).collect();
                        Some((k.clone(), ids))
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.gallery_buttons = v
            .get("gallery_buttons")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .take(500)
                    .filter_map(|(k, v)| {
                        let ids = v.as_array()?.iter().take(100).filter_map(|x| x.as_str().map(str::to_string)).collect();
                        Some((k.clone(), ids))
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.sizes = v
            .get("sizes")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .take(2000)
                    .filter_map(|(k, v)| v.as_str().filter(|s| matches!(*s, "small" | "large")).map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        if !current {
            self.migrate();
        }
        if version < 3 {
            self.migrate_render_tabs();
        }
    }
    /// Split the former six-section Render ribbon without losing personal pins or removals.
    fn migrate_render_tabs(&mut self) {
        let mut migrated_panels: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (old_tab, old, targets) in [
            ("RENDER", "SCENE", &[("RENDER CAMERA", "SHOTS"), ("RENDER CAMERA", "VIEW"), ("RENDER SCENE", "STUDIO")][..]),
            ("RENDER", "APPEARANCE", &[("RENDER APPEARANCE", "APPEARANCE")][..]),
            ("RENDER", "LIGHTING", &[("RENDER SCENE", "STUDIO")][..]),
            ("RENDER", "ENVIRONMENT", &[("RENDER SCENE", "STAGE")][..]),
            ("RENDER", "CAMERA", &[("RENDER CAMERA", "LENS"), ("RENDER CAMERA", "FRAME"), ("RENDER CAMERA", "VIEW")][..]),
            ("RENDER", "IMAGE", &[("RENDER", "IMAGE"), ("RENDER", "RENDER"), ("RENDER", "RESULT")][..]),
            ("RENDER LIGHTING", "LIGHTING", &[("RENDER SCENE", "STUDIO"), ("RENDER SCENE", "LIGHTS")][..]),
            ("RENDER LIGHTING", "ENVIRONMENT", &[("RENDER SCENE", "STAGE"), ("RENDER SCENE", "STUDIO")][..]),
            ("RENDER", "OUTPUT", &[("RENDER", "RENDER"), ("RENDER", "IMAGE")][..]),
            ("RENDER", "RESULTS", &[("RENDER", "RESULT"), ("RENDER", "IMAGE")][..]),
        ] {
            let Some(ids) = self.panels.remove(&key(old_tab, old)) else { continue };
            for &(tab, panel) in targets {
                let k = key(tab, panel);
                let migrated: Vec<_> = ids
                    .iter()
                    .filter(|id| solidcraft_engine::find_command(id).and_then(crate::workspace::legacy_render_destination) == Some((tab, panel)))
                    .cloned()
                    .collect();
                let list = migrated_panels.entry(k).or_default();
                for id in migrated {
                    if !list.contains(&id) {
                        list.push(id);
                    }
                }
            }
            // Preserve deliberately cross-pinned commands with no destination in this split.
            if let Some(&(tab, panel)) = targets.first() {
                let list = migrated_panels.entry(key(tab, panel)).or_default();
                for id in ids {
                    let destination = solidcraft_engine::find_command(&id).and_then(crate::workspace::legacy_render_destination);
                    if !targets.iter().any(|target| Some(*target) == destination) && !list.contains(&id) {
                        list.push(id);
                    }
                }
            }
        }
        for (k, ids) in migrated_panels {
            // New-layout customisation is authoritative when both versions are present.
            self.panels.entry(k).or_insert(ids);
        }
    }

    fn migrate(&mut self) {
        use std::collections::BTreeSet;
        let specs = command_specs();
        let mut pins: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut removals: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let old = std::mem::take(&mut self.panels);
        for (k, ids) in old {
            let parts: Vec<_> = k.split('/').collect();
            let ["DESIGN", tab, panel] = parts.as_slice() else {
                self.panels.insert(k, ids);
                continue;
            };
            if crate::workspace::legacy_panel(tab, panel).is_none() {
                self.panels.insert(k, ids);
                continue;
            }
            if let Some((t, p)) = crate::workspace::legacy_panel(tab, panel) {
                pins.entry(key(t, p)).or_default();
            }
            for id in crate::workspace::legacy_buttons(tab, panel) {
                if !ids.iter().any(|x| x == id)
                    && let Some((t, p)) = crate::workspace::legacy_destination(tab, panel, id)
                {
                    removals.entry(key(t, p)).or_default().insert(id.into());
                }
            }
            for id in ids {
                if let Some((t, p)) = crate::workspace::legacy_destination(tab, panel, &id) {
                    let list = pins.entry(key(t, p)).or_default();
                    if !list.contains(&id) {
                        list.push(id);
                    }
                }
            }
        }
        let touched: BTreeSet<_> = pins.keys().chain(removals.keys()).cloned().collect();
        for k in touched {
            if self.panels.contains_key(&k) {
                continue;
            } // An existing new destination wins.
            let mut ids = pins.remove(&k).unwrap_or_default();
            let parts: Vec<_> = k.split('/').collect();
            if let [_, tab, panel] = parts.as_slice()
                && let Some((cmds, n)) = crate::workspace::layout(tab, panel, &specs)
            {
                for c in cmds.iter().take(n) {
                    if !ids.iter().any(|id| id == c.id) && !removals.get(&k).is_some_and(|r| r.contains(c.id)) {
                        ids.push(c.id.into());
                    }
                }
            }
            self.panels.insert(k, ids);
        }
    }
}

fn key(tab: &str, panel: &str) -> String {
    format!("{WORKSPACE}/{tab}/{panel}")
}

/// The buttons a panel shows: the user's list, or the first `promoted` commands of the panel.
pub fn buttons(app: &SolveApp, tab: &str, panel: &str, cmds: &[&'static CommandSpec], promoted: usize) -> Vec<&'static CommandSpec> {
    match app.toolbar_custom.panels.get(&key(tab, panel)) {
        Some(ids) => ids.iter().filter_map(|id| solidcraft_engine::find_command(id)).collect(),
        // Small tools are shown too, as cells after the large buttons.
        None if crate::workspace::SMALL_TOOLS.contains(&(tab, panel)) => cmds.to_vec(),
        None => cmds.iter().take(promoted).copied().collect(),
    }
}

/// A tab's panels with their commands and default button counts.
pub type PanelLists = Vec<(&'static str, Vec<&'static CommandSpec>, usize)>;

/// The panels of a tab with their commands and default button counts (what the toolbar draws).
/// Made from the command registry and the ribbon's layout, which never change while the app
/// runs, so each tab's lists are worked out once: the toolbar asks for them every frame, and
/// working them out matches every command against every section (about a millisecond a frame).
pub fn panel_lists(tab: &str) -> PanelLists {
    static LISTS: std::sync::Mutex<BTreeMap<String, PanelLists>> = std::sync::Mutex::new(BTreeMap::new());
    let Ok(mut lists) = LISTS.lock() else { return panel_lists_of(tab) };
    lists.entry(tab.to_string()).or_insert_with(|| panel_lists_of(tab)).clone()
}

/// [`panel_lists`] worked out from the registry.
fn panel_lists_of(tab: &str) -> PanelLists {
    let specs = command_specs();
    crate::toolbar::panels(tab)
        .iter()
        .filter_map(|panel| {
            let (cmds, n) = crate::workspace::layout(tab, panel, &specs)?;
            (!cmds.is_empty() || *panel == "SELECT").then_some((*panel, cmds, n))
        })
        .collect()
}

/// The buttons of a panel as ids, made the user's own list (so it can change).
fn own(app: &mut SolveApp, tab: &str, panel: &str) -> Vec<String> {
    let k = key(tab, panel);
    if let Some(v) = app.toolbar_custom.panels.get(&k) {
        return v.clone();
    }
    let lists = panel_lists(tab);
    let ids: Vec<String> =
        lists.iter().find(|(p, ..)| *p == panel).map(|(_, cmds, n)| cmds.iter().take(*n).map(|c| c.id.to_string()).collect()).unwrap_or_default();
    ids
}

/// Where a command is a button on a tab now: its panel.
pub fn pinned_on(app: &SolveApp, tab: &str, id: &str) -> Option<&'static str> {
    panel_lists(tab).into_iter().find(|(panel, cmds, n)| buttons(app, tab, panel, cmds, *n).iter().any(|c| c.id == id)).map(|(p, ..)| p)
}

/// The panel a command belongs to on a tab (its drop-down), or the tab's first panel.
fn home_panel(tab: &str, id: &str) -> Option<&'static str> {
    let lists = panel_lists(tab);
    lists.iter().find(|(_, cmds, _)| cmds.iter().any(|c| c.id == id)).or(lists.first()).map(|(p, ..)| *p)
}

/// FINISH EDITING, shown only while a linked design is edited in place, keeps its one button:
/// nothing moves into it, and Finish Edit moves nowhere else.
fn fixed(tab: &str, panel: &str, id: &str) -> bool {
    (tab == "MODEL" && panel == "FINISH EDITING") || id == "component.finish_edit"
}

/// Pin a command to the toolbar of a tab (as a button of its panel, at the end).
pub fn pin(app: &mut SolveApp, tab: &str, id: &str) -> Result<(), String> {
    if solidcraft_engine::find_command(id).is_none() {
        return Err(format!("no command `{id}`"));
    }
    if pinned_on(app, tab, id).is_some() || fixed(tab, "", id) {
        return Ok(());
    }
    let panel = home_panel(tab, id).ok_or_else(|| format!("the {tab} tab has no panels"))?;
    let mut ids = own(app, tab, panel);
    ids.push(id.to_string());
    app.toolbar_custom.panels.insert(key(tab, panel), ids);
    Ok(())
}

/// Remove a command's button from a tab (it stays in its panel's drop-down).
pub fn remove(app: &mut SolveApp, tab: &str, id: &str) {
    let Some(panel) = pinned_on(app, tab, id).filter(|panel| !fixed(tab, panel, id)) else { return };
    let mut ids = own(app, tab, panel);
    ids.retain(|x| x != id);
    app.toolbar_custom.panels.insert(key(tab, panel), ids);
}

/// Move a button before another one (`before`: None puts it at the end of `panel`).
pub fn reorder(app: &mut SolveApp, tab: &str, id: &str, panel: &str, before: Option<&str>) {
    if fixed(tab, panel, id) {
        return;
    }
    // A gallery command dropped on a gallery cell goes (back) into the gallery; dropped anywhere
    // else it becomes a full-size button.
    if tab == "SKETCH" && crate::workspace::CONSTRAINT_GALLERY.contains(&id) {
        let into_gallery = before.is_some_and(|b| app.toolbar_custom.in_gallery(tab, panel, b));
        let ids = app.toolbar_custom.gallery_buttons.entry(format!("{WORKSPACE}/{tab}")).or_default();
        ids.retain(|c| c != id);
        if !into_gallery {
            ids.push(id.to_string());
        }
    }
    if let Some(from) = pinned_on(app, tab, id) {
        let mut ids = own(app, tab, from);
        ids.retain(|x| x != id);
        app.toolbar_custom.panels.insert(key(tab, from), ids);
    }
    let mut ids = own(app, tab, panel);
    ids.retain(|x| x != id);
    let at = before.and_then(|b| ids.iter().position(|x| x == b)).unwrap_or(ids.len());
    ids.insert(at, id.to_string());
    app.toolbar_custom.panels.insert(key(tab, panel), ids);
}

/// The pin icon of a drop-down row: pin the command, or remove it when it is pinned.
pub fn toggle(app: &mut SolveApp, tab: &str, id: &str) {
    if pinned_on(app, tab, id).is_some() {
        remove(app, tab, id);
    } else if let Err(e) = pin(app, tab, id) {
        app.set_status(e, true);
    }
}

/// Reset Toolbar: a tab's panels back to their defaults.
pub fn reset(app: &mut SolveApp, tab: &str) {
    let prefix = format!("{WORKSPACE}/{tab}/");
    app.toolbar_custom.panels.retain(|k, _| !k.starts_with(&prefix));
    app.toolbar_custom.gallery_buttons.remove(&format!("{WORKSPACE}/{tab}"));
    let sizes = format!("{tab}/");
    app.toolbar_custom.sizes.retain(|k, _| !k.starts_with(&sizes));
}

/// The menu of a command (toolbar button, drop-down row, S box row).
pub fn command_items(app: &SolveApp, id: &str) -> Vec<Item> {
    let tab = app.ui.tab.clone();
    let on = pinned_on(app, &tab, id).is_some();
    let fav = app.sbox.pinned.iter().any(|p| p == id);
    let mut v = Vec::new();
    v.push(if on {
        Item::action("ui.toolbarRemove", "Remove from Toolbar", "close").with(json!({ "command": id }))
    } else {
        Item::action("ui.toolbarPin", "Pin to Toolbar", "plus").with(json!({ "command": id }))
    });
    v.push(Item::action("ui.shortcutPin", if fav { "Unpin from Shortcuts" } else { "Pin to Shortcuts" }, "star").with(json!({ "command": id })));
    v.push(Item::sep());
    v.push(Item::action("ui.toolbarCustomize", "Customize Toolbar…", "settings"));
    v.push(Item::action("ui.toolbarReset", "Reset Toolbar", "restore").with(json!({ "tab": tab })));
    v
}

/// The toolbar's own menu (right-click on its empty space or a tab): every tab that can be shown
/// or hidden, ticked when on, then the button customisation of the selected tab.
pub fn toolbar_items(app: &SolveApp) -> Vec<Item> {
    let mut v = Vec::new();
    if !crate::render_workspace::active(app) {
        v.push(Item::heading(app.tr("prefs.toolbar_tabs")));
        for tab in crate::workspace::choosable_tabs() {
            let on = crate::workspace::tab_turned_on(app, tab);
            // The row reads as the tab does, in every language.
            let item = Item::action("ui.toolbarTab", &crate::toolbar::ribbon_label(app, tab), crate::workspace::tab_icon(tab))
                .with(json!({ "tab": tab, "shown": !on }));
            v.push(if on { item.key("current") } else { item });
        }
        // With every tab hidden there is no tab to customise.
        if !crate::workspace::visible_tabs(app).contains(&app.ui.tab.as_str()) {
            return v;
        }
        v.push(Item::sep());
    }
    v.push(Item::action("ui.toolbarCustomize", "Customize Toolbar…", "settings"));
    v.push(Item::action("ui.toolbarReset", "Reset Toolbar", "restore").with(json!({ "tab": app.ui.tab })));
    v
}

/// Run a toolbar item from a menu.
pub fn run(app: &mut SolveApp, action: &str, p: &Value) {
    let tab = p.get("tab").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| app.ui.tab.clone());
    let id = p.get("command").and_then(Value::as_str).unwrap_or("");
    match action {
        "ui.toolbarPin" => {
            if let Err(e) = pin(app, &tab, id) {
                app.set_status(e, true);
            }
        }
        "ui.toolbarRemove" => remove(app, &tab, id),
        "ui.toolbarReset" => reset(app, &tab),
        "ui.toolbarCustomize" => app.toolbar_custom.customizing = true,
        "ui.toolbarTab" => crate::workspace::set_tab_shown(app, &tab, p.get("shown").and_then(Value::as_bool).unwrap_or(true)),
        "ui.shortcutPin" => app.sbox.toggle_pin(id),
        _ => {}
    }
}

/// Shortcut keys, S box favourites, toolbar and navigation bar as one file (Keyboard Shortcuts › Export).
pub fn export(app: &SolveApp) -> Value {
    json!({ "format": "solidcraft-shortcuts/1", "shortcuts": app.keymap.prefs(), "favourites": app.sbox.pinned, "toolbar": app.toolbar_custom.prefs(), "navigation_bar": app.preferences.navigation_bar })
}

/// The largest shortcuts file Import… reads (hostile input; an export is a few tens of KiB).
pub const MAX_IMPORT_BYTES: usize = 1 << 20;

pub fn import(app: &mut SolveApp, v: &Value) -> Result<(), String> {
    // "solvecraft-shortcuts/1": files exported before the rename (compatibility shim).
    if !matches!(v.get("format").and_then(Value::as_str), Some("solidcraft-shortcuts/1" | "solvecraft-shortcuts/1")) {
        return Err("not a SolidCraft shortcuts file".into());
    }
    let navigation_bar = v
        .get("navigation_bar")
        .map(|value| {
            crate::navigation_bar::validate(value)?;
            serde_json::from_value::<crate::navigation_bar::NavigationBar>(value.clone()).map_err(|e| e.to_string())
        })
        .transpose()?;
    if let Some(bar) = navigation_bar {
        app.preferences.navigation_bar = bar;
    }
    if let Some(s) = v.get("shortcuts") {
        app.keymap.load(s);
    }
    if let Some(f) = v.get("favourites").and_then(Value::as_array) {
        app.sbox.pinned = f.iter().take(64).filter_map(|x| x.as_str().map(str::to_string)).collect();
    }
    if let Some(t) = v.get("toolbar") {
        app.toolbar_custom.load(t);
    }
    Ok(())
}

/// Customize Toolbar: every command of the tab's panels with a check box (on the toolbar or
/// not), how to drag them, and Reset Toolbar.
/// What the tile editor was asked to do (applied after drawing it).
enum Edit {
    Remove(&'static str),
    Size(&'static str, bool),
    Move { id: String, panel: &'static str, before: Option<&'static str> },
    Add(&'static str, &'static str),
}

/// Customize Toolbar, laid out as the tiles of a start menu: each panel of the tab is a group of
/// tiles, large ones (icon and name) and small ones (icon only), in the order the ribbon shows
/// them. Drag a tile to move it (to another place or panel), right-click it to resize or remove
/// it, and add tools from the list on the left.
pub fn window(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.toolbar_custom.customizing {
        return;
    }
    let tab = app.ui.tab.clone();
    let mut edits: Vec<Edit> = Vec::new();
    let mut reset_tab = false;
    let mut open = true;
    let t = crate::theme::Tokens::get();
    let filter_id = egui::Id::new("sc_customize_filter");
    let mut filter: String = ctx.data(|d| d.get_temp(filter_id)).unwrap_or_default();
    egui::Window::new(format!("Customize Toolbar — {}", app.tr_label(&tab)))
        .id(egui::Id::new("sc_toolbar_customize"))
        .open(&mut open)
        .default_size(egui::vec2(900.0, 560.0))
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("Drag a tile to move it. Right-click a tile to resize or remove it. Click + to add a tool.").color(t.text_dim),
            );
            ui.add_space(4.0);
            ui.horizontal_top(|ui| {
                // The tools of the tab, by panel, with + to add one that is not on the ribbon.
                ui.vertical(|ui| {
                    ui.set_width(230.0);
                    ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Search tools").desired_width(220.0));
                    egui::ScrollArea::vertical().id_salt("customize_all").max_height(460.0).show(ui, |ui| {
                        for (panel, cmds, n) in panel_lists(&tab) {
                            let shown = buttons(app, &tab, panel, &cmds, n);
                            let rest: Vec<_> = cmds
                                .iter()
                                .filter(|c| !shown.iter().any(|b| b.id == c.id))
                                .filter(|c| filter.is_empty() || app.tr_label(c.label).to_lowercase().contains(&filter.to_lowercase()))
                                .collect();
                            if rest.is_empty() {
                                continue;
                            }
                            ui.label(egui::RichText::new(app.tr_label(panel)).small().strong().color(t.text_dim));
                            for c in rest {
                                ui.horizontal(|ui| {
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                                    crate::icons::paint_icon(ui.painter(), r, c.info(&app.session).icon, 1.0);
                                    if ui.small_button("+").on_hover_text("Add to the ribbon").clicked() {
                                        edits.push(Edit::Add(c.id, panel));
                                    }
                                    ui.label(app.tr_label(c.label));
                                });
                            }
                        }
                    });
                });
                ui.separator();
                // The tiles.
                egui::ScrollArea::both().id_salt("customize_tiles").max_height(480.0).show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        for (panel, cmds, n) in panel_lists(&tab) {
                            if cmds.is_empty() {
                                continue;
                            }
                            let shown = buttons(app, &tab, panel, &cmds, n);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(app.tr_label(panel)).strong());
                                let group = egui::Frame::new().fill(t.panel_header).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
                                    ui.set_min_size(egui::vec2(140.0, 200.0));
                                    ui.set_max_width(220.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                                        for c in &shown {
                                            let small = app.toolbar_custom.in_gallery(&tab, panel, c.id);
                                            let size = if small { egui::vec2(32.0, 32.0) } else { egui::vec2(68.0, 68.0) };
                                            let drag_id = egui::Id::new(("tile", &tab, c.id));
                                            let resp = ui
                                                .dnd_drag_source(drag_id, c.id.to_string(), |ui| {
                                                    let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                                                    ui.painter().rect_filled(r, 3.0, t.toolbar);
                                                    ui.painter().rect_stroke(r, 3.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                                                    let icon = c.info(&app.session).icon;
                                                    if small {
                                                        crate::icons::paint_icon(ui.painter(), r.shrink(6.0), icon, 1.0);
                                                    } else {
                                                        crate::icons::paint_icon(
                                                            ui.painter(),
                                                            egui::Rect::from_center_size(r.center() - egui::vec2(0.0, 9.0), egui::vec2(30.0, 30.0)),
                                                            icon,
                                                            1.0,
                                                        );
                                                        let label = app.tr_label(crate::workspace::ribbon_button_label(c));
                                                        let mut job = egui::text::LayoutJob::simple(
                                                            label.to_string(),
                                                            egui::FontId::proportional(10.0),
                                                            t.text,
                                                            r.width() - 4.0,
                                                        );
                                                        job.wrap.max_rows = 2;
                                                        job.halign = egui::Align::Center;
                                                        let g = ui.painter().layout_job(job);
                                                        ui.painter().galley(egui::pos2(r.center().x, r.bottom() - 24.0), g, t.text);
                                                    }
                                                })
                                                .response;
                                            crate::scenario::publish_target(&format!("customize:{}", c.id), resp.rect);
                                            if let Some(dragged) = resp.dnd_release_payload::<String>()
                                                && *dragged != c.id
                                            {
                                                edits.push(Edit::Move { id: (*dragged).clone(), panel, before: Some(c.id) });
                                            }
                                            if resp.dnd_hover_payload::<String>().is_some() {
                                                ui.painter().vline(resp.rect.left() - 2.0, resp.rect.y_range(), egui::Stroke::new(2.0, t.accent));
                                            }
                                            let label = app.tr_label(c.label).to_string();
                                            let id = c.id;
                                            resp.on_hover_text(&label).context_menu(|ui| {
                                                ui.label(egui::RichText::new(&label).strong());
                                                ui.separator();
                                                ui.menu_button("Resize", |ui| {
                                                    if ui.radio(small, "Small").clicked() {
                                                        edits.push(Edit::Size(id, true));
                                                        ui.close();
                                                    }
                                                    if ui.radio(!small, "Large").clicked() {
                                                        edits.push(Edit::Size(id, false));
                                                        ui.close();
                                                    }
                                                });
                                                if ui.button("Remove from the ribbon").clicked() {
                                                    edits.push(Edit::Remove(id));
                                                    ui.close();
                                                }
                                            });
                                        }
                                    });
                                });
                                // Dropped on the group's free space: to the end of this panel.
                                let zone = group.response;
                                if let Some(dragged) = zone.dnd_release_payload::<String>()
                                    && !edits.iter().any(|e| matches!(e, Edit::Move { .. }))
                                {
                                    edits.push(Edit::Move { id: (*dragged).clone(), panel, before: None });
                                }
                                if zone.dnd_hover_payload::<String>().is_some() {
                                    ui.painter().rect_stroke(zone.rect, 4.0, egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Outside);
                                }
                            });
                        }
                    });
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                let reset = ui.button(crate::i18n::tr_label("Reset Toolbar"));
                crate::scenario::publish_target("customize:reset", reset.rect);
                reset_tab |= reset.clicked();
            });
        });
    ctx.data_mut(|d| d.insert_temp(filter_id, filter));
    for e in edits {
        match e {
            Edit::Remove(id) => remove(app, &tab, id),
            Edit::Size(id, small) => {
                app.toolbar_custom.sizes.insert(format!("{tab}/{id}"), if small { "small" } else { "large" }.to_string());
            }
            Edit::Move { id, panel, before } => reorder(app, &tab, &id, panel, before),
            Edit::Add(id, panel) => {
                let mut ids = own(app, &tab, panel);
                if !ids.iter().any(|x| x == id) {
                    ids.push(id.to_string());
                }
                app.toolbar_custom.panels.insert(key(&tab, panel), ids);
            }
        }
    }
    if reset_tab {
        reset(app, &tab);
    }
    app.toolbar_custom.customizing = open;
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    /// Each tab's panel lists are worked out once and come back the same as worked out anew,
    /// for every tab the ribbon has (and an unknown one).
    #[test]
    fn panel_lists_are_kept_and_match_the_registry() {
        let tabs: std::collections::BTreeSet<&str> = crate::workspace::SECTIONS.iter().map(|s| s.tab).chain(["NO SUCH TAB"]).collect();
        let ids = |l: &PanelLists| l.iter().map(|(p, cmds, n)| (*p, cmds.iter().map(|c| c.id).collect::<Vec<_>>(), *n)).collect::<Vec<_>>();
        for tab in tabs {
            let fresh = panel_lists_of(tab);
            assert_eq!(ids(&panel_lists(tab)), ids(&fresh), "{tab}");
            assert_eq!(ids(&panel_lists(tab)), ids(&fresh), "{tab}, kept");
        }
    }

    #[test]
    fn legacy_pins_split_merge_and_preserve_removals_and_unknown_keys() {
        let mut custom = ToolbarCustom::default();
        custom.load(&json!({
            "DESIGN/SOLID/CREATE":["solid.hole","solid.revolve","solid.move"],
            "DESIGN/PLASTIC/CREATE":["plastic.boss","plastic.rest","sketch.create"],
            "DESIGN/SOLID/MODIFY":["solid.chamfer"],
            "DESIGN/UTILITIES/SELECT":[],
            "DESIGN/future/panel":["future.command"],
            "OTHER/SOLID/CREATE":["solid.extrude"]
        }));
        let create = &custom.panels["DESIGN/MODEL/CREATE"];
        assert_eq!(create, &["plastic.boss", "plastic.rest", "solid.hole", "solid.revolve"]);
        assert!(!create.iter().any(|id| id == "solid.extrude"));
        assert!(custom.panels["DESIGN/MODEL/ARRANGE"].contains(&"solid.move".into()));
        assert!(!custom.panels["DESIGN/MODEL/MODIFY"].contains(&"solid.fillet".into()));
        assert!(custom.panels["DESIGN/MODEL/MODIFY"].contains(&"solid.chamfer".into()));
        assert_eq!(custom.panels["DESIGN/future/panel"], vec!["future.command"]);
        assert!(custom.panels.contains_key("OTHER/SOLID/CREATE"));
        assert!(!custom.panels.contains_key("DESIGN/SOLID/CREATE"));
        let saved = custom.prefs();
        let mut reloaded = ToolbarCustom::default();
        reloaded.load(&saved);
        assert_eq!(custom, reloaded, "migration runs once, even for unchanged SKETCH/SURFACE keys");
    }

    #[test]
    fn empty_old_panels_keep_default_button_removals() {
        let mut custom = ToolbarCustom::default();
        custom.load(&json!({"DESIGN/SOLID/CREATE":[],"DESIGN/SKETCH/CONSTRAINTS":[],"DESIGN/PLASTIC/SETUP":[]}));
        assert!(!custom.panels["DESIGN/MODEL/SKETCH"].contains(&"sketch.create".into()));
        assert!(!custom.panels["DESIGN/MODEL/CREATE"].contains(&"solid.extrude".into()));
        assert!(!custom.panels["DESIGN/MANAGE/RULES"].contains(&"plastic.manage_rules".into()));
        assert_eq!(custom.panels["DESIGN/SKETCH/CONSTRAIN"], vec!["sketch.dimension"]);
    }
}
