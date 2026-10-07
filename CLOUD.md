# lsuite Cloud: storage that comes with lsuite AI

Started on 2026-10-07 at the owner's request: a paid lsuite AI plan also includes cloud storage,
managed from the **lsuite** launcher (`launcher/`, see its README). The apps never need it:
documents stay plain files on disk (STANDARD.md, section 4), and nothing goes to the cloud unless
the person puts it there. Still no telemetry.

**For now this is a demo**, like lsuite AI: no payment is taken, and every account's storage is
capped small (below) because demo accounts need no verified email.

## Storage per plan

Decided on 2026-10-07 (the owner left it to the demo's builder: the prices are unchanged).
These sizes apply once lsuite AI takes payments and the files live in an object store.

| Plan | Price | Cloud storage | Largest file |
| --- | --- | --- | --- |
| Free | 0 | none (files already there can still be listed, downloaded and deleted) | — |
| Plus | $12 / month | 50 GB | 5 GB |
| Pro | $29 / month | 250 GB | 5 GB |
| Studio | $79 / month | 1 TB | 5 GB |

Sizes are decimal: 1 GB is 10⁹ bytes, 1 TB 10¹² (`PLANS[].storage` in `ai.js`, `MAX_FILE` in
`cloud.js`).

Demo mode caps every plan at **100 MB per account** and **25 MB per file**
(`LSUITE_CLOUD_DEMO_QUOTA`, `LSUITE_CLOUD_DEMO_MAX_FILE`, bytes), and all demo accounts together
at **300 MB** (`LSUITE_CLOUD_DEMO_TOTAL`): the site's Railway volume is 500 MB and also holds the
accounts. Whatever the caps, an upload is refused (507 `storage_full`) when it would leave less
than **100 MB free** on the disk the uploads stream to (`LSUITE_CLOUD_DISK_RESERVE`), so cloud
files can never stop the accounts from being saved. Production applies the plans' own sizes once blobs go to
an object store (below); on the data dir alone it keeps the demo caps.
`GET /api/ai/plans` gives each plan's `storage` (bytes) and `storageLabel` ("50 GB", "None" on
Free); in demo mode also `cloudDemo: {quota, maxFile}`. Whatever applies to an account is in
`GET /api/cloud` (`quota`, `maxFile`). Each account also holds at most 100,000 files and folders.
The plan cards on `/ai` and `/account` show the storage ("50 GB lsuite Cloud").


## Paths

