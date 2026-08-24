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

    // The sheet is the window's child and the scroller is inside it, not the
    // other way round: the header says what this is and the footer band
    // carries the sheet's bottom corners, and neither of those is something
    // to scroll away. Only the lanes move if a very long config outgrows the
    // height sway floats this at.
    let sheet = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sheet.add_css_class("sheet");
    window.set_child(Some(&sheet));

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_width(true)
        .propagate_natural_height(true)
        .vexpand(true)
        .build();

    let all = bindings::load();
    sheet.append(&header(all.len()));

    // Columns, because a single column of a hundred and thirty is a scroll,
    // and a sheet you have to scroll is one you stop opening.
    //
    // Laid out by hand rather than by a flow box. A flow box gives every
    // child in a line the height of the tallest, which with groups of four
    // rows and groups of twelve left craters; and turning that off leaves the
    // columns ragged for the same reason. So: whole groups dealt into N
    // lanes, N chosen from the width, redealt when the width changes.
    let columns = gtk::Box::new(gtk::Orientation::Horizontal, 26);
    columns.add_css_class("columns");
    scroller.set_child(Some(&columns));
    sheet.append(&scroller);

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
                // A row is two lines deep now, and a heading with the air
                // above its group comes to about one of those -- so a group
                // weighs its rows plus one. Counting the heading as two rows,
                // as this did when a row was a single line, made a lane of
                // many small groups look full while it was not.
                *count += rows.len() + 1;
            }
        }
    });

    // About 340px a lane. A row is now two lines deep -- what it does, then
    // the chord under it -- so a lane no longer has to be wide enough for
    // both side by side, and four of them at the size sway floats this at is
    // what fills the sheet top to bottom.
    let lanes_for = |width: i32| ((width / 340).clamp(1, 4)) as usize;
    relayout(lanes_for(window.default_width()));
    {
        let relayout = std::rc::Rc::clone(&relayout);
        window.connect_default_width_notify(move |w| relayout(lanes_for(w.default_width())));
    }

    sheet.append(&footer());

    window.present();
}

fn header(count: usize) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("sheet-head");
    row.set_baseline_position(gtk::BaselinePosition::Bottom);

    let title = gtk::Label::new(Some("Keyboard"));
    title.add_css_class("sheet-title");
    title.set_xalign(0.0);
    // Both the title and the hint sit on one baseline. A box only aligns
    // baselines if its children ask to be aligned that way, so setting it on
    // the hint alone left the hint floating at the top of a 34px title.
    title.set_valign(gtk::Align::Baseline);
    row.append(&title);

    // The one thing a first-time reader cannot guess: there is nothing to
    // dismiss first, because whatever you press does its own job on the way
    // out.
    let hint = gtk::Label::new(Some("any key runs its binding and closes the sheet"));
    hint.add_css_class("sheet-hint");
    hint.set_xalign(0.0);
    hint.set_hexpand(true);
    hint.set_valign(gtk::Align::Baseline);
    row.append(&hint);

    // Natural width, not stretched: the tally wears the same capsule the caps
    // do, and a filled label would pull that pill the width of the header.
    let tally = gtk::Label::new(Some(&format!("{count} bindings")));
    tally.add_css_class("sheet-tally");
    tally.set_xalign(1.0);
    tally.set_valign(gtk::Align::Center);
    row.append(&tally);
    row
}

/// The band along the bottom, carrying the one thing worth saying twice.
///
/// Which is how to get back here: printed as the caps themselves rather than
/// as prose, so it is read the way the rows above it are read.
fn footer() -> gtk::Box {
    let foot = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    foot.add_css_class("sheet-foot");

    // Where the sheet comes from, which is the whole argument for it: nothing
    // here was written down twice. It leads, quietly, from the left.
    let from = gtk::Label::new(Some("read out of ~/.config/sway/config every time it opens"));
    from.set_hexpand(true);
    from.set_xalign(0.0);
    from.set_valign(gtk::Align::Center);
    foot.append(&from);

    // The one thing worth saying twice goes at the right, as real caps.
    let caps = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    caps.add_css_class("keys");
    caps.set_valign(gtk::Align::Center);
    caps.append(&cap("Super", &["mod", "mod-super"]));
    caps.append(&cap("?", &["key"]));
    foot.append(&caps);

    let opens = gtk::Label::new(Some("opens this sheet"));
    opens.set_xalign(0.0);
    opens.set_valign(gtk::Align::Center);
    foot.append(&opens);
    foot
}

