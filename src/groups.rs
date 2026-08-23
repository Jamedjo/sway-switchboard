//! How bindings are sorted into groups on the sheet, and where that is decided.
//!
//! Not here, is the short answer. The default below covers what sway's own
//! example configuration binds -- focus, move, resize, the layouts, the
//! scratchpad, workspaces, reload and exit -- and nothing else, because
//! anything else is somebody's particular setup. A `groups.toml` beside the
//! stylesheet replaces it wholesale, which is where a taxonomy full of one
//! person's script names belongs.
//!
//! Groups are tried in the order they are written, most specific first,
//! because the words overlap: "move container to workspace" is about
//! workspaces though it moves a window, and "move focus to the parent
//! container" is about neither. That is a matching order, not a reading one,
//! so `order` may list the groups again in the order they should be printed.

use serde::Deserialize;

const DEFAULT: &str = include_str!("../groups.toml");

#[derive(Deserialize)]
pub struct Groups {
    /// Where a binding goes when nothing matches.
    #[serde(default = "other")]
    fallback: String,
    #[serde(default, rename = "group")]
    groups: Vec<Group>,
    /// How the groups should be printed, if that is not the order they are
    /// matched in. Anything left out follows in matching order.
    #[serde(default)]
    order: Vec<String>,
}

#[derive(Deserialize)]
struct Group {
    name: String,
    /// Matched against the command and its description, lower-cased.
    #[serde(default)]
    keywords: Vec<String>,
    /// Matched against the start of the command. `exec` catches everything
    /// that launches something without listing every program by name.
    #[serde(default)]
    command_starts_with: Option<String>,
}

fn other() -> String {
    "Other".to_string()
}

impl Groups {
    pub fn load() -> Groups {
        let path = crate::shell::config_dir().join("sway-cheatsheet/groups.toml");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| DEFAULT.to_string());
        toml::from_str(&text).unwrap_or_else(|e| {
            // A broken file is worth saying out loud rather than silently
            // falling back to a taxonomy the reader did not ask for.
            eprintln!("sway-cheatsheet: {}: {e}", path.display());
            toml::from_str(DEFAULT).expect("the compiled-in groups parse")
        })
    }

    pub fn category(&self, command: &str, description: &str) -> String {
        let text = format!("{command} {description}").to_lowercase();
        for group in &self.groups {
            if group.keywords.iter().any(|word| text.contains(word)) {
                return group.name.clone();
            }
            if let Some(prefix) = &group.command_starts_with {
                if command.starts_with(prefix.as_str()) {
                    return group.name.clone();
                }
            }
        }
        self.fallback.clone()
    }

    /// The groups in the order they should be printed, the fallback last.
    pub fn order(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for name in self.order.iter().chain(self.groups.iter().map(|g| &g.name)) {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        if !out.contains(&self.fallback) {
            out.push(self.fallback.clone());
        }
        out
    }
}
