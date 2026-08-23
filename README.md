# sway-switchboard

A cascading mouse menu for sway workspaces. Hovering a group opens another
panel to its left, as deep as the workspace names go.

It is deliberately not something you type into. `sway-ws-switch` already does
fuzzy and most-recently-used, and does it better. This is the other half:
seeing the shape of the desk with a pointer, and reaching a workspace without
knowing its name first.

## Where the grouping comes from

Nowhere new. sway workspace names already carry it:

    2:chief:api:v2
    │ └────┬────┘
    │      └─ path: chief -> api -> v2
    └─ keyboard slot

Any depth works, and nothing is invented: a workspace whose path is one segment
long is a leaf at the top level, sitting beside the groups rather than being
herded under an "Elsewhere" heading. A path can be both a group and a
workspace — if `chief:api` exists alongside `chief:api:v2`, then `api` is a
group whose children include a leaf for the workspace itself.

Glyphs come from waybar's `workspace-icons.json`, keyed the same way, so a row
wears the icon the bar already shows. Casing follows the same acronym list
`sway-ws-lib` uses, so "API V2" reads the same in both.

## Why the panels open leftwards

The button that opens it lives at the right end of the bar. Growing rightwards
would walk off the screen on the first level.

## The dwell

Moving the pointer from a group row towards its panel crosses the rows in
between, and without a delay every one of them would swap the panel out from
under you — the reason cascading menus feel bad when nobody bothered. Rows arm
a 180ms timer on enter and disarm it on leave: long enough to cross two rows,
short enough not to feel like a wait. Clicking opens immediately, because a
deliberate click should never be made to wait.

## No thumbnails, ever

sway does not render workspaces you are not on, so `wlr-screencopy` has nothing
to copy. The pips on a group row are as close as it gets: one per workspace,
filled when it holds windows, gold for the one you are on.

## Building

Needs GTK 4.12+ and [gtk4-layer-shell](https://github.com/wmww/gtk4-layer-shell),
which Ubuntu does not package — build it with `--prefix=/usr/local`.

    cargo build --release
    install -Dm755 target/release/sway-switchboard ~/.local/bin/sway-switchboard

## Theming

`style.css` here is compiled in as the fallback. The one that actually runs is
`~/.config/switchboard/style.css`, kept in the dotfiles repo beside waybar's and
swaync's. It is read at startup and the menu is rebuilt every time it opens, so
editing it is the same as reloading it.
