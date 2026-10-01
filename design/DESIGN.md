# lsuite design system (v1, 2026-10-01)

One look for every lsuite app: **frosted glass for the chrome, solid surfaces for the work, and
one signature color per app**, the way a suite's apps are told apart by their color (a blue one,
an orange one) while sharing everything else. See it live at **lsuite.xyz/design** (`design/index.html`).

- **Source**: `design/tokens.json`. **Generated**: `design/tokens.css` (`node design/build.mjs`).
  Web UIs (Tauri/React/Svelte, Next.js) copy `tokens.css` and use the `--ls-*` variables; Rust or
  native code reads `tokens.json`. Never hard-code a value that lives in the tokens; when an app
  needs a new one, add it here first and rebuild.
- Set the app and the mode on the root element: `<html data-app="kimchi" data-mode="dark">`
  (`data-mode` absent = follow the system).

## Signature colors

Same OKLCH lightness and chroma for every app, only the hue changes, so the apps read as one
family. Each app has an 11-step scale (`--ls-<app>-50 … -950`).

| App | Hue | Accent dark (400) | Accent light (600) | In the spirit of |
| --- | --- | --- | --- | --- |
| ryolune · music | 185° teal | `#00c5b4` | `#009586` | Audition |
| kimchi · video | 32° chili coral | `#f7806a` | `#c3513d` | PowerPoint |
| zenith · code | 262° blue | `#72a6ff` | `#4777d2` | Photoshop, Word |
| *reserved* | violet 300°, green 150°, amber 75°, pink 350° | | | next apps |

`--ls-accent`, `--ls-accent-hover`, `--ls-accent-text` (text and links on the page background),
`--ls-accent-soft` (selected rows, hovered menu items), `--ls-accent-ring` (focus) follow the app
and the mode. Text on an accent fill uses `--ls-text-on-accent`.

**The accent means "yours or active"**: selection, focus, the playhead, the primary action, what
the agent touched, the app's own identity. **States keep their own colors**: record and errors red
(`--ls-danger`), warnings amber, success green, mute/solo and meters keep their meaning. The accent
never replaces a state color.

## Surfaces

| Layer | Token / class | Used for |
| --- | --- | --- |
| Window backdrop | `.ls-backdrop` (`--ls-bg` + two soft radial glows of the app color, `--ls-aurora-strength`) | Behind everything; it is what the glass blurs. |
| Work | `--ls-bg-raised`, `--ls-bg-sunken` (solid) | Timeline, canvas, editors, tables, documents. **Never glass**: what you make is never blurred or tinted. |
| Glass 1 · chrome | `.ls-glass-1` (blur 24, 55 % dark / 58 % light) | Sidebars, toolbars, title bar, inspectors. |
| Glass 2 · floating | `.ls-glass-2` (blur 32, 72 % / 74 %) | Popovers, menus, command palette, transport HUD, toasts. |
| Glass 3 · modal | `.ls-glass-3` (blur 40, 84 % / 86 %) over `--ls-scrim` | Dialogs, sheets. |

Every glass surface has a 1 px edge (`--ls-glass-edge`), a top inner highlight
(`--ls-glass-highlight`) and a soft drop (`--ls-glass-shadow`). Edges between docked chrome and
work are hairlines (`--ls-line`), not shadows.

**Fallbacks**: `prefers-reduced-transparency: reduce` and browsers without `backdrop-filter` get
`--ls-glass-opaque` (handled in `tokens.css`). Native apps honour the OS setting the same way.

**Native windows**: on macOS use the real window material (Tauri: the `window-vibrancy` crate with
`NSVisualEffectMaterial::Sidebar` / `HudWindow`, transparent webview background; AppKit:
`NSVisualEffectView`) for the window-level glass, and the CSS tiers inside. Windows 11: Mica for the
window, Acrylic for floating. Linux: CSS tiers over `.ls-backdrop`.

## Type, shape, motion

- **Manrope** (UI, 400–700) and **IBM Plex Mono** (numbers, time, code, labels in caps), both OFL,
  bundled with the app (no font request at runtime).
- Sizes: 11 · 12 · **13 (controls)** · 15 (body) · 17 · 22 · 28 · 40. Display sizes use
  negative tracking (-0.02 to -0.035 em). Numbers that change use tabular figures.
- Radii: 4 · 6 (controls) · 10 (popovers) · 14 (panels, windows) · 20 (cards) · full (pills).
- Spacing: 4-point grid (`--ls-space-1…10`).
- Motion: 120 / 200 / 320 ms with `--ls-motion-ease`; `--ls-motion-spring` only for small pops.
  Drags, scrubbing and playback are never eased. Everything stops under `prefers-reduced-motion`.

## App icons

One template for all apps: a macOS squircle (`--ls-radius-icon` on a square, or the system icon
grid at 1024 px with an 824 px tile), filled with a top-left to bottom-right gradient of the app's
**300 → 600 → 800** steps, a white glyph at ~58 % of the tile with a 1 px soft shadow, and a glass
sheen (white, 28 % → 0 over the top half). Glyphs: ryolune the ring and dot, kimchi the stalk and
two leaves, zenith the circle with the sun at its top. Generate every size from one SVG per app.

## Accessibility

- Text contrast ≥ 4.5:1 (≥ 3:1 for large text and icons) on every surface **including each glass
  tier over the brightest and darkest backdrop**, in both modes. Test it like ryolune's
  `appearance.test.ts`: fix the tier's opacity or the color step, never the threshold.
- Focus is always visible (`--ls-accent-ring`, 2 px, offset 2).
- Color is never the only signal (icons or labels with every state).

## Adopting it in an app

1. Copy `design/tokens.css` (web UI) or read `design/tokens.json` (native) and the two fonts.
2. Map the app's existing theme tokens onto `--ls-*` (keep app-specific tokens, such as meter
   colors or track colors, but derive their neutrals from the lsuite ones).
3. Put chrome on the glass tiers, keep work surfaces solid, set `data-app` and `data-mode`.
4. Redraw the app icon from the template.
5. Add the contrast test. Ship dark and light.