A file is named by its path: `/`-separated segments, without a leading or trailing `/`
(`Projects/ryolune/demo.ryolune`). In URLs each segment is percent-encoded and decoded on its own
(so `%2F` is refused, never a separator; a bad escape or invalid UTF-8 is refused too); in JSON
bodies a path is the plain string. A path is 1–1024
bytes of UTF-8; a segment is 1–255 bytes and is not `.` or `..`; no `\`, no control characters,
no leading or trailing spaces in a segment. Paths are case-sensitive. Folders exist when a file is
in them, or when created on their own (empty folders). A path can't be both a file and a folder.

## API (served by the site, `cloud.js` through `ai.js`)

Every route takes the app token like `/api/ai` (`Authorization: Bearer <token>` or
`x-api-key`). The GET routes also accept the site's session cookie (the account page shows the
usage). Errors are in the same shape as AI.md's: `{type: "error", error: {type, message, …}}`,
the message one line an app can show as it is.

| Route | Does |
| --- | --- |
| `GET /api/cloud` | `{plan, planName, quota, used, files, folders, maxFile, demo, manageUrl}`. `quota` and `maxFile` are 0 on Free; `files` and `folders` are counts (every folder, its own or holding something). |
| `GET /api/cloud/files` | Everything, flat: `{files: [{path, size, sha256, modifiedAt}], folders: [{path, createdAt}], used, quota}` (sorted by path; `folders` lists only folders created on their own). |
| `GET /api/cloud/files/<path>` | The file's bytes (`application/octet-stream`, `Content-Length`, `ETag: "<sha256>"`, `X-Lsuite-Modified`, `Content-Disposition: attachment`). `HEAD` works too. `If-None-Match` with its ETag → 304. A folder → 409. |
| `PUT /api/cloud/files/<path>` | Uploads the body as that file (replacing one already there). `Content-Length` is required. Optional `X-Lsuite-Sha256` (hex; the server checks it), `X-Lsuite-Modified` (ISO 8601 with a time zone; else now), `If-None-Match: *` (refuse to replace; an ETag refuses to replace that content). Parent folders appear by themselves. → 201 (new) or 200 (replaced) `{file: {path, size, sha256, modifiedAt}, used, quota}`. |
| `DELETE /api/cloud/files/<path>` | Deletes a file, or a folder and everything in it. → `{deleted, used, quota}` (files deleted; 0 for an empty folder). |
| `POST /api/cloud/folders` | `{path}` → 201 `{folder: {path, createdAt}}` (200 if it already exists; a folder that only held files becomes one of its own, so it stays when they go). |
| `POST /api/cloud/move` | `{from, to, overwrite?}` moves or renames a file or a folder (with its content), into new parent folders if needed. With `overwrite`, a file replaces the file at `to` (its size is freed) and a folder merges into the folder at `to`; a file onto a folder (or the reverse) is always a conflict, and a merge that would make one moves nothing. `from` = `to` changes nothing. → `{moved, used, quota}` (files moved). |

Statuses and `error.type`s, besides AI.md's:

| Status | `error.type` | When | Extra fields |
| --- | --- | --- | --- |
| 400 | `invalid_path` | The path breaks the rules above. | `path` |
| 400 | `checksum_mismatch` | The body doesn't match `X-Lsuite-Sha256`. Nothing is saved. | |
| 400 | `invalid_request_error` | `X-Lsuite-Sha256` isn't 64 hex digits, `X-Lsuite-Modified` isn't ISO 8601, a JSON body is malformed, or the body ended before its `Content-Length`. | |
| 401 | `authentication_error` | No token (or, for a GET, no session), or a revoked one. | |
| 403 | `plan_required` | Upload, folder or move on Free: "lsuite Cloud comes with an lsuite AI plan." | `manage_url`, `plan` |
| 404 | `not_found_error` | No such file or folder. | |
| 409 | `conflict_error` | A file where a folder is wanted (or the reverse), `to` already exists without `overwrite`, a folder moved into itself. | `path` |
| 411 | `length_required` | `PUT` without `Content-Length`. | |
| 412 | `precondition_failed` | `If-None-Match: *` and the file exists. | |
| 413 | `request_too_large` | The file is larger than the plan's largest file. | `max_file` |
| 405 | `invalid_request_error` | A method the route doesn't take (`Allow` lists them). | |
| 507 | `storage_full` | The upload doesn't fit: the plan's quota (also after a downgrade left the account over it), the demo's total, or the 100,000 files and folders. | `used`, `quota`, `manage_url`, `plan` |

Free accounts can list, download and delete; upload, new folder and move answer `plan_required`.

`GET /api/account/me` adds `cloud: {used, quota, files}`.

## Storage on the server

`<LSUITE_DATA_DIR>/cloud/<userId>/` (production: `production-cloud/`, never shared with demo
data): `index.json` (`{format: 1, files: {path: {sha256, size, modifiedAt}}, folders: {path:
{createdAt}}}`, 0600, written atomically) and `blobs/<sha256>`, one per distinct content (a file
copied under two names is stored once and counted once per name). Without `LSUITE_DATA_DIR` the
files live in a temporary folder that is removed with the process (exit, SIGINT, SIGTERM). Uploads
stream to `.tmp/` (emptied at start) while being hashed and counted. As `Content-Length` is
required, a file larger than the largest file or than the room left (the quota, the demo's
total, uploads already streaming counted in) is refused before a byte is read, and a body that
runs past its `Content-Length` stops at once, so the server never holds a whole file in memory.
One user's changes run one at a time, and each re-checks room and conflicts when its turn comes.
A blob goes when no path names it; blobs left by an interrupted write are swept when the account
is next loaded.

### The object store

1 TB plans cannot live on a Railway volume, so the blobs can go to any S3-compatible object store
instead (AWS S3, Cloudflare R2, Railway buckets, MinIO…), set with the variables below; the
index stays in the data dir (it is small), so the store needs `LSUITE_DATA_DIR`. `cloud.js` speaks
to it itself (AWS Signature V4 with `node:crypto`, no SDK):

- Keys mirror the data dir: `[<prefix>/]cloud/<userId>/blobs/<sha256>` (`production-cloud/` in
  production), one object per distinct content of each account.
- Upload: the body still streams to `.tmp/` while being hashed and counted; then the temp file is
  streamed to the store in one `PUT` with its `Content-Length` and the SHA-256 already computed as
  `x-amz-content-sha256` (so the store checks the bytes too), never held in memory. Content one of
  the account's paths already names isn't sent again. The `PUT` runs in the account's turn, so no
  delete can drop the blob before the index names it (the account's other changes wait for it).
- Download: the store's `GET` is streamed to the client (`HEAD` and a 304 never reach the store).
- Delete: a `DELETE` once no path names the blob. If the store refuses it, the file is still
  deleted and the blob only left over; when an account is next loaded, the blobs no path names are
  listed (`ListObjectsV2`) and deleted before its first change.
- A store that fails or doesn't answer (60 s without a byte) → **502** `api_error`, one line:
  "lsuite Cloud's storage can't be reached right now (the object store answered 503 SlowDown). Try
  again in a moment." A failed upload saves nothing.
- Quotas: production with a store applies the plans' sizes (50 GB, 250 GB, 1 TB, largest file
  5 GB) and no total; demo mode keeps the demo caps whatever the backend; production without a
  store keeps the demo caps.

| Variable | Value |
| --- | --- |
| `LSUITE_CLOUD_S3_ENDPOINT` | The service's URL: `https://t3.storageapi.dev` (Railway), `https://<account>.r2.cloudflarestorage.com` (R2), `https://s3.<region>.amazonaws.com` (AWS). |
| `LSUITE_CLOUD_S3_BUCKET` | The bucket's name. |
| `LSUITE_CLOUD_S3_ACCESS_KEY_ID` | An access key that can read, write, delete and list the bucket. |
| `LSUITE_CLOUD_S3_SECRET_ACCESS_KEY` | Its secret. |
| `LSUITE_CLOUD_S3_REGION` | Optional: the AWS endpoint's region (else `us-east-1`); `auto` for any other endpoint. |
| `LSUITE_CLOUD_S3_PREFIX` | Optional: a folder for every key (`lsuite`). |
| `LSUITE_CLOUD_S3_PATH_STYLE` | Optional: `true` for `<endpoint>/<bucket>/<key>` URLs (MinIO, or a bucket whose credentials say "path"); else `<bucket>.<endpoint host>/<key>`. |

