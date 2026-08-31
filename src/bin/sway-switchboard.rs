//! sway-switchboard: a cascading mouse menu for sway workspaces.
//!
//! Hovering a group opens another panel to its left, as deep as the names go.
//! It is deliberately not a switcher you type into -- sway-ws-switch already
//! does fuzzy and MRU, and does it better. This is the other half: seeing the
//! shape of the desk with a pointer, and reaching a workspace without knowing
//! its name first.
//!
//! Panels grow leftwards because the button that opens it lives at the right
//! end of the bar; growing rightwards would walk off the screen immediately.

use gtk::prelude::*;
use gtk::{glib, pango};
use layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use sway_switchboard::icons::Icons;
use sway_switchboard::shell;
use sway_switchboard::sway::{self, Workspace};
use sway_switchboard::tree::{self, Node};

const APP_ID: &str = "dev.jamedjo.sway-switchboard";
const DEFAULT_CSS: &str = include_str!("../../style.css");

/// How long the pointer has to rest on a group before its panel opens.
///
/// This is the fix for the diagonal-travel problem: moving from a group row
/// towards its panel crosses the rows in between, and without a dwell every one
/// of them would swap the panel out from under the pointer. 180ms is long
/// enough to cross two rows and short enough not to feel like a wait.
const DWELL: Duration = Duration::from_millis(180);

/// How long the pointer may be off the menu before it gives up and closes.
const LEAVE_GRACE: Duration = Duration::from_millis(400);

struct State {
    workspaces: Vec<Workspace>,
    icons: Icons,
    /// Workspace ids, most recently focused first, from sway-ws-mru.
    mru: Vec<i64>,
    /// One entry per open panel: the path it represents.
    open: Vec<Vec<String>>,
}

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| shell::load_css("switchboard/style.css", DEFAULT_CSS));

    // Registered with GOption rather than read off argv, so it shows up in
    // --help alongside GTK's own options instead of being folklore.
    app.add_main_option(
        "overview",
        glib::Char::from(0),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Open centred and ordered by recency, for a gesture rather than a click",
        None,
    );
    let overview = Rc::new(Cell::new(false));
    {
        let flag = overview.clone();
        app.connect_handle_local_options(move |_, opts| {
            flag.set(opts.lookup::<bool>("overview").ok().flatten().unwrap_or(false));
            // Not handled here: carry on into the usual startup.
            std::ops::ControlFlow::Continue(())
        });
    }

    // GtkApplication is single-instance over D-Bus already, so a second launch
    // arrives here as another activate on the first process rather than as a
    // second process. Building a window unconditionally therefore stacked
    // menus on top of each other. Closing instead makes the bar button a
    // toggle -- click to open, click again to collapse -- and makes a second
    // copy impossible rather than merely unlikely.
    app.connect_activate(move |app| {
        if let Some(open) = app.windows().first() {
            open.close();
        } else {
            build_ui(app, overview.get());
        }
    });
    app.run()
}

