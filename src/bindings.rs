//! sway's keybindings, read out of the config it is actually running.
//!
//! A cheatsheet written by hand is wrong the day after you change a binding.
//! This one parses `bindsym` lines out of ~/.config/sway/config and whatever
//! it includes, so it cannot drift: if the binding is not in the config, it is
//! not on the sheet.
//!
//! Three things make the parse more than a grep. Variables have to be resolved
//! or every row reads "$mod+..." instead of "Super+...". Mode blocks have to be
//! tracked, because a binding inside `mode "resize"` is not a binding you can
//! press right now. And the comment above a binding is worth more than the
//! command it runs -- "Toggle night light" beats
//! "exec /home/james/.config/sway/scripts/sway-nightlight" -- so the comment
//! block immediately above each line is taken as its description.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Binding {
    /// The mode it belongs to; "default" for the ones always live.
    pub mode: String,
    /// The chord, with variables resolved: "Super+Shift+d".
    pub keys: String,
    /// What it does, from the comment above it where there is one.
    pub description: String,
    /// The command itself, kept for deciding what to print rather than for
    /// printing: a media key is judged by what it runs, not by whatever
    /// comment happens to sit above it.
    pub command: String,
}

pub fn load() -> Vec<Binding> {
    let root = crate::shell::config_dir().join("sway/config");
    let mut vars = HashMap::new();
    let mut out = Vec::new();
    parse_file(&root, &mut vars, &mut out);
    out.retain(worth_printing);
    dedupe(collapse(out))
}

/// Whether a binding earns a line on the sheet.
///
/// Two kinds do not. A binding inside a mode is unreachable until you have
/// entered that mode, and the mode itself has a prompt of its own that sway
/// puts on screen -- printing them here is a list of things you cannot press.
///
/// And a media key already says what it does: the picture on the key is the
/// documentation. Only when one has been bound to something *other* than what
/// it depicts is it worth a line, which is why this checks the description
/// rather than just dropping every XF86.
fn worth_printing(binding: &Binding) -> bool {
    binding.mode == "default" && !does_what_it_says(&binding.keys, &binding.command)
}

fn does_what_it_says(keys: &str, command: &str) -> bool {
    let Some(name) = keys.strip_prefix("XF86") else {
        return false;
    };
    let does = command.to_lowercase();

    // The word on the key, pulled out of its CamelCase name.
    name.chars()
        .fold(Vec::new(), |mut words: Vec<String>, c| {
            if c.is_uppercase() || words.is_empty() {
                words.push(String::new());
            }
            words.last_mut().unwrap().push(c.to_ascii_lowercase());
            words
        })
        .iter()
        .filter(|word| {
            matches!(
                word.as_str(),
                "volume" | "mute" | "play" | "pause" | "next" | "prev" | "brightness"
            )
        })
        .any(|word| does.contains(word.as_str()))
}

/// Fold a run of near-identical bindings into one row.
///
/// Ten lines reading "workspace number 1", "workspace number 2" and so on say
/// one thing ten times and push everything else off the sheet. A run collapses
/// when the chords differ only in their last segment and the descriptions are
/// the same once the numbers and the quoted names are taken out -- so
/// Super+1 through Super+0 becomes a single "Super+1…0".
///
/// Three is the floor. Two lines are not a pattern, and saying them separately
/// costs a line rather than a paragraph.
fn collapse(bindings: Vec<Binding>) -> Vec<Binding> {
    let mut out: Vec<Binding> = Vec::new();
    let mut run: Vec<Binding> = Vec::new();

    fn flush(run: &mut Vec<Binding>, out: &mut Vec<Binding>) {
        if run.len() >= 2 {
            let first = run.first().unwrap().clone();
            let last = run.last().unwrap();
            let (head, from) = split_chord(&first.keys);
            let (_, to) = split_chord(&last.keys);

            // Arrows are shown as the set rather than as a range: nobody reads
            // "Super+left-arrow to right-arrow" and thinks of four keys, but
            // Super+arrows is obvious at a glance.
            let tails: Vec<&str> = run.iter().map(|b| split_chord(&b.keys).1).collect();
            let arrows = tails.iter().all(|t| ARROWS.contains(t));
            out.push(Binding {
                command: first.command.clone(),
                mode: first.mode,
                keys: if arrows {
                    format!("{head}+{}", ARROWS.join(""))
                } else if tails.len() <= 3 {
                    // Two or three keys are worth naming; an ellipsis between
                    // them reads as a range that is not one.
                    format!("{head}+{}", tails.join("/"))
                } else if head.is_empty() {
                    format!("{from}…{to}")
                } else {
                    format!("{head}+{from}…{to}")
                },
                // A comment on the first line of a run describes the run --
                // "Switch to workspace 1-10" sits above $mod+1 and means all
                // ten. Only when there is no comment does the command's shape
                // stand in for one.
                description: if first.description == describe(&first.command) {
                    describe(&shape(&first.command))
                } else {
                    first.description.clone()
                },
            });
        } else {
            out.append(run);
        }
        run.clear();
    }

    for binding in bindings {
        let joins = run.last().is_some_and(|prev: &Binding| {
            prev.mode == binding.mode
                && split_chord(&prev.keys).0 == split_chord(&binding.keys).0
                && shape(&prev.command) == shape(&binding.command)
                && shape(&binding.command) != binding.command.trim()
        });
        if !joins {
            flush(&mut run, &mut out);
        }
        run.push(binding);
    }
    flush(&mut run, &mut out);
    out
}

