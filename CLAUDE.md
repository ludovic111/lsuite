# lsuite.xyz

The suite's site; see README.md (layout, routes, domains) and STANDARD.md (the contract every
lsuite app meets, with a status table), PLUGINS.md and AI.md (the plugin and lsuite AI contracts).
Each app repo (`../ryolune`, `../kimchi`, `../zenith`, `../nori`, `../folio`)
has an "lsuite" section in its CLAUDE.md with its remaining gaps.

## Current availability (2026-10-07)

- **Five apps, all in beta** (owner's decision on the night of 2026-10-06): ryolune (music),
  kimchi (video), zenith (code), nori (image and design: Photoshop + Illustrator + InDesign in one
  document) and folio (office: documents, spreadsheets and presentations). The suite's line: every
  kind of creative and office tool, combined, free and open source, driven by your agent.
- ryolune, kimchi and zenith have full pages with downloads (`DOWNLOADS` and `REPOS` in
  `server.js`, `%VERSION:<app>%`); nori and folio have no repository on GitHub yet: their pages
  say "First build coming", `/<app>/download[/…]` lands on the page (`published: false`), and the
  route table already names `ludovic111/nori` and `ludovic111/folio`. When one ships: set
  `published: true`, check the asset names, add it to `REPOS`/`FALLBACK_VERSIONS`, put back a
  download section like zenith's and a version in its hero.
- **lsuite AI** (AI.md) is live as a demo: `ai.js` serves the API, `/ai`, `/account`,
  `/account/connect`, `/account/checkout` (Demo — no payment is taken). The apps stay free;
  lsuite AI is the one optional thing sold. Footer and home say so; no telemetry still holds.
- The site wears **design system v2** (`design/DESIGN.md`). All app names are lowercase.
  Real native-app captures are in `assets/img/<app>/`; each hero uses light and dark images.
  All five marks come from the app repositories' own generated icons. App capabilities and
  command counts describe the new beta builds; public download versions follow GitHub Releases.
  nori and folio have local macOS builds but no public release yet.
- **lsuite launcher** (`launcher/`, 2026-10-07, see its README): a Rust/GPUI app that installs and
  updates the five apps from their signed releases, manages the lsuite AI account and **lsuite
  Cloud** (CLOUD.md, `cloud.js`, `/api/cloud`, two-way synced folders; storage per plan is a
  proposal awaiting the owner; an S3-compatible store is supported for production). Its page is
  `/launcher` (`pages/launcher.html`), downloads from `launcher-v*` releases of this repository,
  built by kimchi's suite release workflow (launcher/README.md, Releasing).
- The favicon and the nav keep the plain lsuite grain tile (no lsuite logo).
- Earlier completed notes below describe the previous pages; keep the current availability
  above unless explicitly asked to change it.

## Next session (2026-10-03)

- [x] ryolune 0.12 page (2026-10-01): New in 0.12, one theme dark/light gallery, 35 plugins,
      199 commands, new agents. Versions on pages are `%VERSION:<app>%`, filled by `server.js`
      from the latest published GitHub release, so the hero shows 0.11.1 until the 0.12.0 release
      is published (its release run failed on Apple notarization: the Apple developer agreement
      must be accepted, then re-run). Retake captures from `../ryolune/site/img/` when they change.
- [x] zenith page (2026-10-01): rewritten for the new zenith, a Mac app for coding with agents
      (Claude Code and Codex threads, Rust server, Tauri app); `zmock` now illustrates a thread.
      The old dashboard, Ask zenith, the agent team and the `zenith_*` MCP tools are gone: don't
      advertise them. `assets/img/og/zenith.png` retaken from the new page (2026-10-01).
- [x] zenith marked Coming soon (2026-10-02): zenith is not ready, so the site no longer offers
      it. Its page is a preview (badge, future tense, no download, "Follow it on GitHub"), "What's
      new in 0.2" is gone, the home card, lede and agent-readiness column say Coming soon, nav and
      footer carry a "soon" marker, and `/zenith/download[/…]` redirects to `/zenith`. T3 Code is
      used instead for now. When zenith ships: put back the download section, `%VERSION:zenith%`,
      the `ZENITH` assets in `server.js` and the table cells, and retake `assets/img/og/zenith.png`
      (its hero has no Coming soon badge).
- [x] kimchi 0.5 page (2026-10-03): New in 0.5 (keyframes, 2D motion, 3D, templates, transitions,
      colour, local captions, reverse/freeze, GPU export), 122 commands, new captures from the
      real app (README, "Updating an app's page"), og image retaken. Home: kimchi's card, its
      agent-readiness column (`kimchi-cli`, `kimchi-mcp --live`, Agent panel) and the plug-in
      example; `STANDARD.md` count. Not yet checked by anyone: 3D on Metal, Windows builds.
- [x] kimchi 0.6 page (2026-10-03): New in 0.6 (stability pass, logs and crash reports, what's new,
      updates everywhere, phone media), 128 commands, and a **Changelog** section (`#changelog`): a
      timeline of every release like ryolune's, `.releases` / `.release` / `.releases__older` in
      `styles.css` (tier-1 glass, tokens only). Each kimchi release: add its card on top with
      `release--now` (move the class off the previous one), from `../kimchi/CHANGELOG.md`. The
      captures are still 0.5's (the window barely changed).
- [x] kimchi 0.7 page (2026-10-04): New in 0.7 (Blender-like 3D: modelling, modifiers, path tracer;
      After Effects-like 2D; expressions; render ahead or live), 24 templates, 164 commands (home table
      and STANDARD.md too), changelog card. New captures from the release build on vscreen: the hero is the
      Studio on a 3D product shot (`studio[-light].webp`), the New section shows the 2D Studio
      (`studio-2d[-light].webp`), `editor[-light].webp` (also the home card) is the editor with the
      path-traced product shot. The demo project is built by `kimchi-cli` in `~/.cache/kimchi-shots/`.
- [ ] Keep the agent-readiness table on the home page and `STANDARD.md`'s status table in step
      as kimchi and zenith close their gaps.
- [x] **Design system on the site itself** (2026-10-01): pages load `/design/tokens.css` before
      `assets/styles.css`, whose variables all map onto `--ls-*`; app pages set `data-app` on
      `<html>` (signature color, aurora backdrop); nav, cards, cells, tables, stats, downloads, FAQ,
      bands and footer are glass tier 1, terminals and code tier 2; screenshots stay solid. Light and
      dark follow the system. No hard-coded color is left in `styles.css` or the pages: add a token.
