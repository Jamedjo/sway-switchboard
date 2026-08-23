//! The glyph on a row, taken from the bar rather than from a second list.
//!
//! waybar's workspace-icons.json is written by sway-ws-icon and keyed on the
//! whole label after the slot ("chief:api:v2"), so a menu row can wear the same
//! glyph the bar does with no new configuration. Entries there are a glyph
//! followed by pango markup; only what precedes the first space or "<" is the
//! glyph itself.

use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub struct Icons(HashMap<String, String>);

impl Icons {
    pub fn load() -> Self {
        let mut map = HashMap::new();
        let path: PathBuf = dirs_config().join("waybar/workspace-icons.json");

        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(Value::Object(root)) = serde_json::from_str::<Value>(&text) {
                if let Some(Value::Object(icons)) = root
                    .get("sway/workspaces")
                    .and_then(|v| v.get("format-icons"))
                    .map(|v| v.clone())
                    .as_ref()
                {
                    for (key, value) in icons {
                        if let Some(text) = value.as_str() {
                            if let Some(glyph) = first_glyph(text) {
                                map.insert(key.clone(), glyph);
                            }
                        }
                    }
                }
            }
        }
        Icons(map)
    }

    /// Look up the full path first, then walk back up it -- so a group can wear
    /// its own icon if one exists, and a workspace falls back to its project's.
    pub fn get(&self, path: &[String]) -> Option<&str> {
        for end in (1..=path.len()).rev() {
            if let Some(glyph) = self.0.get(&path[..end].join(":")) {
                return Some(glyph);
            }
        }
        None
    }
}

fn first_glyph(entry: &str) -> Option<String> {
    let glyph: String = entry
        .trim_start()
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '<')
        .collect();
    // A value that is markup from the start is the "default" branch of waybar's
    // format, not an icon.
    if glyph.is_empty() { None } else { Some(glyph) }
}

fn dirs_config() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
}