fn build_ui(app: &gtk::Application, overview: bool) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .decorated(false)
        .build();

    // A layer-shell overlay rather than a window: it has no place in the
    // window tree, sway will not tile it, and it can take the keyboard only
    // while it is up.
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    // The bar's menu hangs under the button that opened it and grows leftwards
    // as you go deeper, so it is pinned to the top right. The grid is a fixed
    // block with nowhere to grow, and layer-shell centres a surface on any
    // axis it is not anchored to -- so leaving both edges alone is the whole
    // of what puts it in the middle of the screen.
    if !overview {
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Right, true);
        window.set_margin(Edge::Top, 4);
        window.set_margin(Edge::Right, 8);
    }
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    window.add_css_class("switchboard");

    if overview {
        window.set_child(Some(&board(&window)));
    } else {
        let cascade = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        cascade.set_valign(gtk::Align::Start);
        cascade.add_css_class("cascade");
        window.set_child(Some(&cascade));

        let state = Rc::new(RefCell::new(State {
            workspaces: sway::workspaces(),
            icons: Icons::load(),
            mru: sway::mru_order(),
            open: vec![Vec::new()],
        }));
        redraw(&cascade, &state, &window);
    }

    // Clicking anywhere else closes it, the way a menu should. Two things have
    // to be watched, because neither covers the other: losing keyboard focus
    // catches a click into a real window, and the pointer leaving catches a
    // click on the bar, which is a layer surface that never takes focus.
    //
    // The grace period is what stops an overshoot from dismissing the menu --
    // sliding off the edge of one panel on the way to another crosses a few
    // pixels of nothing.
    {
        let win = window.clone();
        window.connect_is_active_notify(move |w| {
            if !w.is_active() {
                win.close();
            }
        });
    }
    {
        let leaving: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        let motion = gtk::EventControllerMotion::new();
        let win = window.clone();
        let pending = leaving.clone();
        motion.connect_leave(move |_| {
            let win = win.clone();
            let id = glib::timeout_add_local_once(LEAVE_GRACE, move || win.close());
            if let Some(old) = pending.borrow_mut().replace(id) {
                old.remove();
            }
        });
        let pending = leaving.clone();
        motion.connect_enter(move |_, _, _| {
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
        });
        window.add_controller(motion);
    }

    // Escape closes. So does picking anything.
    shell::close_on_escape(&window);

    window.present();
}


// --- The board -------------------------------------------------------------
//
// The other view, and the reason there are two. The cascade groups, and
// grouping fights recency: a group sits wherever its most recent workspace
// puts it, so the top level reads as most-recent-project, and the workspace
// you were in before this one can be hidden inside a collapsed group where you
// cannot see it at all. Recency wants no groups.
//
// So this one has none. Every workspace is a card, in the order you left them,
// and the shape is wide and shallow rather than narrow and tall -- a gesture
// puts your eyes in the middle of the screen, and a column you have to read
// down is the wrong thing to find there.

/// Cards to a row.
///
/// Six is as wide as it goes: past that the eye travels further across than a
/// glance covers. Rows are evened out rather than filled and left with a stub,
/// so eight workspaces read as two rows of four, not as six and two.
fn columns(count: usize) -> u32 {
    let rows = (count + 5) / 6;
    let rows = if rows == 0 { 1 } else { rows };
    ((count + rows - 1) / rows).max(1) as u32
}

/// Workspaces in the order they were last focused, most recent first.
///
/// A workspace nobody has been in yet is not in the history at all. Those sort
/// after everything that is, by slot, rather than jumping the queue.
fn by_recency(workspaces: Vec<Workspace>, order: &[i64]) -> Vec<Workspace> {
    let mut out = workspaces;
    out.sort_by_key(|w| {
        let at = order.iter().position(|id| *id == w.id).unwrap_or(usize::MAX);
        (at, w.num)
    });
    out
}

fn board(window: &gtk::ApplicationWindow) -> gtk::Box {
    let icons = Icons::load();
    let workspaces = by_recency(sway::workspaces(), &sway::mru_order());

    let flow = gtk::FlowBox::new();
    flow.add_css_class("grid");
    // Nothing here is selectable in GTK's sense -- a card is a button, and
    // clicking it takes you somewhere rather than marking it.
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_row_spacing(6);
    flow.set_column_spacing(6);
    // Both bounds, so the grid keeps the shape `columns` chose instead of
    // reflowing to whatever width the surface ends up with.
    let cols = columns(workspaces.len());
    flow.set_min_children_per_line(cols);
    flow.set_max_children_per_line(cols);

    for w in &workspaces {
        flow.insert(&card(w, &icons, window), -1);
    }

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 1);
    panel.add_css_class("panel");
    panel.add_css_class("board");
    panel.append(&header(&[], workspaces.len()));
    panel.append(&flow);
    panel.append(&separator());
    panel.append(&new_row(&[], window));
    panel
}

