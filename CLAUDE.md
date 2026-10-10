# lsuite.xyz

The suite's site; see README.md (layout, routes, domains), STANDARD.md (the contract every
lsuite app meets, with a status table), PLUGINS.md (plugins), DISTRIBUTION.md (getting the apps)
and HARNESS.md (the agent harness).
Each app repo (`../ryolune`, `../kimchi`, `../nori`, `../folio`)
has an "lsuite" section in its CLAUDE.md with its remaining gaps.

## Current availability (2026-10-10)

- **Four apps, all in beta** (owner's decision on the night of 2026-10-06; the code app was
  dropped on 2026-10-10 and its old addresses redirect to the home page): ryolune (music),
  kimchi (video), nori (image and design: Photoshop + Illustrator + InDesign in one document) and
  folio (office: documents, spreadsheets and presentations). The suite's line: every kind of
  creative and office tool, combined, free and open source, driven by your agent.
- **Linux only during the beta** (owner's decision, 2026-10-08): the launcher and the four apps
  are built and shipped for Linux x86_64 only; macOS and Windows are "coming soon" everywhere on
  the site (hero lines, stats, the launcher's download tiles, `/launcher/download/<macos|windows…>`
  → `/launcher#downloads`). Their files were removed from every release (launcher, lsuite-builds
  and the apps' public releases); the release workflows keep the macOS/Windows matrix lines
  commented out. Past changelog cards still describe what those versions shipped.
- **Entirely free, no account** (owner's decision, 2026-10-10): lsuite Pass (lsuite AI, lsuite
  Cloud, the lsuite Marketplace), the lsuite account and every page and API of theirs were removed
  (`ai.js`, `cloud.js`, `marketplace.js`, `live.js`, PASS.md, AI.md, CLOUD.md, MARKETPLACE.md).
  `/ai`, `/pass`, `/account[/…]` are 301s to `/`, `/marketplace` to `/plugins` (`pages/plugins.html`:
  plugins built by your agent, PLUGINS.md); `/api/ai|account|cloud|marketplace|billing…` answer a
  JSON 410. The nav has Plugins after the apps; no Account button. Never bring back plans, credits,
  sign-in or "demo" wording. The apps and the launcher drop their account, lsuite AI provider,
  cloud and marketplace in their next releases (PRs `free-suite` / `free-launcher`, 2026-10-10).
- **The apps come only through the lsuite app** (DISTRIBUTION.md, owner's decision 2026-10-07,
  like Creative Cloud), with no account; the source stays open. Each app page
  has "Get <app> in the lsuite app" (`#downloads`, primary button `/launcher/download`, then
  `/launcher`) instead of downloads, and `/<app>/download[/…]` is a 302 to `/launcher`. The builds
  live in the private `ludovic111/lsuite-builds` (one release per version, tag `<app>-v<version>`),
  served by `builds.js` (`/api/apps/<app>/latest`, `latest.json`, `releases/latest`,
  `files/<tag>/<name>`: public, 302 to GitHub's signed address) with
  `LSUITE_BUILDS_TOKEN` (Railway; 503 without it). `%VERSION:<app>%` and `/api/apps` read the
  versions there when the token is set (else the public releases while they last, then
  `FALLBACK_VERSIONS`): ryolune 0.15.3, kimchi 0.10.0, nori 0.1.0 and folio 0.1.0.
  The apps' public GitHub releases become drafts once launcher 0.2.0 and each app's next version
  (whose updater reads lsuite.xyz) are out; the changelog cards' "Full notes" links to them will
  need another target then.
- The site wears **design system v2** (`design/DESIGN.md`). All app names are lowercase.
  Real native-app captures are in `assets/img/<app>/`; each hero uses light and dark images.
  All four marks come from the app repositories' own generated icons. App capabilities and
  command counts describe the new beta builds; public download versions follow GitHub Releases.
- **lsuite launcher** (`launcher/`, 2026-10-07, see its README): a Rust/GPUI app that installs and
  updates the four apps from their signed releases and, from 0.3.0, lists their plugins and asks an
  app's agent to build one (no account, cloud or marketplace any more). Its page is
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
      as kimchi closes its gaps.
- [x] **Design system on the site itself** (2026-10-01): pages load `/design/tokens.css` before
      `assets/styles.css`, whose variables all map onto `--ls-*`; app pages set `data-app` on
      `<html>` (signature color, aurora backdrop); nav, cards, cells, tables, stats, downloads, FAQ,
      bands and footer are glass tier 1, terminals and code tier 2; screenshots stay solid. Light and
      dark follow the system. No hard-coded color is left in `styles.css` or the pages: add a token.