/// Left, down, up, right, in the order sway's own config lists them.
const ARROWS: &[&str] = &["\u{2190}", "\u{2193}", "\u{2191}", "\u{2192}"];

/// "Super+Shift+3" -> ("Super+Shift", "3").
fn split_chord(keys: &str) -> (&str, &str) {
    match keys.rsplit_once('+') {
        Some((head, tail)) => (head, tail),
        None => ("", keys),
    }
}

/// What a description says once the particulars are removed: the number it
/// refers to, and the workspace name in quotes after it. Two descriptions with
/// the same shape are the same sentence about different things.
fn shape(description: &str) -> String {
    let without_quotes = match (description.find('"'), description.rfind('"')) {
        (Some(open), Some(close)) if close > open => {
            format!("{}{}", &description[..open], &description[close + 1..])
        }
        _ => description.to_string(),
    };
    without_quotes
        .split_whitespace()
        .filter(|word| !word.chars().all(|c| c.is_ascii_digit()))
        .filter(|word| !matches!(*word, "left" | "right" | "up" | "down"))
        // splith and splitv are one thing done two ways, and so are resize
        // shrink and resize grow. Taking the difference out of the shape lets
        // each pair share a row without letting "layout tabbed" and "layout
        // stacking" share one, which are two different things.
        .filter(|word| !matches!(*word, "shrink" | "grow"))
        .map(|word| if word == "splith" || word == "splitv" { "split" } else { word })
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn parse_file(path: &Path, vars: &mut HashMap<String, String>, out: &mut Vec<Binding>) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };

    let mut mode = "default".to_string();
    let mut comment: Vec<String> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();

        if line.is_empty() {
            comment.clear();
            continue;
        }

        // A commented-out binding is not a binding, but it is also not a
        // description of the next one.
        if let Some(rest) = line.strip_prefix('#') {
            let rest = rest.trim();
            if rest.starts_with("bindsym") || rest.starts_with("exec") || rest.starts_with('#') {
                // A commented-out binding is not a description, and a `##`
                // banner describes a section rather than the next line.
                comment.clear();
            } else if !rest.is_empty() {
                // Only the last line: a paragraph above a binding is about
                // the group it opens, and the line touching it is its own.
                comment.clear();
                comment.push(rest.to_string());
            }
            continue;
        }

        // sway's parser is case-insensitive and the config uses both `set`
        // and `Set`. Matching only the lower-case one left $mode_system
        // undefined, and $mod was then substituted inside its name.
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("set ") {
            let rest = &line[4..];
            let mut parts = rest.splitn(2, char::is_whitespace);
            if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                let value = expand(value.trim(), vars);
                vars.insert(name.trim_start_matches('$').to_string(), value);
            }
            comment.clear();
            continue;
        }

        if let Some(rest) = line.strip_prefix("include ") {
            let pattern = expand(rest.trim(), vars);
            for file in glob_include(path, &pattern) {
                parse_file(&file, vars, out);
            }
            comment.clear();
            continue;
        }

        if let Some(rest) = line.strip_prefix("mode ") {
            // `mode "resize" {` quotes it; `mode $mode_system {` does not, and
            // the variable holds the quotes itself. Expand first, then take
            // what is inside them, so both shapes land in the same place.
            let expanded = expand(rest.trim(), vars);
            let name = expanded
                .split('"')
                .nth(1)
                .unwrap_or_else(|| expanded.split_whitespace().next().unwrap_or(""));
            if !name.is_empty() && name != "{" {
                mode = name.to_string();
            }
            comment.clear();
            continue;
        }
        if line == "}" {
            mode = "default".to_string();
            comment.clear();
            continue;
        }

        if let Some(rest) = line.strip_prefix("bindsym ") {
            if let Some(binding) = parse_bindsym(rest, &mode, &comment, vars) {
                out.push(binding);
            }
            comment.clear();
            continue;
        }

        comment.clear();
    }
}