/// One workspace. The slot and the glyph are the pair the bar shows, the name
/// is the last segment of the path -- the same thing the cascade labels a leaf
/// with -- and the project it sits under goes small underneath.
///
/// That way round because the leaf is what tells two workspaces apart. Most of
/// them belong to the same handful of projects, so a card headed by its project
/// would head most cards the same way, and the one word that says which
/// workspace this actually is would be the small print.
fn card(workspace: &Workspace, icons: &Icons, window: &gtk::ApplicationWindow) -> gtk::Button {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 2);

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let slot = gtk::Label::new(Some(&workspace.num.to_string()));
    slot.add_css_class("slot");
    slot.set_width_chars(2);
    slot.set_xalign(1.0);
    top.append(&slot);

    let glyph = gtk::Label::new(Some(icons.get(&workspace.path).unwrap_or(" ")));
    glyph.add_css_class("glyph");
    top.append(&glyph);

    let head = workspace
        .path
        .last()
        .map(|s| tree::titleize(s))
        .unwrap_or_else(|| workspace.num.to_string());
    let name = gtk::Label::new(Some(&head));
    name.add_css_class("name");
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(pango::EllipsizeMode::End);
    top.append(&name);
    outer.append(&top);

    // A workspace at the top level has no project over it, but the line still
    // has to be there: the cards are homogeneous, and a missing line would
    // leave the chips of one card sitting higher than its neighbours'.
    let depth = workspace.path.len().saturating_sub(1);
    let rest: Vec<String> = workspace.path[..depth].iter().map(|s| tree::titleize(s)).collect();
    let trail = gtk::Label::new(Some(&rest.join(" \u{b7} ")));
    trail.add_css_class("path");
    trail.set_xalign(0.0);
    trail.set_ellipsize(pango::EllipsizeMode::End);
    outer.append(&trail);

    if let Some(detail) = chips(&workspace.apps) {
        detail.add_css_class("detail");
        outer.append(&detail);
    }

    let button = gtk::Button::builder().child(&outer).build();
    button.add_css_class("card");
    if workspace.focused {
        button.add_css_class("here");
    }
    if workspace.urgent {
        button.add_css_class("urgent");
    }

    let num = workspace.num;
    let window = window.clone();
    button.connect_clicked(move |_| {
        sway::focus(num);
        window.close();
    });
    button
}

/// Rebuild every panel from `state.open`.
///
/// Panels are cheap -- a dozen rows each -- and rebuilding the whole cascade
/// rather than patching it means the open path is the single source of truth,
/// with no way for a stale panel to survive a level closing.
fn redraw(cascade: &gtk::Box, state: &Rc<RefCell<State>>, window: &gtk::ApplicationWindow) {
    while let Some(child) = cascade.first_child() {
        cascade.remove(&child);
    }

    let open = state.borrow().open.clone();
    for (depth, path) in open.iter().enumerate() {
        let panel = build_panel(path, depth, state, cascade, window);
        // Prepend, so the deepest panel ends up furthest left.
        cascade.prepend(&panel);
    }
}

fn build_panel(
    path: &[String],
    depth: usize,
    state: &Rc<RefCell<State>>,
    cascade: &gtk::Box,
    window: &gtk::ApplicationWindow,
) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 1);
    panel.add_css_class("panel");
    panel.set_valign(gtk::Align::Start);

    let (nodes, tally) = {
        let s = state.borrow();
        let under: Vec<Workspace> = s
            .workspaces
            .iter()
            .filter(|w| w.path.len() >= path.len() && w.path[..path.len()] == *path)
            .cloned()
            .collect();
        let tally = under.len();
        (tree::build(&under, depth), tally)
    };

    panel.append(&header(path, tally));

    for node in &nodes {
        let row = match node {
            Node::Group { key, .. } => {
                let mut child_path = path.to_vec();
                child_path.push(key.clone());
                group_row(node, &child_path, depth, state, cascade, window)
            }
            Node::Leaf(w) => leaf_row(node, w, state, window),
        };
        panel.append(&row);
    }

    panel.append(&separator());
    panel.append(&new_row(path, window));
    panel
}

