// The apps' builds (DISTRIBUTION.md): the five apps come only through the lsuite app, with a free
// lsuite account. Their builds live in the private repository `ludovic111/lsuite-builds`, one
// release per app version tagged `<app>-v<version>`; this serves them to signed-in apps with the
// server's read-only `LSUITE_BUILDS_TOKEN`, which never leaves the server.
//
//   GET /api/apps/<app>/latest            the newest release, its rewritten latest.json and checksums
//   GET /api/apps/<app>/latest.json       the rewritten latest.json alone (kimchi, nori, folio)
//   GET /api/apps/<app>/releases/latest   the release in GitHub's shape (ryolune, zenith)
//   GET /api/apps/<app>/files/<tag>/<name>  302 to GitHub's short-lived download address
//
// Dependency-free. Options for tests: `fetch`, `api` (GitHub's base URL), `ttl`, `now`.

export const BUILDS_REPO = 'ludovic111/lsuite-builds';
export const BUILD_APPS = ['ryolune', 'kimchi', 'zenith', 'nori', 'folio'];

const API_HEADERS = { 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff', 'Referrer-Policy': 'no-referrer' };
/** The small files read on the server (`latest.json`, `SHA256SUMS`, `SHA256SUMS.sig`), at most this big. */
const SMALL_FILE = 1024 * 1024;
const TEXTS = { manifest: 'latest.json', sha256sums: 'SHA256SUMS', sha256sumsSig: 'SHA256SUMS.sig' };

export const SIGN_IN = 'Sign in to lsuite to get the apps: the account is free.';
export const NOT_SET_UP = "App downloads aren't set up on this server yet.";

class BuildsError extends Error {
  constructor(status, type, message) {
    super(message);
    this.status = status;
    this.type = type;
  }
}
const upstream = (what) => new BuildsError(502, 'api_error', `The app builds couldn't be read from GitHub (${what}). Try again in a moment.`);
const missing = (message) => new BuildsError(404, 'not_found_error', message);

/** `1.2.3` or `1.2.3-rc.1` as comparable parts, or null. */
function parseSemver(text) {
  const m = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(String(text));
  return m ? { core: [Number(m[1]), Number(m[2]), Number(m[3])], pre: m[4] ? m[4].split('.') : [] } : null;
}

/** Semver order: negative when `a` comes before `b`. A pre-release comes before its release. */
export function compareSemver(a, b) {
  const x = parseSemver(a);
  const y = parseSemver(b);
  for (let i = 0; i < 3; i++) if (x.core[i] !== y.core[i]) return x.core[i] - y.core[i];
  if (!x.pre.length || !y.pre.length) return y.pre.length - x.pre.length;
  for (let i = 0; i < Math.max(x.pre.length, y.pre.length); i++) {
    const [p, q] = [x.pre[i], y.pre[i]];
    if (p === undefined) return -1;
    if (q === undefined) return 1;
    const [pn, qn] = [/^\d+$/.test(p), /^\d+$/.test(q)];
    if (pn && qn && Number(p) !== Number(q)) return Number(p) - Number(q);
    if (pn !== qn) return pn ? -1 : 1;
    if (p !== q) return p < q ? -1 : 1;
  }
  return 0;
}

/** The version a builds tag names (`kimchi-v0.10.0` → `0.10.0`), or null when it isn't that app's or isn't semver. */
export function buildVersion(app, tag) {
  const prefix = `${app}-v`;
  const tagText = String(tag ?? '');
  if (!tagText.startsWith(prefix)) return null;
  const version = tagText.slice(prefix.length);
  return parseSemver(version) ? version : null;
}

/** The file name a download address ends with (`…/download/v1/kimchi_x64.dmg` → `kimchi_x64.dmg`). */
function fileNameOf(url) {
  try {
    const last = new URL(url).pathname.split('/').pop();
    return last ? decodeURIComponent(last) : null;
  } catch {
    return null;
  }
}

/** A copy of `value` with every string `url` (at any depth) through `rewrite`. */
function rewriteUrls(value, rewrite) {
  if (Array.isArray(value)) return value.map((v) => rewriteUrls(v, rewrite));
  if (!value || typeof value !== 'object') return value;
  const out = {};
  for (const [k, v] of Object.entries(value)) out[k] = k === 'url' && typeof v === 'string' ? rewrite(v) : rewriteUrls(v, rewrite);
  return out;
}

function json(res, req, status, body, headers = {}) {
  const text = JSON.stringify(body);
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', 'Content-Length': Buffer.byteLength(text), ...API_HEADERS, ...headers });
  res.end(req.method === 'HEAD' ? undefined : text);
}

/** An error in the shape of AI.md (`{type: "error", error: {type, message}}`). */
function fail(res, req, status, type, message) {
  json(res, req, status, { type: 'error', error: { type, message } });
}

/**
 * The builds service. `token`: `LSUITE_BUILDS_TOKEN` (unset: every route answers 503). `auth(req)`:
 * the account of an app's token, or null (the site's cookie isn't one). `origin(req)`: the public
 * origin download addresses are written on. `repo`, `apps`, `api`, `fetch`, `ttl` (ms), `now`.
 */
export function createBuilds(options = {}) {
  const token = String(options.token ?? '').trim();
  const repo = options.repo ?? BUILDS_REPO;
  const apps = options.apps ?? BUILD_APPS;
  const api = String(options.api ?? 'https://api.github.com').replace(/\/+$/, '');
  const fetchImpl = options.fetch ?? fetch;
  const ttl = options.ttl ?? 5 * 60 * 1000;
  const now = options.now ?? Date.now;
  const auth = options.auth ?? (() => null);
  const originOf = options.origin ?? ((req) => `http://${String(req.headers.host ?? 'localhost').replace(/[^\w.:-]/g, '')}`);
  const timeout = options.timeoutMs ?? 8000;
  const headers = (accept) => ({ Accept: accept, Authorization: `Bearer ${token}`, 'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'lsuite-site' });

  /** Values kept `ttl` ms, one request in flight per key; on a failed refresh the last value serves. */
  const cache = new Map();
  function cached(key, load) {
    const hit = cache.get(key);
    if (hit?.pending) return hit.pending;
    if (hit && 'value' in hit && now() - hit.at < ttl) return Promise.resolve(hit.value);
    const pending = load().then(
      (value) => (cache.set(key, { at: now(), value }), value),
      (err) => {
        if (hit && 'value' in hit) return cache.set(key, hit), hit.value;
        cache.delete(key);
        throw err;
      },
    );
    cache.set(key, { ...(hit ?? {}), pending });
    return pending;
  }

  /** Every release of the builds repository (first 100, newest first as GitHub lists them). */
  const releases = () =>
    cached('releases', async () => {
      let res;
      try {
        res = await fetchImpl(`${api}/repos/${repo}/releases?per_page=100`, { headers: headers('application/vnd.github+json'), signal: AbortSignal.timeout(timeout) });
      } catch (err) {
        throw upstream(err.name === 'TimeoutError' ? 'timed out' : 'unreachable');
      }
      if (!res.ok) throw upstream(`HTTP ${res.status}`);
      const list = await res.json().catch(() => null);
      if (!Array.isArray(list)) throw upstream('unexpected answer');
      return list;
    });

  /** The app's newest published release (no drafts, no pre-releases), by the semver of its tag; null when none. */
  async function latestRelease(app) {
    let best = null;
    for (const r of await releases()) {
      const version = buildVersion(app, r?.tag_name);
      if (!version || r.draft || r.prerelease) continue;
      if (!best || compareSemver(version, best.version) > 0) best = { release: r, version };
    }
    return best;
  }

  /** The text of a small asset (`latest.json`, `SHA256SUMS`, its signature), redirects followed here without the token. */
  const assetText = (asset) =>
    cached(`asset:${asset.id}`, async () => {
      if (Number(asset.size) > SMALL_FILE) throw upstream(`${asset.name} is too large`);
      let url = `${api}/repos/${repo}/releases/assets/${asset.id}`;
      let init = { headers: headers('application/octet-stream'), redirect: 'manual', signal: AbortSignal.timeout(timeout) };
      for (let hop = 0; hop < 5; hop++) {
        let res;
        try {
          res = await fetchImpl(url, init);
        } catch (err) {
          throw upstream(err.name === 'TimeoutError' ? 'timed out' : 'unreachable');
        }
        if (res.status >= 300 && res.status < 400 && res.headers.get('location')) {
          // The signed address needs no token: it must never get one.
          url = new URL(res.headers.get('location'), url).href;
          init = { headers: { 'User-Agent': 'lsuite-site' }, redirect: 'manual', signal: AbortSignal.timeout(timeout) };
          continue;
        }
        if (!res.ok) throw upstream(`HTTP ${res.status} for ${asset.name}`);
        const body = await res.arrayBuffer();
        if (body.byteLength > SMALL_FILE) throw upstream(`${asset.name} is too large`);
        return new TextDecoder().decode(body);
      }
      throw upstream(`too many redirects for ${asset.name}`);
    });

  /** The app's newest release as the API describes it, before addresses are put on an origin. */
  async function latest(app) {
    const found = await latestRelease(app);
    if (!found) throw missing(`No ${app} build has been published yet.`);
    const { release, version } = found;
    const assets = Array.isArray(release.assets) ? release.assets : [];
    const texts = {};
    await Promise.all(
      Object.entries(TEXTS).map(async ([key, name]) => {
        const asset = assets.find((a) => a.name === name);
        texts[key] = asset ? await assetText(asset) : null;
      }),
    );
    let manifest = null;
    if (texts.manifest !== null) {
      try {
        manifest = JSON.parse(texts.manifest);
      } catch {
        throw upstream(`${app}'s latest.json isn't valid JSON`);
      }
    }
    return { release, version, assets, manifest, sha256sums: texts.sha256sums, sha256sumsSig: texts.sha256sumsSig };
  }

  const fileUrl = (origin, app, tag, name) => `${origin}/api/apps/${app}/files/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`;
  const rewritten = (manifest, origin, app, tag) => rewriteUrls(manifest, (url) => fileUrl(origin, app, tag, fileNameOf(url) ?? url));

  /** 302 to GitHub's signed address of one file of one of the app's releases. */
  async function file(req, res, app, tag, name) {
    if (!buildVersion(app, tag)) throw missing(`${tag} isn't a release of ${app}.`);
    const release = (await releases()).find((r) => r?.tag_name === tag && !r.draft);
    if (!release) throw missing(`${app} has no release ${tag}.`);
    const asset = (release.assets ?? []).find((a) => a.name === name);
    if (!asset) throw missing(`${app} ${tag} has no file called ${name}.`);
    let up;
    try {
      up = await fetchImpl(`${api}/repos/${repo}/releases/assets/${asset.id}`, { headers: headers('application/octet-stream'), redirect: 'manual', signal: AbortSignal.timeout(timeout) });
    } catch (err) {
      throw upstream(err.name === 'TimeoutError' ? 'timed out' : 'unreachable');
    }
    // Never the file itself through here: only GitHub's short-lived address.
    const location = up.status >= 300 && up.status < 400 ? up.headers.get('location') : null;
    await up.body?.cancel().catch(() => {});
    if (!location) throw upstream(`HTTP ${up.status} for ${name}`);
    res.writeHead(302, { Location: location, ...API_HEADERS });
    res.end();
  }

  /** The routes under `/api/apps/<app>/…`; returns false for any other path. */
  async function handle(req, res, url) {
    const raw = String(req.url ?? '').split('?')[0];
    const m = /^\/api\/apps\/([^/]+)\/(latest|latest\.json|releases\/latest|files\/([^/]+)\/([^/]+))$/.exec(raw);
    if (!raw.startsWith('/api/apps/')) return false;
    try {
      if (!m) throw missing(`No API at ${req.method} ${url?.pathname ?? raw}.`);
      if (req.method !== 'GET' && req.method !== 'HEAD') return fail(res, req, 405, 'invalid_request_error', 'Use GET.'), true;
      if (!(await auth(req))) return fail(res, req, 401, 'authentication_error', SIGN_IN), true;
      const app = m[1];
      if (!apps.includes(app)) throw missing(`There's no lsuite app called ${JSON.stringify(app)}. The apps: ${apps.join(', ')}.`);
      if (!token) return fail(res, req, 503, 'api_error', NOT_SET_UP), true;
      const origin = originOf(req);
      if (m[3] !== undefined) {
        let tag, name;
        try {
          [tag, name] = [decodeURIComponent(m[3]), decodeURIComponent(m[4])];
        } catch {
          throw missing('No such file.');
        }
        await file(req, res, app, tag, name);
        return true;
      }
      const found = await latest(app);
      const tag = found.release.tag_name;
      const manifest = found.manifest === null ? null : rewritten(found.manifest, origin, app, tag);
      if (m[2] === 'latest.json') {
        if (manifest === null) throw missing(`${app} ${found.version} has no latest.json.`);
        return json(res, req, 200, manifest), true;
      }
      if (m[2] === 'releases/latest') {
        return json(res, req, 200, {
          // `v<version>`, as the app's own releases were tagged, so ryolune's and zenith's updaters
          // parse it unchanged; the builds tag stays in the download addresses.
          tag_name: `v${found.version}`,
          name: found.release.name ?? tag,
          body: found.release.body ?? '',
          assets: found.assets.map((a) => ({ name: a.name, size: a.size, browser_download_url: fileUrl(origin, app, tag, a.name) })),
        }), true;
      }
      return json(res, req, 200, {
        app,
        version: found.version,
        tag,
        notes: found.release.body ?? '',
        files: found.assets.map((a) => ({ name: a.name, size: a.size })),
        manifest,
        sha256sums: found.sha256sums,
        sha256sumsSig: found.sha256sumsSig,
      }), true;
    } catch (err) {
      if (!(err instanceof BuildsError)) throw err;
      if (res.headersSent) return res.destroy(), true;
      return fail(res, req, err.status, err.type, err.message), true;
    }
  }

  /** The app's newest version in the builds (the pages' `%VERSION:<app>%`), or null when unknown or unreachable. */
  async function version(app) {
    if (!token) return null;
    // A page waits for this: past `versionWaitMs` it shows the fallback (the lookup still fills the cache).
    const wait = new Promise((resolve) => setTimeout(resolve, options.versionWaitMs ?? 4000, null).unref?.());
    try {
      return await Promise.race([latestRelease(app).then((found) => found?.version ?? null), wait]);
    } catch {
      return null;
    }
  }

  return { handle, version, configured: !!token };
}
