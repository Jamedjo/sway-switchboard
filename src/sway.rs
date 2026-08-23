//! Everything this tool knows about sway, and the only place that shells out.
//!
//! It talks to swaymsg rather than opening the IPC socket itself. The socket
//! protocol is not hard, but swaymsg is already on PATH, already handles
//! $SWAYSOCK, and the two calls made here happen once per menu open -- there is
//! no loop to make the cost matter.

use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Workspace {
    /// sway's own id, which survives a rename. sway-ws-mru records focus order
    /// by id for exactly that reason, so this is what matches against it.
    pub id: i64,
    /// The keyboard slot. sway matches `workspace number N` on this and ignores
    /// the label, which is what lets labels be renamed freely.
    pub num: i32,
    /// The raw name, "2:chief:api:v2". Kept whole rather than only as a path
    /// because renaming -- which is how a workspace changes group -- will need
    /// the original back.
    #[allow(dead_code)]
    pub name: String,
    /// The name split on colons with the slot removed: ["chief", "api", "v2"].
    pub path: Vec<String>,
    pub focused: bool,
    pub urgent: bool,
    pub windows: usize,
    /// What is running in there, most-numerous first: [("chrome", 3), ("foot", 2)].
    /// The only thing that says a workspace is the langfuse one without
    /// reading its name.
    pub apps: Vec<(String, usize)>,
}

#[derive(Deserialize)]
struct RawWorkspace {
    id: i64,
    num: i32,
    name: String,
    focused: bool,
    urgent: bool,
}

#[derive(Deserialize)]
struct WindowProperties {
    #[serde(default)]
    class: Option<String>,
}

#[derive(Deserialize)]
struct Node {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "type", default)]
    node_type: Option<String>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    window: Option<i64>,
    #[serde(default)]
    window_properties: Option<WindowProperties>,
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    floating_nodes: Vec<Node>,
}

fn swaymsg(args: &[&str]) -> Option<String> {
    let out = Command::new("swaymsg").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// "2:chief:api:v2" -> ["chief", "api", "v2"]; "sway" -> ["sway"].
///
/// A leading all-digit field is the slot, not part of the path. A workspace
/// that was never given a name at all has its number as its name, and yields an
/// empty path -- it is nameless, and shows as its slot.
fn split_path(name: &str) -> Vec<String> {
    let mut parts: Vec<&str> = name.split(':').collect();
    if parts.first().is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
        parts.remove(0);
    }
    parts
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// What is running on each workspace, walked out of the tree.
///
/// A container counts as a window when it has an app_id or an X11 window id;
/// splits and the workspace nodes themselves do not. Wayland clients are named
/// by app_id, X11 ones by their class, because neither has the other.
fn contents() -> HashMap<String, Vec<String>> {
    let mut found: HashMap<String, Vec<String>> = HashMap::new();
    let Some(raw) = swaymsg(&["-t", "get_tree"]) else {
        return found;
    };
    let Ok(root) = serde_json::from_str::<Node>(&raw) else {
        return found;
    };

    fn walk(node: &Node, workspace: Option<&str>, found: &mut HashMap<String, Vec<String>>) {
        let here = if node.node_type.as_deref() == Some("workspace") {
            let name = node.name.clone().unwrap_or_default();
            found.entry(name.clone()).or_default();
            Some(name)
        } else {
            workspace.map(str::to_string)
        };

        if let Some(ws) = here.as_deref() {
            if node.app_id.is_some() || node.window.is_some() {
                let app = node
                    .app_id
                    .clone()
                    .or_else(|| {
                        node.window_properties
                            .as_ref()
                            .and_then(|p| p.class.clone())
                    })
                    .unwrap_or_else(|| "window".to_string());
                found.entry(ws.to_string()).or_default().push(pretty(&app));
            }
        }

        for child in node.nodes.iter().chain(node.floating_nodes.iter()) {
            walk(child, here.as_deref(), found);
        }
    }

    walk(&root, None, &mut found);
    found
}

/// "org.mozilla.firefox" -> "firefox", "Google-chrome" -> "google-chrome".
///
/// Reverse-DNS app_ids are the common case and their last segment is the only
/// part anyone reads.
fn pretty(app: &str) -> String {
    app.rsplit('.').next().unwrap_or(app).to_lowercase()
}

/// Tally a workspace's windows by program, most-numerous first.
fn tally(apps: Vec<String>) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for app in apps {
        *counts.entry(app).or_insert(0) += 1;
    }
    let mut out: Vec<(String, usize)> = counts.into_iter().collect();
    // Most first, then alphabetical, so the row is stable between openings
    // rather than reshuffling on every hash iteration.
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

pub fn workspaces() -> Vec<Workspace> {
    let Some(raw) = swaymsg(&["-t", "get_workspaces"]) else {
        return Vec::new();
    };
    let Ok(list) = serde_json::from_str::<Vec<RawWorkspace>>(&raw) else {
        return Vec::new();
    };
    let mut found = contents();

    list.into_iter()
        .map(|w| {
            let apps = tally(found.remove(&w.name).unwrap_or_default());
            Workspace {
                id: w.id,
                path: split_path(&w.name),
                windows: apps.iter().map(|(_, n)| n).sum(),
                apps,
                num: w.num,
                name: w.name,
                focused: w.focused,
                urgent: w.urgent,
            }
        })
        .collect()
}

pub fn focus(num: i32) {
    let _ = swaymsg(&["workspace", "number", &num.to_string()]);
}

/// Ask for a name, then make the workspace.
///
/// The prompt is sway-ws-new, a script beside the other sway-ws-* tools, so the
/// naming rules stay in one place: it picks the free slot the same way the rest
/// of them do, prefills fuzzel with the path of whichever panel the row was
/// clicked in, and puts the label through the same cleaning the rename does.
///
/// Spawned and forgotten. fuzzel needs the keyboard, and this menu is holding
/// it until the window closes a moment later.
pub fn new_workspace(under: &[String]) {
    let script = format!(
        "{}/.config/sway/scripts/sway-ws-new",
        std::env::var("HOME").unwrap_or_default()
    );
    let mut cmd = Command::new(script);
    if !under.is_empty() {
        cmd.arg(under.join(":"));
    }
    let _ = cmd.spawn();
}

/// Workspace ids in the order they were last focused, most recent first.
///
/// Written by sway-ws-mru, which subscribes to sway and keeps the last couple
/// of hundred. sway itself keeps no history at all, so without that file there
/// is nothing to sort by -- and a missing file is not an error, it just means
/// nothing has been focused since the session started.
pub fn mru_order() -> Vec<i64> {
    let path = format!(
        "{}/sway-ws-mru",
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())
    );
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}
