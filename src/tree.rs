//! Workspace names, turned into a tree of any depth.
//!
//! The grouping is already in the name: "2:chief:api:v2" is slot, then the path
//! chief -> api -> v2. Nothing here assumes two levels, and nothing invents an
//! "Elsewhere" -- a workspace whose path is one segment long is a leaf at the
//! top, sitting beside the groups rather than being herded under a heading.
//!
//! One case is worth naming: a path can be both a group and a workspace. If
//! "chief:api" exists as a workspace and "chief:api:v2" exists too, then `api`
//! is a group whose children include a leaf for the workspace itself.

use crate::sway::Workspace;
use std::collections::BTreeMap;

pub enum Node {
    Group { label: String, key: String, children: Vec<Node> },
    Leaf(Workspace),
}

impl Node {
    /// How the row reads. Groups are title-cased the same way the bar and the
    /// fuzzel picker case them.
    pub fn label(&self) -> String {
        match self {
            Node::Group { label, .. } => label.clone(),
            Node::Leaf(w) => match w.path.last() {
                Some(last) => titleize(last),
                None => w.num.to_string(),
            },
        }
    }

    /// Workspaces at or under this node, for counts and pips.
    pub fn leaves(&self) -> Vec<&Workspace> {
        match self {
            Node::Leaf(w) => vec![w],
            Node::Group { children, .. } => children.iter().flat_map(Node::leaves).collect(),
        }
    }

    /// The lowest-slot workspace under this node. A group wears its icon: the
    /// project as a whole has no glyph of its own in waybar's icon file, and
    /// the first workspace in it is the one already associated with the name.
    pub fn first_leaf(&self) -> Option<&Workspace> {
        self.leaves().into_iter().min_by_key(|w| w.num)
    }

    /// The workspace in this group you were on most recently.
    ///
    /// A hard click on a group goes here rather than opening the group, which
    /// is what you want nine times in ten: coming back to a project means
    /// coming back to whichever part of it you left. Falls back to the lowest
    /// slot when none of them are in the history yet.
    pub fn mru_leaf(&self, order: &[i64]) -> Option<&Workspace> {
        let leaves = self.leaves();
        leaves
            .iter()
            .filter_map(|w| order.iter().position(|id| *id == w.id).map(|at| (at, *w)))
            .min_by_key(|(at, _)| *at)
            .map(|(_, w)| w)
            .or_else(|| leaves.into_iter().min_by_key(|w| w.num))
    }

    pub fn holds_focus(&self) -> bool {
        self.leaves().iter().any(|w| w.focused)
    }
}

/// Group a slice of workspaces by their path at `depth`.
///
/// Order is by lowest slot, not alphabetical: the hot workspaces are 2-5 and a
/// menu that puts them first is a menu that needs reading less often.
pub fn build(workspaces: &[Workspace], depth: usize) -> Vec<Node> {
    let mut buckets: BTreeMap<String, Vec<Workspace>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();

    for w in workspaces {
        let key = w.path.get(depth).cloned().unwrap_or_default();
        if !buckets.contains_key(&key) {
            order.push(key.clone());
        }
        buckets.entry(key).or_default().push(w.clone());
    }

    let mut nodes: Vec<Node> = Vec::new();

    for key in order {
        let group = buckets.remove(&key).unwrap_or_default();

        // A workspace with nothing after this segment ends here.
        let ends_here = group.iter().filter(|w| w.path.len() == depth + 1).count();
        let goes_deeper = group.len() - ends_here;

        if key.is_empty() || goes_deeper == 0 {
            // Every workspace in this bucket is a leaf at this level. The
            // empty key is the nameless case -- a workspace called only by its
            // number -- and those are always leaves.
            nodes.extend(group.into_iter().map(Node::Leaf));
        } else {
            nodes.push(Node::Group {
                label: titleize(&key),
                key: key.clone(),
                children: build(&group, depth + 1),
            });
        }
    }

    nodes.sort_by_key(|n| n.leaves().iter().map(|w| w.num).min().unwrap_or(i32::MAX));
    nodes
}

/// Words that read as shouted initialisms, not names: "API V2 Planner" rather
/// than "Api V2 Planner". The same list sway-ws-lib keeps in WS_ACRONYMS; if
/// one gains a word, so should the other.
const ACRONYMS: &[&str] = &[
    "ai", "api", "cd", "ci", "cli", "cpu", "css", "csv", "db", "dns", "gpu", "html", "http",
    "https", "id", "ios", "json", "llm", "ml", "os", "pdf", "pr", "rss", "sql", "ssh", "tv",
    "ui", "url", "ux", "vpn",
];

/// Title-case, dashes and underscores as word breaks.
pub fn titleize(s: &str) -> String {
    s.split(|c| c == '-' || c == '_' || c == ' ')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let lower = w.to_lowercase();
            if ACRONYMS.contains(&lower.as_str()) {
                w.to_uppercase()
            } else {
                let mut chars = w.chars();
                match chars.next() {
                    Some(first) if first.is_ascii_lowercase() => {
                        first.to_ascii_uppercase().to_string() + chars.as_str()
                    }
                    // Leave anything not starting with an ASCII lower-case
                    // letter alone, so a legacy icon label is not sliced in
                    // half mid-glyph.
                    _ => w.to_string(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