fn header(path: &[String], tally: usize) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.add_css_class("phead");

    let title = gtk::Label::new(Some(if path.is_empty() {
        "workspaces"
    } else {
        path.last().map(String::as_str).unwrap_or("workspaces")
    }));
    title.set_xalign(0.0);
    bar.append(&title);

    let count = gtk::Label::new(Some(&tally.to_string()));
    count.add_css_class("tally");
    count.set_hexpand(true);
    count.set_xalign(1.0);
    bar.append(&count);
    bar
}

fn separator() -> gtk::Separator {
    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    sep.add_css_class("divider");
    sep
}

/// A row that opens another panel. The dwell timer is armed on enter and
/// disarmed on leave, so crossing a row on the way somewhere else does nothing.
fn group_row(
    node: &Node,
    child_path: &[String],
    depth: usize,
    state: &Rc<RefCell<State>>,
    cascade: &gtk::Box,
    window: &gtk::ApplicationWindow,
) -> gtk::Button {
    let leaves = node.leaves();
    let glyph = {
        let s = state.borrow();
        // A project has no glyph of its own in waybar's icon file, so a group
        // wears the icon of the workspace with the lowest slot in it.
        node.first_leaf()
            .and_then(|w| s.icons.get(&w.path))
            .or_else(|| s.icons.get(child_path))
            .map(str::to_string)
    };

    let button = row_button(
        glyph.as_deref(),
        &node.label(),
        &leaves.len().to_string(),
        "count",
        Some(&pips(&leaves)),
        true,
        node.holds_focus(),
    );
    button.add_css_class("group");
    if state.borrow().open.len() > depth + 1
        && state.borrow().open[depth + 1] == child_path
    {
        button.add_css_class("trail");
    }

    let open_child = {
        let state = state.clone();
        let cascade = cascade.clone();
        let window = window.clone();
        let child_path = child_path.to_vec();
        move || {
            {
                let mut s = state.borrow_mut();
                s.open.truncate(depth + 1);
                s.open.push(child_path.clone());
            }
            redraw(&cascade, &state, &window);
        }
    };

    // Hover with a dwell, click without one -- a deliberate click should never
    // be made to wait.
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let motion = gtk::EventControllerMotion::new();
    {
        let pending = pending.clone();
        let open_child = open_child.clone();
        motion.connect_enter(move |_, _, _| {
            let open_child = open_child.clone();
            let id = glib::timeout_add_local_once(DWELL, move || open_child());
            if let Some(old) = pending.borrow_mut().replace(id) {
                old.remove();
            }
        });
    }
    {
        let pending = pending.clone();
        motion.connect_leave(move |_| {
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
        });
    }
    button.add_controller(motion);

    // Hover opens the group; clicking it goes somewhere. Coming back to a
    // project almost always means coming back to the part of it you left, so a
    // click lands on the most recently focused workspace inside rather than
    // making you pick it out of a list you have already seen.
    let jump = {
        let s = state.borrow();
        node.mru_leaf(&s.mru).map(|w| w.num)
    };
    let window = window.clone();
    button.connect_clicked(move |_| {
        if let Some(num) = jump {
            sway::focus(num);
            window.close();
        }
    });
    button
}

fn leaf_row(
    node: &Node,
    workspace: &Workspace,
    state: &Rc<RefCell<State>>,
    window: &gtk::ApplicationWindow,
) -> gtk::Button {
    let glyph = state.borrow().icons.get(&workspace.path).map(str::to_string);
    let button = row_button(
        glyph.as_deref(),
        &node.label(),
        &workspace.num.to_string(),
        "slot",
        chips(&workspace.apps).as_ref(),
        false,
        workspace.focused,
    );
    if workspace.urgent {
        button.add_css_class("urgent");
    }

    let num = workspace.num;
    let window = window.clone();
    button.connect_clicked(move |_| {
        sway::focus(num);
        window.close();
    });
    button
}

