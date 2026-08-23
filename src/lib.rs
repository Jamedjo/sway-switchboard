//! Shared innards for the two tools in here.
//!
//! sway-switchboard is a workspace menu; sway-cheatsheet is a keybinding
//! reference. They have almost nothing in common except how they get on
//! screen -- a layer-shell overlay wearing a stylesheet from the dotfiles --
//! so that part lives here and the rest does not.

pub mod bindings;
pub mod groups;
pub mod icons;
pub mod shell;
pub mod sway;
pub mod tree;