fn parse_bindsym(
    rest: &str,
    mode: &str,
    comment: &[String],
    vars: &HashMap<String, String>,
) -> Option<Binding> {
    // Skip the --flags sway allows before the chord.
    let mut words = rest.split_whitespace().peekable();
    while let Some(word) = words.peek() {
        if word.starts_with("--") {
            words.next();
        } else {
            break;
        }
    }

    let keys = expand(words.next()?, vars);
    let command: Vec<&str> = words.collect();
    if command.is_empty() {
        return None;
    }

    let description = if comment.is_empty() {
        describe(&expand(&command.join(" "), vars))
    } else {
        comment.join(" ")
    };
    let description = shorten(&description);

    Some(Binding {
        mode: mode.to_string(),
        keys: pretty_keys(&keys),
        description,
        command: expand(&command.join(" "), vars),
    })
}

/// Fall back to the command when there is no comment, tidied a little: the
/// full path to a script says less than the name of it, and the `mode
/// "default"` half of every mode binding is bookkeeping rather than meaning.
fn describe(command: &str) -> String {
    let command = command
        .split(", mode ")
        .next()
        .unwrap_or(command)
        .trim_end_matches(|c: char| c == ';' || c == ',')
        .trim();
    let trimmed = command.strip_prefix("exec ").unwrap_or(command).trim();
    match trimmed.split_whitespace().next() {
        Some(first) if first.contains('/') => {
            let name = first.rsplit('/').next().unwrap_or(first);
            let rest = trimmed[first.len()..].trim();
            if rest.is_empty() {
                name.to_string()
            } else {
                format!("{name} {rest}")
            }
        }
        _ => trimmed.to_string(),
    }
}

/// A description is a line on a sheet, not a transcript.
///
/// Some bindings are a whole pipeline written inline -- the window switcher
/// is a swaymsg, a jq, a fuzzel and two awks -- and printing it whole pushes
/// everything else off the page while telling you nothing you could act on.
/// Past this length only the first few words survive.
fn shorten(description: &str) -> String {
    const LIMIT: usize = 96;
    let clean = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() <= LIMIT {
        return clean;
    }
    let mut out = String::new();
    for word in clean.split_whitespace() {
        if out.chars().count() + word.chars().count() + 1 > LIMIT {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out.push('\u{2026}');
    out
}

/// "$mod+Shift+d" -> "Super+Shift+d", and the names nobody says out loud.
fn pretty_keys(keys: &str) -> String {
    keys.split('+')
        .map(|part| match part {
            "Mod4" => "Super",
            "Mod1" => "Alt",
            "Left" => "\u{2190}",
            "Down" => "\u{2193}",
            "Up" => "\u{2191}",
            "Right" => "\u{2192}",
            "grave" => "`",
            "backslash" => "\\",
            "bracketleft" => "[",
            "bracketright" => "]",
            "semicolon" => ";",
            "apostrophe" => "'",
            "comma" => ",",
            "period" => ".",
            "slash" => "/",
            "question" => "?",
            "minus" => "-",
            "equal" => "=",
            "Print" => "PrtSc",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn expand(value: &str, vars: &HashMap<String, String>) -> String {
    let mut out = value.to_string();
    // Longest first, so $mod does not eat the front of $mode_system.
    let mut names: Vec<&String> = vars.keys().collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for name in names {
        if let Some(v) = vars.get(name) {
            out = out.replace(&format!("${name}"), v);
        }
    }
    out
}

/// sway's include takes a shell glob. Only the one shape is used here --
/// a directory of fragments -- so this expands that rather than pulling in a
/// glob crate for it.
fn glob_include(from: &Path, pattern: &str) -> Vec<PathBuf> {
    let base = from.parent().unwrap_or(Path::new("."));
    let full = if pattern.starts_with('/') {
        PathBuf::from(pattern)
    } else {
        base.join(pattern)
    };

    let Some(name) = full.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };
    if !name.contains('*') {
        return vec![full];
    }

    let dir = full.parent().unwrap_or(base).to_path_buf();
    let (prefix, suffix) = name.split_once('*').unwrap_or((name, ""));
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && n.ends_with(suffix))
        })
        .collect();
    files.sort();
    files
}


/// One line per thing, not one per way of reaching it.
///
/// Help is bound to ?, to / and to Shift+/ so that it opens however you reach
/// for the key -- but three rows saying the same thing is three rows of noise.
/// The first spelling in the config wins, which is why the one worth printing
/// is written first there.
fn dedupe(bindings: Vec<Binding>) -> Vec<Binding> {
    let mut seen: Vec<String> = Vec::new();
    bindings
        .into_iter()
        .filter(|b| {
            let key = format!("{}|{}", b.mode, b.command);
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        })
        .collect()
}
