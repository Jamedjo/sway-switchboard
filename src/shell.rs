//! Getting a window on screen as a layer-shell overlay, and dressing it.

use gtk::gdk;
use gtk::prelude::*;
use std::path::PathBuf;

/// Load a stylesheet for the whole application.
///
/// `name` picks the file under ~/.config; the compiled-in `fallback` is used
/// when there is none, so the tool still looks like itself on a machine
/// without the dotfiles. Read once at startup, which is enough: both tools are
/// built fresh every time they are opened, so editing the file is the same as
/// reloading it.
pub fn load_css(name: &str, fallback: &str) {
    let user = config_dir().join(name);
    let css = std::fs::read_to_string(&user).unwrap_or_else(|_| fallback.to_string());

    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
}

/// Close on Escape. Both tools are transient things you dismiss.
pub fn close_on_escape(window: &gtk::ApplicationWindow) {
    let keys = gtk::EventControllerKey::new();
    let win = window.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            win.close();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    window.add_controller(keys);
}