fn new_row(path: &[String], window: &gtk::ApplicationWindow) -> gtk::Button {
    let label = if path.is_empty() {
        "New workspace".to_string()
    } else {
        format!("New in {}", path.join(":"))
    };
    let button = row_button(Some("+"), &label, "", "", None, false, false);
    button.add_css_class("new");

    // Close first, then prompt: fuzzel wants the keyboard this menu is holding.
    let path = path.to_vec();
    let window = window.clone();
    button.connect_clicked(move |_| {
        window.close();
        sway::new_workspace(&path);
    });
    button
}

/// One row, as two lines.
///
/// The icon leads: it is what is recognised first and the only part that is the
/// same shape on every row. The figure at the end is the row's single number --
/// the keyboard slot on a workspace, the size of the group on a group -- sized
/// for two digits and right-aligned so the column lines up down the panel.
///
/// The second line is what is actually in there. `get_tree` knows the programs
/// on every workspace and nothing was showing them, which is the one thing that
/// says which of nine chief workspaces is the langfuse one without reading its
/// name. It is indented to the name, not the icon, so the two lines read as one
/// block.
fn row_button(
    glyph: Option<&str>,
    name: &str,
    figure: &str,
    figure_class: &str,
    detail: Option<&gtk::Box>,
    chevron: bool,
    here: bool,
) -> gtk::Button {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 2);

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    let icon = gtk::Label::new(glyph.or(Some(" ")));
    icon.add_css_class("glyph");
    icon.set_width_chars(2);
    top.append(&icon);

    let label = gtk::Label::new(Some(name));
    label.add_css_class("name");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    top.append(&label);

    if !figure.is_empty() {
        let figure_label = gtk::Label::new(Some(figure));
        figure_label.add_css_class(figure_class);
        figure_label.set_width_chars(2);
        figure_label.set_xalign(1.0);
        top.append(&figure_label);
    }
    if chevron {
        let chev = gtk::Label::new(Some("\u{2039}"));
        chev.add_css_class("chev");
        top.append(&chev);
    }
    outer.append(&top);

    if let Some(detail) = detail {
        detail.add_css_class("detail");
        outer.append(detail);
    }

    let button = gtk::Button::builder().child(&outer).build();
    button.add_css_class("row");
    if here {
        button.add_css_class("here");
    }
    button
}

/// The programs on a workspace, most-numerous first: "chrome x3  foot x2".
/// Three kinds is as many as fits; the rest become a tail count so a workspace
/// with ninety windows in it still says so.
fn chips(apps: &[(String, usize)]) -> Option<gtk::Box> {
    if apps.is_empty() {
        return None;
    }
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("chips");

    for (app, count) in apps.iter().take(3) {
        let text = if *count > 1 {
            format!("{} ×{}", app, count)
        } else {
            app.clone()
        };
        let label = gtk::Label::new(Some(&text));
        label.add_css_class("chip");
        row.append(&label);
    }
    if apps.len() > 3 {
        let rest: usize = apps.iter().skip(3).map(|(_, n)| n).sum();
        let label = gtk::Label::new(Some(&format!("+{rest}")));
        label.add_css_class("chip");
        row.append(&label);
    }
    Some(row)
}

/// One pip per workspace in a group: filled when it holds windows, gold when it
/// is the one you are on. It is as close to a thumbnail as sway will allow --
/// unfocused workspaces are never rendered, so wlr-screencopy has nothing to
/// copy and a preview is not on the table at any price.
fn pips(leaves: &[&Workspace]) -> gtk::Box {
    let strip = gtk::Box::new(gtk::Orientation::Horizontal, 3);
    strip.add_css_class("pips");
    strip.set_valign(gtk::Align::Center);

    for w in leaves.iter().take(12) {
        let pip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        pip.add_css_class("pip");
        if w.focused {
            pip.add_css_class("here");
        } else if w.windows > 0 {
            pip.add_css_class("busy");
        }
        strip.append(&pip);
    }
    strip
}
