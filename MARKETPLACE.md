# The lsuite Marketplace: plugins made by the people who use lsuite

Decided on 2026-10-07: **lsuite Pass** (PASS.md) includes a marketplace of plugins for the five
apps, made by the people who use them and by lsuite itself. Anyone with an lsuite account can
publish; installing from the marketplace comes with a paid Pass plan. Plugins are the bundles of
PLUGINS.md (Rust, frozen `repr(C)` ABI per app), so **every version is reviewed by lsuite before
anyone can install it**: a plugin is native code that runs inside the app.

For now this is a demo like the rest of the Pass: no payment is taken, and creators aren't paid
yet (a revenue share is for when payments are live).

## Listings

A listing is one plugin id (reverse-DNS, from `plugin.toml`), owned by the account that first
published it. Ids starting `xyz.lsuite.` are lsuite's own. A listing shows its newest approved
version:

```json
{ "id": "com.example.tape-warmth", "app": "ryolune", "name": "Tape warmth", "kind": "effect",
  "description": "One line.", "version": "1.2.0", "abi": 1,
  "author": { "name": "Ada", "verified": false },
  "platforms": { "macos-arm64": { "size": 812345, "sha256": "…" }, "linux-x86_64": { … } },
  "downloads": 42, "updatedAt": "2026-10-07T12:00:00Z", "notes": "What changed." }
```

`author.verified` is true for lsuite's own plugins (published by an admin account). Platforms are
`macos-arm64`, `macos-x86_64`, `linux-x86_64`, `windows-x86_64`: a version is listed with the
platforms it was uploaded for.

## Bundle files

One `.tar.gz` per platform, holding **one top folder** that is the bundle: `plugin.toml` and the
library named in its `[library]` table for that platform (`.dylib`, `.so`, `.dll`), plus any
files the plugin needs. At most 15 MB per file in the demo (`LSUITE_MARKET_MAX_FILE`); all
marketplace files together at most 80 MB in the demo (`LSUITE_MARKET_TOTAL`); the cloud's disk
guard applies too (CLOUD.md). The server checks the archive is gzip, that `plugin.toml` is there
and that its `id`, `version` and `app` match the submission.

## API (served by the site, `marketplace.js` through `ai.js`)

Same auth and error shape as AI.md and CLOUD.md: an app token (`Authorization: Bearer`) or, for
the GET routes and the account page, the site's session cookie.

| Route | Who | Does |
| --- | --- | --- |
| `GET /api/marketplace` | anyone | `{plugins: [listing…], apps: ["ryolune",…], pass: {required: true, plans: ["plus","pro","studio"]}}`, approved only, newest first; `?app=` filters. |
| `GET /api/marketplace/plugins/<id>` | anyone | One listing, plus `versions: [{version, notes, approvedAt, platforms}]`. |
| `GET /api/marketplace/plugins/<id>/download?platform=<p>[&version=<v>]` | token, **paid plan** | The `.tar.gz` (`ETag: "<sha256>"`, `X-Lsuite-Sha256`, `Content-Length`); counts a download. Free → 403 `plan_required` ("The marketplace comes with lsuite Pass."); no such platform → 404. |
| `POST /api/marketplace/submit` | token or session | `{id, app, name, kind, version, abi, description, notes?}` → 201 `{submission}`: a pending version. Only the listing's owner publishes new versions; an approved version can't be replaced (publish a new version); `xyz.lsuite.*` only for admins. |
| `PUT /api/marketplace/submit/<id>/<version>/<platform>` | token | The bundle `.tar.gz` for that pending version (`Content-Length` required, optional `X-Lsuite-Sha256`). → `{submission}`. |
| `GET /api/marketplace/mine` | token or session | The account's submissions: `[{id, version, app, name, status: "pending"|"approved"|"rejected", note, platforms, submittedAt}]`. |
| `GET /api/marketplace/review` | admin | Pending submissions, with their manifests and file hashes. |
| `POST /api/marketplace/review` | admin | `{id, version, decision: "approve"|"reject", note?}`. Approving lists it; rejecting keeps the note for the author. |
| `GET /api/marketplace/review/<id>/<version>/<platform>` | admin | Downloads a pending file to look at it. |

Admins are the accounts whose email is in `LSUITE_ADMIN_EMAILS` (comma-separated). An admin's own
submissions are approved at once and marked `verified`.

Errors besides AI.md's: 400 `invalid_plugin` (manifest or archive doesn't match), 403
`plan_required`, 403 `not_owner`, 404 `not_found_error`, 409 `conflict_error` (version already
approved), 411 `length_required`, 413 `request_too_large`, 507 `storage_full`.

