//! sway-cheatsheet: every keybinding you have, read from the config you run.
//!
//! Nothing here is written down twice. The bindings come out of
//! ~/.config/sway/config and its includes, the descriptions come from the
//! comments already above them, and the whole thing is rebuilt each time it
//! opens -- so it cannot fall out of step with the config the way a hand-kept
//! list does.
//!
//! Grouped by mode, because a binding inside `mode "resize"` is not something
//! you can press right now, and a sheet that mixes the two is a sheet that
//! lies.

use gtk::glib;
use gtk::prelude::*;

use sway_switchboard::bindings::Binding;
use sway_switchboard::bindings;
use sway_switchboard::groups::Groups;
use sway_switchboard::shell;

const APP_ID: &str = "dev.jamedjo.sway-cheatsheet";
const DEFAULT_CSS: &str = include_str!("../../cheatsheet.css");

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| shell::load_css("sway-cheatsheet/style.css", DEFAULT_CSS));

    // Same single-instance toggle sway-switchboard uses: a second launch is
    // another activate on the first process, so it closes rather than stacking.
    app.connect_activate(|app| {
        if let Some(open) = app.windows().first() {
            open.close();
        } else {
            build_ui(app);
        }
    });
    app.run()
}

fn build_ui(app: &gtk::Application) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .decorated(false)
        .build();

    // An ordinary window, not a layer-shell overlay. sway-switchboard is a
    // menu -- it appears, you pick, it goes -- and an overlay is right for
    // that. A cheatsheet is a reference: you may want it beside the thing you
    // are doing, moved, resized, or tabbed behind something else, and all of
    // that is what the window tree is for. sway/config floats it and gives it
    // a size.
    window.set_default_size(1400, 900);
    window.set_title(Some("Keyboard"));

    window.add_css_class("cheatsheet");

    // Any key closes it, and so does losing focus.
    //
    // That is the point rather than a hazard: what you open this for is a
    // shortcut, and pressing the shortcut should be the same gesture as
    // putting the sheet away. sway grabs its own bindings before this window
    // sees them, so the keypress often never arrives here at all -- but the
    // focus goes with it, and that closes it just the same.
    //
    // Which leaves moving it as the only thing you can do to it, and that is
    // the whole reason it is a window rather than an overlay.
    // SWAY_CHEATSHEET_PIN leaves it up: with the closers armed the window goes
    // away on the first key pressed anywhere, which is right in use and makes
    // it impossible to photograph or to poke at while working on it.
    let pinned = std::env::var_os("SWAY_CHEATSHEET_PIN").is_some();

    // Only a key-up for a key we saw go down.
    //
    // The window opens while the chord that opened it is still held, so its
    // release arrives here first and closed it again before it had been
    // looked at -- hold Super+? for a moment and it was gone on letting go.
    // A key released without having been pressed in this window's lifetime
    // was down before it existed, and is not the user putting it away.
    let seen: std::rc::Rc<std::cell::RefCell<std::collections::HashSet<u32>>> =
        std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashSet::new()));

    let keys = gtk::EventControllerKey::new();
    {
        let seen = std::rc::Rc::clone(&seen);
        keys.connect_key_pressed(move |_, _, code, _| {
            seen.borrow_mut().insert(code);
            gtk::glib::Propagation::Proceed
        });
    }
    {
        let seen = std::rc::Rc::clone(&seen);
        let win = window.clone();
        keys.connect_key_released(move |_, _, code, _| {
            if seen.borrow_mut().remove(&code) && !pinned {
                win.close();
            }
        });
    }
    window.add_controller(keys);

    // Only once it has been focused. Launched from a shell or from a panel
    // button the window arrives unfocused, and closing on the first
    // not-active would shut it before it was ever on screen.
    let had_focus = std::cell::Cell::new(false);
    let win = window.clone();
    window.connect_is_active_notify(move |w| {
        if w.is_active() {
            had_focus.set(true);
        } else if had_focus.get() && !pinned {
            win.close();
        }
    });

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_width(true)
        .propagate_natural_height(true)
        .build();

    let sheet = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sheet.add_css_class("sheet");
    scroller.set_child(Some(&sheet));

    window.set_child(Some(&scroller));

    let all = bindings::load();
    sheet.append(&header(all.len()));

    // Two columns, because a single column of a hundred and thirty is a
    // scroll, and a sheet you have to scroll is one you stop opening.
    //
    // The split is by count, not by mode: the default mode holds nine tenths
    // of the bindings, so splitting on mode boundaries put everything in one
    // column and two stubs in the other. A mode that straddles the break gets
    // its heading again at the top of the second column.
    // Columns are laid out by hand rather than by a flow box. A flow box
    // gives every child in a line the height of the tallest, which with
    // groups of four rows and groups of twelve left craters; and turning that
    // off leaves the columns ragged for the same reason. So: whole groups
    // dealt into N lanes, N chosen from the width, redealt when the width
    // changes.
    let columns = gtk::Box::new(gtk::Orientation::Horizontal, 26);
    columns.add_css_class("columns");
    sheet.append(&columns);

    // Owned rather than borrowed: the closure that re-deals them outlives the
    // function that read them.
    let taxonomy = Groups::load();
    let mut groups: Vec<(String, Vec<Binding>)> = Vec::new();
    for group in taxonomy.order() {
        let rows: Vec<Binding> = all
            .iter()
            .filter(|b| taxonomy.category(&b.command, &b.description) == group)
            .cloned()
            .collect();
        if !rows.is_empty() {
            groups.push((group.clone(), rows));
        }
    }

    let lanes = std::cell::Cell::new(0usize);
    let relayout = std::rc::Rc::new({
        let columns = columns.clone();
        move |wanted: usize| {
            if lanes.get() == wanted {
                return;
            }
            lanes.set(wanted);
            while let Some(child) = columns.first_child() {
                columns.remove(&child);
            }

            // Deal the groups out so every lane ends up with about the same
            // number of rows -- always to whichever lane is currently
            // shortest, which keeps a tall group from stranding a lane.
            let mut boxes: Vec<(gtk::Box, usize)> = Vec::new();
            for _ in 0..wanted {
                let lane = gtk::Box::new(gtk::Orientation::Vertical, 0);
                lane.set_hexpand(true);
                lane.set_valign(gtk::Align::Start);
                columns.append(&lane);
                boxes.push((lane, 0));
            }
            for (name, rows) in &groups {
                let (lane, count) = boxes
                    .iter_mut()
                    .min_by_key(|(_, count)| *count)
                    .expect("at least one lane");
                let group_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
                group_box.add_css_class("group");
                group_box.append(&heading(name));
                for binding in rows {
                    group_box.append(&row(binding));
                }
                lane.append(&group_box);
                *count += rows.len() + 2; // the heading costs a row and a gap
            }
        }
    });

    // About 440px a lane: narrower and a description wraps to three lines.
    let lanes_for = |width: i32| ((width / 440).clamp(1, 4)) as usize;
    relayout(lanes_for(window.default_width()));
    {
        let relayout = std::rc::Rc::clone(&relayout);
        window.connect_default_width_notify(move |w| relayout(lanes_for(w.default_width())));
    }

    window.present();
}

fn header(count: usize) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("sheet-head");

    let title = gtk::Label::new(Some("Keyboard"));
    title.add_css_class("sheet-title");
    title.set_xalign(0.0);
    row.append(&title);

    let tally = gtk::Label::new(Some(&format!("{count} bindings")));
    tally.add_css_class("sheet-tally");
    tally.set_hexpand(true);
    tally.set_xalign(1.0);
    row.append(&tally);
    row
}

fn heading(group: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(group));
    label.add_css_class("section-head");
    label.set_xalign(0.0);
    label
}

fn row(binding: &Binding) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("binding");

    let keys = gtk::Label::new(Some(&binding.keys));
    keys.add_css_class("keys");
    keys.set_xalign(0.0);
    keys.set_width_chars(17);
    row.append(&keys);

    let what = gtk::Label::new(Some(&binding.description));
    what.add_css_class("what");
    what.set_xalign(0.0);
    what.set_hexpand(true);
    what.set_wrap(true);
    what.set_max_width_chars(34);
    row.append(&what);
    row
}
