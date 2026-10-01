# lsuite

The site of **lsuite** (written in lowercase), at [lsuite.xyz](https://lsuite.xyz): a free,
open-source creative suite whose apps can be driven end to end by an AI agent.

| App | Page | Repository |
| --- | --- | --- |
| ryolune · music | `/ryolune` | [ludovic111/ryolune](https://github.com/ludovic111/ryolune) |
| kimchi · video | `/kimchi` | [ludovic111/kimchi](https://github.com/ludovic111/kimchi) |
| zenith · hub | `/zenith` | [ludovic111/zenith](https://github.com/ludovic111/zenith) |

Plain HTML, CSS and JavaScript served by a dependency-free Node server (`server.js`).

```bash
npm run dev     # http://localhost:4321
npm test
```

## Layout

- `index.html` is the suite page; `<app>/index.html` is each app's page, served at `/<app>`
  (`/<app>/` and `/index.html` redirect to the bare path).
- `partials/nav.html` and `partials/foot.html` are inserted where a page says
  `<!-- include:nav -->` / `<!-- include:foot -->`; the nav link of the page's app gets
  `aria-current="page"`. `%ORIGIN%` becomes the request's origin (Open Graph, canonical).
- `assets/` is the only static folder: `styles.css` (every page; the accent comes from
  `body.app-<name>`), `main.js` (reveals, "Download for your OS", copy buttons, the ryolune theme
  gallery), fonts (Manrope, IBM Plex Mono, OFL), images and the ryolune film.
- `?v=` on `/assets/*.js|css` is replaced by a hash of the file, so those URLs are cached for good.
- CSP is `'self'` only: no inline scripts, no third-party requests.

## Routes

| Path | Does |
| --- | --- |
| `/ryolune/download[/<platform>]` | 302 to the latest ryolune release asset (`macos-arm64`, `macos-x86_64`, `windows-x86_64`, `linux-x86_64`; by User-Agent without one). Keep in step with `update::asset_name` in ryolune. |
| `/kimchi/download[/<platform>]` | 302 to the matching asset of kimchi's latest release, looked up on the GitHub API (cached 10 min): `macos-arm64`, `macos-x86_64`, `windows-x86_64`, `windows-msi`, `linux-appimage`, `linux-deb`, `linux-rpm`. |
| `/support`, `/<app>/support` | 302 to `LSUITE_DONATION_URL` (https only), else GitHub Sponsors. |
| `/health` | `ok`, for Railway's health check. |
| `/robots.txt`, `/sitemap.xml` | Generated for the request's origin. |

## Domains

- `LSUITE_CANONICAL_HOST=lsuite.xyz` on the host: any other host name (www, the Railway domain)
  gets a 301 to the same path on lsuite.xyz.
- **ryolune.com** is a Porkbun URL forward (permanent 301, path included, wildcard so www
  follows) to `https://lsuite.xyz/ryolune`: `ryolune.com/support` → `/ryolune/support`,
  `ryolune.com/download/macos-arm64` → `/ryolune/download/macos-arm64`, and the browser keeps
  `#downloads` and other anchors (the ryolune page keeps the same section ids). Its DNS is
  Porkbun's forwarder (ALIAS and `*` CNAME to `uixie.porkbun.com`); no Railway service is involved.
  `MOVED_HOSTS` in `server.js` does the same redirect should those hosts ever point here.
- `/ondera` and `/ondera/*` redirect to `/ryolune` (its name before 0.11).
- The former ryolune site service (Railway project `ondera-site`) was deleted on 2026-10-01; its
  domain `site-production-7751.up.railway.app`, which ryolune 0.11.0 linked from Help › Support,
  no longer answers. 0.11.1 and later link to ryolune.com/support.

## Updating an app's page

The copy is written from each app's README and release notes. When an app ships: update its
version in the hero line, the JSON-LD `softwareVersion`, the card on the suite page, and the
download notes. Screenshots live in `assets/img/<app>/` (2000x1250 WebP; kimchi's comes from its
demo UI: `npm run ui:dev`, `/?editor&t=13&sel=c2&tab=generate&mode=video&prompt=…`, 1600x1000 at
1.25x, `cwebp -q 76`). Open Graph images are 1200x630 captures of each page's hero in
`assets/img/og/`.
