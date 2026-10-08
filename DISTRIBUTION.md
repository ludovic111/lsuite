# Getting the apps: only through the lsuite app

Decided by the owner on 2026-10-07: like Adobe's Creative Cloud, the five apps (ryolune, kimchi,
zenith, nori, folio) are obtained **only through the lsuite app** (the launcher, `launcher/`),
which needs a **free lsuite account**. The site offers the lsuite app alone; the apps' builds
are no longer public. The apps stay free and their source stays open (MIT): anyone may build them
from source, but the ready-made builds come through lsuite.

## Where the builds live

- **`ludovic111/lsuite-builds`**, a private GitHub repository: one release per app version,
  tagged `<app>-v<version>` (`kimchi-v0.10.0`), holding exactly the files the app's own release
  had (the platform files, `latest.json`, `*.sig`, `SHA256SUMS`, `SHA256SUMS.sig`). Signatures are
  unchanged: the launcher and each app still check them against the keys built into them, so the
  server can't alter a build unnoticed.
- The lsuite app itself stays public (`launcher-v*` releases of `ludovic111/lsuite`): it is what
  people download from lsuite.xyz.
- The apps' public GitHub releases become drafts (hidden) once the lsuite app 0.2.0 and each app's
  next version (whose updater reads lsuite.xyz) are out. Older copies of an app then update
  through the lsuite app.
- The server reads the private releases with `LSUITE_BUILDS_TOKEN` (a fine-grained token on
  `lsuite-builds` only, set on Railway; never in Git).
- Release workflows no longer publish in the app's public repository: they create a **draft**
  release there (drafts are private), and `scripts/publish-build.sh <version>` (run by the owner
  or an agent with `gh`) copies its files to `lsuite-builds` as `<app>-v<version>` and deletes the
  draft. nori and folio (built by kimchi's suite workflow) are copied from its artifacts the same
  way.

## API (served by the site, `builds.js`)

Every route needs an app token (`Authorization: Bearer <token>` from `~/.lsuite/account.json`):
any account, Free included. Without one: 401 `authentication_error`, "Sign in to lsuite to get
the apps: the account is free." Unknown app: 404. Errors in the shape of AI.md.

| Route | Does |
| --- | --- |
| `GET /api/apps/<app>/latest` | The app's newest release: `{app, version, tag, notes, files: [{name, size}], manifest, sha256sums, sha256sumsSig}`: `manifest` is the release's `latest.json` with every `url` rewritten to the file route below (null when the release has none); `sha256sums` and `sha256sumsSig` are the texts of `SHA256SUMS` and `SHA256SUMS.sig` (null when absent). Cached 5 minutes. |
| `GET /api/apps/<app>/latest.json` | Just the rewritten `latest.json` (for the updaters of kimchi, nori and folio, which read that format). 404 when the release has none. |
| `GET /api/apps/<app>/releases/latest` | The release in the GitHub API's shape, `{tag_name, name, body, assets: [{name, size, browser_download_url}]}`, with `browser_download_url` on the file route (for the updaters of ryolune and zenith, which read GitHub's API). `tag_name` is `v<version>` (`v0.16.0`), as the app's own releases were tagged, so those updaters parse it unchanged; the builds tag (`ryolune-v0.16.0`) stays in the `browser_download_url` paths and in `/latest`'s `tag`. |
| `GET /api/apps/<app>/files/<tag>/<name>` | 302 to a short-lived download address of that file (GitHub's signed URL; the token never leaves the server). Only files of that app's releases. |

`GET /api/apps` (the list the lsuite app shows) is unchanged and public.

## The site

App pages no longer have download buttons: each says "Get <app> in the lsuite app" and links to
`/launcher`. `/<app>/download[/<platform>]` is a 302 to `/launcher`. The lsuite app's own
downloads (`/launcher/download/<platform>`) don't change.

## The lsuite app (launcher 0.2.0)

Apps' installs and updates read `GET /api/apps/<app>/latest` and download through the file route,
checking signatures exactly as before. Signed out, the Apps area asks to sign in first ("Your free
lsuite account gets you every app").

## In each app

The updater (STANDARD.md section 3) asks lsuite.xyz instead of GitHub, with the account's token:
kimchi, nori and folio fetch `<server>/api/apps/<app>/latest.json`; ryolune and zenith fetch
`<server>/api/apps/<app>/releases/latest` and its `SHA256SUMS(.sig)` files. `<server>` is
`LSUITE_ACCOUNT_SERVER` or the account's server, else `https://lsuite.xyz`; the token comes from
`~/.lsuite/account.json` (`LSUITE_HOME`), sent as `Authorization: Bearer`. Signed out, the update
check says "Sign in to lsuite (in the lsuite app) to get updates" instead of failing. The existing
`<APP>_UPDATE_URL` overrides keep working for tests. Release workflows make a draft
release, published to `ludovic111/lsuite-builds` by `scripts/publish-build.sh` (above).