## On the computer

The launcher's **Marketplace** area (and `lsuite-cli market.*`, `lsuite-mcp`): browse by app,
install (with a Pass plan), update, remove; publish a bundle you or your agent built
(`market.publish path=<bundle folder>`, after `plugin.publishLocal` in the app). Installing checks
the file's SHA-256 against the listing, unpacks it into `~/.lsuite/plugins/<app>/<id>/`
(PLUGINS.md) and asks a running app to `plugin.rescan` through its CLI; the launcher records it in
`~/.lsuite/launcher/plugins.json`. Agents need `agent.install` to install and `agent.publish`
(off by default) to publish.

## As built (2026-10-07, `marketplace.js`)

The details the table above leaves open, as the server answers them (`test/marketplace.test.js`
covers each):

- **Shapes.** `GET /mine` and `GET /review` answer a bare JSON array. A submission (in `/mine`,
  `{submission}` and the review list) is `{id, version, app, name, kind, abi, description, notes,
  status, note, platforms: {<p>: {size, sha256, uploadedAt}}, submittedAt, reviewedAt, verified,
  downloads}`; the review list adds `author: {name, email, verified}` and, per platform,
  `manifest` (`plugin.toml`'s `id, name, version, app, kind, abi, description, authors, library`),
  `files` (paths in the bundle folder, 200 at most) and `fileCount`. `GET /plugins/<id>` is the
  listing's own fields plus `versions` (approved, newest first). `POST /review` → `{submission}`.
- **Order.** A listing shows its highest approved version (semantic-version order) that has at
  least one file; the catalogue lists the most recently approved first.
- **Submit.** `id`: lowercase reverse-DNS (`[a-z0-9][a-z0-9_-]*` between dots, two parts at least,
  128 characters); `version`: semantic (`1.2.0`, `1.2.0-beta.1`); `app`: one of the five; `kind`:
  `[a-z][a-z0-9-]{0,31}`; `abi`: a positive integer; `name` ≤ 60, `description` ≤ 300 (one line),
  `notes` ≤ 2000. Anything else → 400 `invalid_request_error`. Submitting a pending or rejected
  version again updates it and makes it pending (201 too; a rejected one has no files left, so it is
  uploaded again). An id already listed for another app → 409 `conflict_error`. `xyz.lsuite.*` by a
  non-admin → 403 `not_owner`. An admin's submission is approved at once and listed once it has a
  file; the listing's `verified` follows whether its last publisher is an admin.
- **Upload.** App token only (the session can't upload). Unknown platform → 400
  `invalid_request_error`; no such submission → 404; a rejected or approved version → 409 (an admin
  may add a missing platform to an approved version of theirs, never replace one); a wrong
  `X-Lsuite-Sha256` → 400 `checksum_mismatch`. The archive: gzip; tar (ustar, pax and GNU long names;
  `./` prefixes are fine); exactly one top folder; only files and folders (links, devices and paths
  with `..`, `\` or a leading `/` are refused); `plugin.toml` (64 KB at most) whose `id`, `version`
  and `app`, and `abi` when it has one, match; `[library]` `macos` / `linux` / `windows` (by the
  platform) naming a file in the folder; at most 10 times the largest file once unpacked. A refused
  bundle leaves nothing behind. Replacing a pending platform's file frees its room first.
- **Download.** `platform` missing → 400; an unknown or missing platform, version or id → 404 (the
  404 comes before the plan check). `Content-Type: application/gzip`, `Content-Disposition:
  attachment; filename="<id>-<version>-<platform>.tar.gz"`. `If-None-Match` with the ETag → 304;
  neither a 304 nor `HEAD` counts as a download. The session cookie works too (GET).
- **Review.** Not an admin → 403 `permission_error`. Approving needs a pending version with at
  least one file (400 otherwise; 409 if it isn't pending). Rejecting drops the version's files and
  keeps the note; rejecting an approved version takes it down. Session posts must come from the
  site itself (else 403 `permission_error`, as for the account routes).
- **Storage.** `<LSUITE_DATA_DIR>/marketplace/` (`production-marketplace/` in production; a
  temporary folder removed at exit without a data dir): `index.json` (listings, versions,
  download counts; 0600, written atomically) and `files/<sha256>`, one per distinct bundle, dropped
  when no version names it, leftovers swept at start. Uploads stream to `.tmp/`. With lsuite
  Cloud's object store (`LSUITE_CLOUD_S3_*`, CLOUD.md) the files go there instead
  (`[<prefix>/]<marketplace|production-marketplace>/files/<sha256>`) and the 80 MB total no longer
  applies (the largest file and the disk guard still do).
