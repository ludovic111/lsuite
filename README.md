# lsuite

The site of **lsuite** (written in lowercase), at [lsuite.xyz](https://lsuite.xyz): every kind of
creative and office tool, combined, free and open source, and driven by your agent. Four apps, all
in beta:

**Get the launcher:** [lsuite.xyz/launcher](https://lsuite.xyz/launcher), or the latest `launcher-v*` [release](https://github.com/ludovic111/lsuite/releases).
The four apps come only through it, with a free lsuite account ([DISTRIBUTION.md](DISTRIBUTION.md)):
their builds live in the private `ludovic111/lsuite-builds`, served by `builds.js`.

**Beta: Linux only.** While lsuite is in beta, the launcher and the four apps are built and
shipped for Linux x86_64 only; macOS and Windows are coming soon (the site says so, and no builds
are made for them).

| App | Kind | Page | Source |
| --- | --- | --- | --- |
| ryolune | music | `/ryolune` | [ludovic111/ryolune](https://github.com/ludovic111/ryolune) |
| kimchi | video | `/kimchi` | [ludovic111/kimchi](https://github.com/ludovic111/kimchi) |
| nori | images, vectors and page layout | `/nori` | [ludovic111/nori](https://github.com/ludovic111/nori) |
| folio | documents, spreadsheets and slides | `/folio` | [ludovic111/folio](https://github.com/ludovic111/folio) |

The apps are free and MIT licensed. The one thing lsuite sells is optional: **lsuite Pass**
([PASS.md](PASS.md)), a demo for now (no payment is taken), with its pages at `/pass` and
`/account`. It holds **lsuite AI** ([AI.md](AI.md), `ai.js`), agents that work in every app without
setup; **lsuite Cloud** storage ([CLOUD.md](CLOUD.md), `cloud.js`), managed from the lsuite
launcher; and the **lsuite Marketplace** ([MARKETPLACE.md](MARKETPLACE.md), `marketplace.js`,
`/marketplace`), plugins anyone can publish, every version reviewed. Every app has plugins
([PLUGINS.md](PLUGINS.md)), and [STANDARD.md](STANDARD.md) is the contract they all meet.

Plain HTML, CSS and JavaScript served by a dependency-free Node server (`server.js`).

```bash
npm run dev     # http://localhost:4321
npm test
```

## Layout

- `index.html` is the suite page; `<app>/index.html` is each app's page, served at `/<app>`
  (`/<app>/` and `/index.html` redirect to the bare path). The launcher's page is
  `pages/launcher.html`, served at `/launcher`: the folder `launcher/` holds its Rust workspace,
  which is never served (only `/assets/` is static). A page's app, for the nav, is its path's
  first segment.
- `partials/nav.html` and `partials/foot.html` are inserted where a page says
  `<!-- include:nav -->` / `<!-- include:foot -->`; the nav link of the page's app gets
  `aria-current="page"`. `%ORIGIN%` becomes the request's origin (Open Graph, canonical).
- `assets/` is the only static folder: `styles.css` (every page, built on the design system's
  `/design/tokens.css`, loaded first; all apps use monochrome v2 chrome, light and dark
  follow the system), `main.js` (reveals, "Download for your OS", copy buttons, and the ryolune
  theme gallery, unused for now), fonts (Chakra Petch, IBM Plex Mono, OFL)
  and real native-app captures in `assets/img/<app>/` (both themes). Image credits are in
  `assets/img/SOURCES.md`.
- `pass/index.html` is `/pass` (lsuite Pass; `/ai` is a 301 to it, out of the sitemap);
  `marketplace/index.html` is `/marketplace` (`<!-- include:marketplace -->` is filled with the
  approved plugins, `?app=<app>` filters them, no script); `account/index.html`,
  `account/connect.html` and `account/checkout.html` are the account pages (`assets/account.js`,
  the only script they run; `<!-- include:plans -->` is filled from the plans of `ai.js`).
  `/account/connect` and `/account/checkout` stay out of the sitemap. `/account` also shows the
  account's lsuite Cloud usage, its marketplace submissions and, for admins, the review queue.
- `ai.js` serves the API (accounts, lsuite AI); `cloud.js` is lsuite Cloud's storage behind
  `/api/cloud`; `marketplace.js` is the lsuite Marketplace behind `/api/marketplace`; `builds.js`
  serves the apps' builds behind `/api/apps/<app>/…` (DISTRIBUTION.md); `live.js` holds the
  production services (Stripe, email).
- `?v=` on `/assets/*.js|css` is replaced by a hash of the file, so those URLs are cached for good.
- `%VERSION:<app>%` becomes the app's newest version in `ludovic111/lsuite-builds` (`builds.js`,
  cached 5 min) when `LSUITE_BUILDS_TOKEN` is set, else the app's latest public GitHub release
  while those last (cached 10 min); `FALLBACK_VERSIONS` when neither answers. So a page never
  announces a version that cannot be had yet. `%VERSION:launcher%` is the newest published
  `launcher-vX.Y.Z` release of `ludovic111/lsuite`.
- CSP is `'self'` only: no inline scripts, no third-party requests.

## The launcher

`launcher/` is **lsuite**, the suite's native launcher (Rust, GPUI; its own Cargo workspace, not
part of the site's deployment): installs and updates the apps, the lsuite account (lsuite Pass),
lsuite Cloud and, in its next release, plugins from the lsuite Marketplace. See [launcher/README.md](launcher/README.md). Its page is `/launcher` (`pages/launcher.html`,
captures in `assets/img/launcher/`: `apps`, `account`, `cloud`, each with `-light`, 2000x1250;
mark in `assets/img/icons/lsuite.webp`, from `launcher/brand/icon.png`); the home page and the
footer link to it. Its releases are published in this repository, tagged `launcher-vX.Y.Z`.

## Design system

`design/` holds the suite's shared design system: `DESIGN.md` (the spec), `tokens.json` (the
source), `tokens.css` (generated by `node design/build.mjs`) and a live preview served at
`/design` (`design/index.html`, `design/preview.js`). The server serves those three files as they
are under `/design/`.

## Routes

| Path | Does |
| --- | --- |
| `/<app>/download[/<platform>]` | 302 to `/launcher` for the four apps: they come only through the lsuite app (DISTRIBUTION.md). Their pages say "Get <app> in the lsuite app". |
| `/launcher/download[/<platform>]` | 302 to the matching asset of the newest published `launcher-vX.Y.Z` release of `ludovic111/lsuite` (`DOWNLOADS` in `server.js`, `tagPrefix`, looked up on the GitHub API, cached 10 min); without a platform the visitor's OS picks one (macOS and Windows, coming soon, land on `/launcher#downloads`); with no matching asset, that release's page, or the releases list: `macos-arm64`, `macos-x86_64` (.dmg), `windows-x86_64` (setup .exe), `windows-zip`, `linux-x86_64` (.AppImage), `linux-tar` (.tar.gz). The launcher is not one of the apps: not in `/api/apps`, no `/launcher/support`. |
| `/api/apps/<app>/latest`, `…/latest.json`, `…/releases/latest`, `…/files/<tag>/<name>` | The apps' builds (DISTRIBUTION.md, `builds.js`), for any signed-in app token (Free included; not the site's cookie): the newest release of `<app>-v*` in the private `ludovic111/lsuite-builds` (no drafts or pre-releases, newest by semver), with `latest.json` and `SHA256SUMS(.sig)` read on the server (cached 5 min) and every download address rewritten to the file route, which answers a 302 to GitHub's short-lived address (the file never passes through the site, the token never leaves it). Needs `LSUITE_BUILDS_TOKEN` (a fine-grained token, read-only on `lsuite-builds`; never in Git): without it 503. 401 "Sign in to lsuite to get the apps: the account is free."; GitHub errors 502. |
| `/api/ai/…`, `/api/account/…` | lsuite AI and accounts (AI.md, `ai.js`): JSON, Anthropic-compatible `/api/ai/v1/messages`. `LSUITE_DATA_DIR` holds account state. Live service requires explicit `LSUITE_MODE=production` and the complete Stripe, Anthropic and verified-email configuration in AI.md; adding an API key alone never turns demo accounts into paid access. |
| `/api/cloud`, `/api/cloud/…` | lsuite Cloud (CLOUD.md, `cloud.js` through `ai.js`): the account's files, app token (GET also takes the session cookie). Stored under `LSUITE_DATA_DIR/cloud/` (`production-cloud/` in production), a temporary folder without it. Demo caps: `LSUITE_CLOUD_DEMO_QUOTA`, `LSUITE_CLOUD_DEMO_MAX_FILE`, `LSUITE_CLOUD_DEMO_TOTAL` (bytes; 100 MB, 25 MB, 300 MB), and `LSUITE_CLOUD_DISK_RESERVE` (100 MB always left free on the disk). With `LSUITE_CLOUD_S3_ENDPOINT`, `…_BUCKET`, `…_ACCESS_KEY_ID`, `…_SECRET_ACCESS_KEY` (optional `…_REGION`, `…_PREFIX`, `…_PATH_STYLE`), the files go to an S3-compatible object store and the index stays in the data dir; production then applies the plans' sizes (CLOUD.md, "The object store"). |
| `/api/marketplace`, `/api/marketplace/…` | The lsuite Marketplace (MARKETPLACE.md, `marketplace.js` through `ai.js`): listings (public), submissions and uploads (app token), downloads (paid plan), review (admins: `LSUITE_ADMIN_EMAILS`, comma-separated emails). Stored under `LSUITE_DATA_DIR/marketplace/` (`production-marketplace/` in production), a temporary folder without it, or in lsuite Cloud's object store when set. Demo caps: `LSUITE_MARKET_MAX_FILE` (15 MB a bundle) and `LSUITE_MARKET_TOTAL` (80 MB in all, not with an object store); the cloud's disk reserve applies. |
| `/api/apps` | The four apps for the lsuite launcher (CLOUD.md), public: `{apps: [{id, name, kind, summary, page, repo, version, published, platforms}]}` from `DOWNLOADS`, `REPOS` and the versions `%VERSION:<app>%` uses (the builds when configured); `Cache-Control: public, max-age=300`. |
| `/pass`, `/marketplace`, `/account`, `/account/connect`, `/account/checkout` | lsuite Pass, the marketplace and the account pages (session cookie `lsuite_session`, HttpOnly, SameSite=Lax). `/ai` (and `/ai/`) is a 301 to `/pass`. |
| `/support`, `/<app>/support` | 302 to `LSUITE_DONATION_URL` (https only), else GitHub Sponsors. |
| `/health` | `ok`, for Railway's health check. |
| `/robots.txt`, `/sitemap.xml` | Generated for the request's origin. |

## Domains

- `LSUITE_CANONICAL_HOST=lsuite.xyz` on the host: any other host name (www, the Railway domain)
  gets a 301 to the same path on lsuite.xyz.
- **ryolune.com** is a Porkbun URL forward (permanent 301, path included, wildcard so www
  follows) to `https://lsuite.xyz/ryolune`: `ryolune.com/support` → `/ryolune/support`,
  `ryolune.com/download/macos-arm64` → `/ryolune/download/macos-arm64` (→ `/launcher`), and the
  browser keeps URL fragments. The ryolune page's `#downloads` section now leads to the lsuite app. Its DNS is
  Porkbun's forwarder (ALIAS and `*` CNAME to `uixie.porkbun.com`); no Railway service is involved.
  `MOVED_HOSTS` in `server.js` does the same redirect should those hosts ever point here.
- `/ondera` and `/ondera/*` redirect to `/ryolune` (its name before 0.11).
- `/zenith` and `/zenith/*` redirect to the home page: zenith was an lsuite app (code) until
  2026-10-10, when it was dropped.
- The former ryolune site service (Railway project `ondera-site`) was deleted on 2026-10-01; its
  domain `site-production-7751.up.railway.app`, which ryolune 0.11.0 linked from Help › Support,
  no longer answers. 0.11.1 and later link to ryolune.com/support.

## Updating an app's page

The copy is written from each app's README and release notes. When an app ships: versions update by themselves;
update what's new, features and screenshots. On kimchi's page that is "New in 0.x", the command
count and a new card on top of the changelog (`#changelog`, from `../kimchi/CHANGELOG.md`, with
`release--now` moved to it). Every app page has a changelog (`#changelog`) with `release--now` on the newest card.
Placeholders still waiting for the app agents' captures are marked `shot--soon` with
`data-capture="<app>/<file>"` (and `card__shot--soon` on the home cards): replace each with a
`<picture>` like kimchi's hero.
Screenshots live in `assets/img/<app>/` (2000x1250 WebP, dark and `-light`,
`magick shot.png -quality 76 shot.webp`). kimchi's come from the real app on a
virtual screen: `vscreen size 2000x1250`, then `vscreen start target/debug/kimchi` with
`KIMCHI_WINDOW_SIZE=2000x1250` and scratch `KIMCHI_DATA_DIR`, `KIMCHI_CONFIG_DIR`, `LSUITE_HOME`,
`KIMCHI_NO_UPDATE=1` (plus `KIMCHI_FFMPEG`/`KIMCHI_FFPROBE` if no ffmpeg is installed); build the
project with `kimchi-cli` (`media.import`, `motion.addTemplate`, `captions.add`, `ui.select`,
`ui.zoom fit=true`), switch with `app.setSetting key=appearance.mode value=dark|light`, and take
`vscreen shot`. On the Mac: `KIMCHI_WINDOW_SIZE=1600x1000` and `kimchi-cli ui.screenshot`, scaled
to 2000x1250. Open Graph images are 1200x630 captures of each page's hero in `assets/img/og/`.
Pages reveal sections on scroll: full-page captures need `.reveal` forced visible.
