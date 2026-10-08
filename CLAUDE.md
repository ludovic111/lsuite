# lsuite.xyz

The suite's site; see README.md (layout, routes, domains) and STANDARD.md (the contract every
lsuite app meets, with a status table), PLUGINS.md (plugins), PASS.md (lsuite Pass) with AI.md,
CLOUD.md and MARKETPLACE.md (its three parts' contracts).
Each app repo (`../ryolune`, `../kimchi`, `../zenith`, `../nori`, `../folio`)
has an "lsuite" section in its CLAUDE.md with its remaining gaps.

## Current availability (2026-10-07)

- **Five apps, all in beta** (owner's decision on the night of 2026-10-06): ryolune (music),
  kimchi (video), zenith (code), nori (image and design: Photoshop + Illustrator + InDesign in one
  document) and folio (office: documents, spreadsheets and presentations). The suite's line: every
  kind of creative and office tool, combined, free and open source, driven by your agent.
- **The apps come only through the lsuite app** (DISTRIBUTION.md, owner's decision 2026-10-07,
  like Creative Cloud): a free lsuite account gets every app; the source stays open. Each app page
  has "Get <app> in the lsuite app" (`#downloads`, primary button `/launcher/download`, then
  `/launcher`) instead of downloads, and `/<app>/download[/…]` is a 302 to `/launcher`. The builds
  live in the private `ludovic111/lsuite-builds` (one release per version, tag `<app>-v<version>`),
  served by `builds.js` (`/api/apps/<app>/latest`, `latest.json`, `releases/latest`,
  `files/<tag>/<name>`: app token required, 302 to GitHub's signed address) with
  `LSUITE_BUILDS_TOKEN` (Railway; 503 without it). `%VERSION:<app>%` and `/api/apps` read the
  versions there when the token is set (else the public releases while they last, then
  `FALLBACK_VERSIONS`): ryolune 0.15.3, kimchi 0.10.0, zenith 0.4.0, nori 0.1.0 and folio 0.1.0.
  The apps' public GitHub releases become drafts once launcher 0.2.0 and each app's next version
  (whose updater reads lsuite.xyz) are out; the changelog cards' "Full notes" links to them will
  need another target then.
- **lsuite Pass** (PASS.md, owner's decision 2026-10-07) is the paid subscription, renamed from
  "lsuite AI": one optional plan for everything extra, the apps staying free. It holds **lsuite AI**
  (AI.md: the agents; the apps' provider keeps that name, and the API paths `/api/ai/…`,
  `/api/account/…` don't change), **lsuite Cloud** (CLOUD.md) and the **lsuite Marketplace**
  (MARKETPLACE.md). Tiers Free / Plus $12 / Pro $29 / Studio $79, still a demo (no payment taken).
  Pages: `/pass` (`pass/index.html`; `/ai` is a 301 to it), `/marketplace`, `/account`,
  `/account/connect`, `/account/checkout`. Say lsuite Pass for the subscription, lsuite AI for the
  agents, models and credits. Nav: Pass and Marketplace after the apps; footer and home link both.
- **lsuite Marketplace** (MARKETPLACE.md, `marketplace.js`, 2026-10-07): plugins anyone with an
  account publishes, every version reviewed by an admin (`LSUITE_ADMIN_EMAILS`; an admin's own are
  approved at once and marked "by lsuite"), installed with a paid plan. Bundles in
  `<data>/marketplace/` (or the cloud's object store), 15 MB each and 80 MB in all in the demo.
  `/marketplace` lists the approved plugins server-side; `/account` has "Your plugins" and, for
  admins, "Review". The launcher's Marketplace area and `lsuite-cli market.*` are being built.
- The site wears **design system v2** (`design/DESIGN.md`). All app names are lowercase.
  Real native-app captures are in `assets/img/<app>/`; each hero uses light and dark images.
  All five marks come from the app repositories' own generated icons. App capabilities and
  command counts describe the new beta builds; public download versions follow GitHub Releases.
- **lsuite launcher** (`launcher/`, 2026-10-07, see its README): a Rust/GPUI app that installs and
  updates the five apps from their signed releases, manages the lsuite account (lsuite Pass) and **lsuite
  Cloud** (CLOUD.md, `cloud.js`, `/api/cloud`, two-way synced folders; storage per plan decided:
  50 GB / 250 GB / 1 TB, demo 100 MB per account; an S3-compatible store is supported for
  production). Its page is
  `/launcher` (`pages/launcher.html`), downloads from `launcher-v*` releases of this repository
  (0.1.0 and 0.1.1 published 2026-10-07; 0.2.0, which installs the apps through lsuite.xyz with an account, published 2026-10-08 and the latest; `launcher/CHANGELOG.md`),
  built by kimchi's suite release workflow (launcher/README.md, Releasing).
- The favicon and the nav keep the plain lsuite grain tile (no lsuite logo).
- Earlier completed notes below describe the previous pages; keep the current availability
  above unless explicitly asked to change it.

## Next session (2026-10-03)

- [x] ryolune 0.12 page (2026-10-01): New in 0.12, one theme dark/light gallery, 35 plugins,
      199 commands, new agents. Versions on pages are `%VERSION:<app>%`, filled by `server.js`
      from the latest published GitHub release, so the hero shows 0.11.1 until the 0.12.0 release
      is published (its release run failed on Apple notarization: the Apple developer agreement
      must be accepted, then re-run). Retake captures from the ryolune app (`ryolune-cli
      ui.screenshot --path <file>` while it runs) when they change.
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