Endpoint, bucket and both keys go together: a partial setup stops the server at start rather than
quietly use the volume. The keys are secrets: set them on the host, never in Git, issues or chat.

**Production activation** (with AI.md's): create a bucket (on Railway: a bucket in the same project as
`lsuite-site`; its Credentials tab gives the endpoint, bucket, keys, region and URL style), then set the
variables above privately on the Railway `lsuite-site` service, as references to the bucket's own
variables where the host offers them (`${{<bucket>.ENDPOINT}}`…), and `LSUITE_CLOUD_S3_PATH_STYLE`
if its URL style is "path". Restart, upload a file from the launcher and check that the object
appears under `production-cloud/` in the bucket. Accounts already holding files on the volume keep
their index, but their blobs aren't copied over: move them (`blobs/<sha256>` to the same key in the
store) before switching, or start with an empty `production-cloud/`.

## The apps (`GET /api/apps`)

What the launcher reads to know which apps exist, served by `server.js` (no auth,
`Cache-Control: public, max-age=300`):

```json
{"apps": [{"id": "ryolune", "name": "ryolune", "kind": "music", "summary": "The DAW your AI can drive.",
  "page": "https://lsuite.xyz/ryolune", "repo": "ludovic111/ryolune", "version": "0.15.3",
  "published": true, "platforms": ["macos-arm64", "macos-x86_64", "windows-x86_64", "windows-zip", "linux-x86_64"]}, …]}
```

`kind` is `music`, `video`, `code`, `image` or `office`; `repo` is the GitHub `owner/name`;
`version` is the latest published release (cached 10 minutes, a fallback when GitHub can't be
reached); `platforms` are what `<origin>/<app>/download/<platform>` takes (README, Routes).

## In the launcher

The launcher signs in like an app (AI.md, loopback), with `/account/connect?app=lsuite`: the page
says "Connect the lsuite launcher", and the account page lists it as "lsuite launcher".

The launcher's **Cloud** area: the plan's storage used, a file browser (folders, upload files or a
folder, download, rename, move, new folder, delete with confirmation), and the same `cloud.*`
commands from `lsuite-cli` and `lsuite-mcp` (`cloud.status`, `cloud.list`, `cloud.upload`,
`cloud.download`, `cloud.mkdir`, `cloud.move`, `cloud.delete`).