fn heading(group: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(group));
    label.add_css_class("section-head");
    label.set_xalign(0.0);
    label
}

/// What it does, and under it the chord that does it.
///
/// Stacked rather than side by side. The chord column was a column of ragged
/// text at a fixed width -- wide enough for Super+Alt+Ctrl+1…0 and therefore
/// mostly empty -- and putting the caps on their own line under the sentence
/// they belong to buys the width back for the sentence and lets the keys be
/// keys.
fn row(binding: &Binding) -> gtk::Box {
    // No spacing between the two lines: the gap is .keys' top margin, so the
    // stylesheet owns it like every other measurement here.
    let row = gtk::Box::new(gtk::Orientation::Vertical, 0);
    row.add_css_class("binding");

    let what = gtk::Label::new(Some(&binding.description));
    what.add_css_class("what");
    what.set_xalign(0.0);
    what.set_hexpand(true);
    what.set_wrap(true);
    what.set_max_width_chars(30);
    row.append(&what);

    row.append(&chord(&binding.keys));
    row
}

/// A chord as one label per key.
///
/// "Super+Shift+d" is three caps, not a string: every part before the last is
/// a modifier and tinted by which one it is, and the last -- the key that
/// actually does the thing -- is the one that stands out. A collapsed run
/// keeps its "/" or "…" as a separator between two caps rather than inside
/// one, since it is punctuation about the keys and not a key.
fn chord(keys: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("keys");
    row.set_halign(gtk::Align::Start);

    let parts: Vec<&str> = keys.split('+').collect();
    let last = parts.len().saturating_sub(1);
    for (i, part) in parts.iter().enumerate() {
        if i < last {
            match modifier(part) {
                Some(which) => row.append(&cap(part, &["mod", which])),
                None => row.append(&cap(part, &["mod"])),
            }
        } else {
            for (text, class) in alternatives(part) {
                row.append(&cap(&text, &[class]));
            }
        }
    }
    row
}

/// Which modifier a chord segment names, for the tint it gets.
fn modifier(part: &str) -> Option<&'static str> {
    match part.to_ascii_lowercase().as_str() {
        "super" | "mod4" => Some("mod-super"),
        "ctrl" | "control" => Some("mod-ctrl"),
        "alt" | "mod1" | "mod2" | "mod3" | "mod5" => Some("mod-alt"),
        "shift" => Some("mod-shift"),
        _ => None,
    }
}

/// The last segment split into the keys it offers and the marks between them.
///
/// "b/v" is two keys and a separator; "1…0" is the same shape. A bare "/" or
/// "…" is the key itself -- Super+/ is a binding here -- so a mark only
/// separates when there is a key on each side of it.
fn alternatives(tail: &str) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    let mut current = String::new();
    for c in tail.chars() {
        if (c == '/' || c == '\u{2026}') && !current.is_empty() {
            out.push((std::mem::take(&mut current), "key"));
            out.push((c.to_string(), "sep"));
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        out.push((current, "key"));
    } else if let Some((text, class)) = out.pop() {
        // A mark with nothing after it was never a separator. Fold it back
        // into the key it trails.
        match out.pop() {
            Some((key, _)) if class == "sep" => out.push((format!("{key}{text}"), "key")),
            Some(previous) => {
                out.push(previous);
                out.push((text, class));
            }
            None => out.push((text, "key")),
        }
    }
    out
}

fn cap(text: &str, classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    for class in classes {
        label.add_css_class(class);
    }
    label
}
