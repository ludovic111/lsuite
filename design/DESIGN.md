# lsuite design system (v2, 2026-10-06)

One look for every lsuite app: **black and white, cut square, with grain**. Ink on paper, nothing
rounded, surfaces that cast hard shadows, and a page of film grain and dithered light behind the
chrome. The apps are told apart by their names and their work, not by a color. See it live at
**lsuite.xyz/design** (`design/index.html`).

kimchi wears v2 first (from its next release). ryolune and zenith still wear v1 (frosted glass, one signature color
per app); their pages on the site stay empty until they move over.

- **Source**: `design/tokens.json`. **Generated**: `design/tokens.css` (`node design/build.mjs`).
  Web UIs copy `tokens.css` and use the `--ls-*` variables; Rust or native code reads
  `tokens.json`. Never hard-code a value that lives in the tokens; when an app needs a new one, add
  it here first and rebuild.
- Set the app and the mode on the root element: `<html data-app="kimchi" data-mode="dark">`
  (`data-mode` absent = follow the system). `data-app` no longer changes any color.

## Ink

| | Dark | Light |
| --- | --- | --- |
| Page / raised / sunken | `#050505` / `#0e0e0e` / `#000000` | `#f0f0f0` / `#fbfbfb` / `#e3e3e3` |
| Text / 2 / 3 | `#f2f2f2` / `#a8a8a8` / `#707070` | `#0a0a0a` / `#4d4d4d` / `#7a7a7a` |
| Ink (`--ls-ink`, the accent) | white | black |
| Danger | `#ff5b4d` | `#c8291c` |

- **The accent is the ink** of the mode, for every app: selection, focus, the playhead, the primary
  action. **A chosen thing is inverted** (paper on ink): the selected segment, the open tab, a lit
  toggle, the menu item under the pointer.
- **Red only for what destroys or records.** Warnings and success are greys; every state also has
  an icon or a word, never a hue alone.
- **Logos of other services keep their own colors** (providers, plugins). Everything else is grey.
- The work keeps its own colors: footage, 3D scenes, waveforms are what the person made.
- The `apps.*.scale` steps stay in `tokens.json` for v1 apps and history; v2 interfaces don't use them.

## Surfaces

| Layer | Token / class | Used for |
| --- | --- | --- |
| Page | `.ls-backdrop` (`--ls-bg`, dots of `--ls-dots-image` in two corners at `--ls-dither-strength`, `--ls-grain-image` over all at `--ls-grain-strength`) | Behind everything. Natively, draw the grain and the ordered-dither light at device pixels (kimchi: `ui/grain.rs`). |
| Work | `--ls-bg-raised`, `--ls-bg-sunken` (solid) | Timeline, canvas, editors, tables. **Never grain or tint on the work.** |
| Tier 1 · chrome | `.ls-glass-1` (62 % over the page) | Sidebars, toolbars, title bar, inspectors. |
| Tier 2 · floating | `.ls-glass-2` (92–94 %) | Menus, popovers, the palette, toasts. |
| Tier 3 · modal | `.ls-glass-3` (96–97 %) over `--ls-scrim` | Dialogs, sheets, inside corner brackets. |

Every tier has a 1 px edge (`--ls-glass-edge`) and floating tiers a **hard offset shadow**
(`--ls-glass-shadow`: 4 px down and right, no blur; in the dark a soft black one under it so the
surface separates). The primary button stands on a smaller one (`--ls-chip-shadow`). Edges between
docked areas are hairlines (`--ls-line`). `prefers-reduced-transparency` makes every tier opaque.

## Layout

- **Every area is titled** like a sidebar: a title bar with the area's name (Media, Viewer,
  Timeline, Agent), what it shows in mono (`1920×1080 · 30 fps`), and its actions on the right.
- **Tools that belong together are boxed together**: one box, hairlines between the tools
  (history · AI · panels · app in a title bar; edit · modes · add · sound · zoom in a timeline).
  A tool shows its icon and its label when there is room, the icon alone (the label in its
  tooltip) when there isn't.
- Section headings in caps (mono) run into a hairline, like a drawing.
- Rows that carry switches show them always, boxed, lit when on; names are never cut to make room
  for controls (two lines instead).
- Dialogs and the main composer sit inside **viewfinder brackets**.

## Type, shape, motion

- **Chakra Petch** (UI, 400–700; a corner cut off every letter) and **IBM Plex Mono** (numbers,
  time, code, labels in caps), both OFL, bundled (no font request at runtime).
- Sizes: 11 · 12 · **13 (controls)** · 15 (body) · 17 · 22 · 28 · 40. Display tracking -0.01 to
  -0.025 em. A word can be set in negative (ink block) for emphasis, once per screen.
- Radii: **zero** everywhere (`--ls-radius-*` are `0px`). Only things round in the world stay round
  (a 3D navigation sphere, a dial).
- Spacing: 4-point grid (`--ls-space-1…10`).
- Motion: 120 / 200 / 320 ms with `--ls-motion-ease`. Drags, scrubbing and playback are never eased.
  Everything stops under `prefers-reduced-motion`.

## App icons

The app's mark, in one ink with one dithered part, white on a near-black tile (the macOS icon grid:
824 px continuous-corner tile on 1024, the platform's shape), with a corner of dithered light like
the page. Marks: kimchi's napa stalk cut square, a sharp leaf and a leaf dissolving into dither
(kimchi `scripts/gen-mark.py`). ryolune and zenith get theirs when they move to v2.

## Accessibility

- Text contrast ≥ 4.5:1 (≥ 3:1 for large text and icons) on every surface **including each tier
  over the densest grain and dither**, in both modes. Fix the tier or the grey, never the threshold.
- Focus is always visible (`--ls-accent-ring`, 2 px, offset 2).
- Color is never the only signal (icons or labels with every state).

## Adopting it in an app

1. Copy `design/tokens.css` (web UI) or read `design/tokens.json` (native) and the two fonts.
2. Map the app's theme onto `--ls-*`: ink accent, greys, red for danger only.
3. Square every corner, hard shadows on floating surfaces, grain behind the chrome, solid work.
4. Title every area, box the tools by kind, invert what is chosen.
5. Redraw the mark and the icon in one ink.
6. Add the contrast test. Ship dark and light.
