//! Shared innards for sway-switchboard.
//!
//! A menu that has to know what the desk looks like: the sway tree (`sway`),
//! the shape the workspace names imply (`tree`), the glyph the bar already
//! shows for each one (`icons`), and how the window gets on screen wearing a
//! stylesheet from the dotfiles (`shell`).

pub mod icons;
pub mod shell;
pub mod sway;
pub mod tree;
