// Dependency-free server for lsuite.xyz. Railway runs `npm start`.
//
// Pages are plain HTML (`index.html`, `<app>/index.html`) with two includes, `<!-- include:nav -->`
// and `<!-- include:foot -->`, filled from `partials/` when served. Everything static lives in
// `assets/`. Old app domains (ryolune.com) answer with a 301 to the app's page here.
import { createServer } from 'node:http';
import { createReadStream } from 'node:fs';
import { readFile, stat } from 'node:fs/promises';
import { dirname, extname, join, normalize, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { createAccounts } from './ai.js';

const ROOT = dirname(fileURLToPath(import.meta.url));
const PORT = Number(process.env.PORT) || 3000;
const HOST = '0.0.0.0';

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.txt': 'text/plain; charset=utf-8',
  '.xml': 'application/xml; charset=utf-8',
  '.woff2': 'font/woff2',
  '.mp4': 'video/mp4',
};

/** The pages, by path. Each app page is `<app>/index.html`. */
export const PAGES = { '/': 'index.html', '/ryolune': 'ryolune/index.html', '/kimchi': 'kimchi/index.html', '/zenith': 'zenith/index.html', '/design': 'design/index.html' };

/** The design system's files, served as they are (design/DESIGN.md explains them). */
const DESIGN_FILES = new Set(['/design/tokens.css', '/design/tokens.json', '/design/preview.js']);

/**
 * Hosts that used to be an app's own site and now point here: every path keeps working under
 * the app's page (ryolune.com/support → lsuite.xyz/ryolune/support, and the hash, which the
 * browser keeps across a redirect, still lands on the same section).
 */
export const MOVED_HOSTS = {
  'ryolune.com': 'ryolune',
  'www.ryolune.com': 'ryolune',
  // The ryolune-site service's own Railway domain, which the ryolune 0.11 app links to.
  'site-production-7751.up.railway.app': 'ryolune',
};

/** The one public address, `LSUITE_CANONICAL_HOST` on the host (lsuite.xyz). Unset: no redirect. */
const canonicalHost = () => String(process.env.LSUITE_CANONICAL_HOST ?? '').trim().toLowerCase();

/** Where a request on another host should go, or null to serve it here. */
export function hostRedirect(host, url, canonical = canonicalHost()) {
  const actual = String(host ?? '').split(':')[0].toLowerCase();
  const target = canonical || 'lsuite.xyz';
  const app = MOVED_HOSTS[actual];
  if (app) {
    const path = url === '/' ? '' : url.startsWith('/?') ? url.slice(1) : url;
    return `https://${target}/${app}${path}`;
  }
  if (!canonical || !actual || actual === canonical || actual === 'localhost' || /^[\d.]+$/.test(actual)) return null;
  return `https://${canonical}${url}`;
}

/** Best guess from the User-Agent. Macs report Intel even on Apple silicon, so default to arm64. */
export function osFor(userAgent = '') {
  if (/Windows/i.test(userAgent)) return 'windows';
  if (/Android|iPhone|iPad/i.test(userAgent)) return null;
  if (/Mac OS X|Macintosh/i.test(userAgent)) return 'macos';
  if (/Linux|X11/i.test(userAgent)) return 'linux';
  return null;
}

// kimchi's file names carry the version (Tauri bundles), so the latest release is looked up.
const KIMCHI = {
  repo: 'ludovic111/kimchi',
  releases: 'https://github.com/ludovic111/kimchi/releases/latest',
  patterns: {
    'macos-arm64': /_aarch64\.dmg$/,
    'macos-x86_64': /_x64\.dmg$/,
    'windows-x86_64': /_x64-setup\.exe$/,
    'windows-msi': /_x64_en-US\.msi$/,
    'linux-appimage': /_amd64\.AppImage$/,
    'linux-deb': /_amd64\.deb$/,
    'linux-rpm': /\.x86_64\.rpm$/,
  },
  byOs: { macos: 'macos-arm64', windows: 'windows-x86_64', linux: 'linux-appimage' },
};

const releaseCache = new Map();
/** A repository's latest published release (`tag`, asset `urls`), cached for ten minutes. */
async function latestRelease(repo) {
  const hit = releaseCache.get(repo);
  if (hit && Date.now() - hit.at < 10 * 60 * 1000) return hit;
  try {
    const res = await fetch(`https://api.github.com/repos/${repo}/releases/latest`, {
      headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'lsuite-site' },
      signal: AbortSignal.timeout(4000),
    });
    if (!res.ok) throw new Error(String(res.status));
    const json = await res.json();
    const release = { at: Date.now(), tag: json.tag_name, urls: json.assets.map((a) => a.browser_download_url) };
    releaseCache.set(repo, release);
    return release;
  } catch {
    return hit ?? { tag: null, urls: [] };
  }
}

// Shown when GitHub cannot be reached. Pages say `%VERSION:<app>%` and get the version of the
// latest published release, so the page never announces a version you cannot download yet.
// ryolune and zenith are coming soon: no page asks for their versions.
const FALLBACK_VERSIONS = { kimchi: '0.1.0' };
const REPOS = { kimchi: 'ludovic111/kimchi' };

/** `{ app: version }` for every app whose version a page asks for. */
export async function appVersions(apps) {
  const out = {};
  await Promise.all(
    apps.map(async (app) => {
      const { tag } = REPOS[app] ? await latestRelease(REPOS[app]) : {};
      out[app] = tag ? tag.replace(/^v/, '') : FALLBACK_VERSIONS[app] ?? '';
    }),
  );
  return out;
}

/** Where `/<app>/download[/<platform>]` sends the visitor. */
export async function downloadTarget(app, wanted, userAgent) {
  // Coming-soon apps aren't offered for download yet: old download links land on their pages.
  if (app === 'ryolune' || app === 'zenith') return `/${app}`;
  if (app === 'kimchi') {
    const platform = wanted || KIMCHI.byOs[osFor(userAgent)];
    const pattern = KIMCHI.patterns[platform];
    if (!pattern) return KIMCHI.releases;
    const url = (await latestRelease(KIMCHI.repo)).urls.find((u) => pattern.test(u));
    return url ?? KIMCHI.releases;
  }
  return null;
}

/**
 * Where "Donate" goes: `LSUITE_DONATION_URL` (https only), else GitHub Sponsors. The ryolune
 * app opens ryolune.com/support, which lands on /ryolune/support, so this can move without a release.
 */
export function supportTarget(donation = process.env.LSUITE_DONATION_URL) {
  try {
    const url = new URL(String(donation ?? ''));
    if (url.protocol === 'https:') return url.href;
  } catch {}
  return 'https://github.com/sponsors/ludovic111';
}

const SECURITY = {
  'X-Content-Type-Options': 'nosniff',
  'Referrer-Policy': 'strict-origin-when-cross-origin',
  'X-Frame-Options': 'DENY',
  'Permissions-Policy': 'camera=(), microphone=(), geolocation=()',
  'Strict-Transport-Security': 'max-age=31536000',
  'Content-Security-Policy':
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' data:; media-src 'self'; object-src 'none'; form-action 'none'; frame-ancestors 'none'; base-uri 'self'",
};

function send(res, status, body, type, cache, req) {
  const etag = status === 200 ? `"${createHash('sha1').update(body).digest('base64url').slice(0, 22)}"` : null;
  if (etag && req?.headers['if-none-match'] === etag) {
    res.writeHead(304, { ETag: etag, 'Cache-Control': cache, Vary: 'Accept-Encoding', ...SECURITY });
    return res.end();
  }
  const gzip =
    /^text\/|json|svg|xml/.test(type) && Buffer.byteLength(body) > 1024 && /\bgzip\b/.test(String(req?.headers['accept-encoding'] ?? ''));
  const payload = gzip ? gzipSync(body) : body;
  res.writeHead(status, {
    'Content-Type': type,
    'Content-Length': Buffer.byteLength(payload),
    'Cache-Control': cache,
    ...(etag ? { ETag: etag } : {}),
    ...(gzip ? { 'Content-Encoding': 'gzip' } : {}),
    Vary: 'Accept-Encoding',
    ...SECURITY,
  });
  res.end(req?.method === 'HEAD' ? undefined : payload);
}

function redirect(res, status, location, cache = 'no-store') {
  res.writeHead(status, { Location: location, 'Cache-Control': cache, ...SECURITY });
  res.end();
}

// `?v=` on a script or stylesheet becomes a hash of that file, so versioned URLs are cached for good.
const stamps = new Map();
async function stampOf(path) {
  const file = normalize(join(ROOT, path));
  if (!file.startsWith(ROOT + sep)) return null;
  try {
    const { mtimeMs } = await stat(file);
    const hit = stamps.get(file);
    if (hit?.mtimeMs === mtimeMs) return hit.stamp;
    const stamp = createHash('sha1').update(await readFile(file)).digest('hex').slice(0, 10);
    stamps.set(file, { mtimeMs, stamp });
    return stamp;
  } catch {
    return null;
  }
}

/** A page as served: includes filled, its app marked current in the nav, versions stamped. */
export async function renderPage(file, origin, versions) {
  let html = await readFile(join(ROOT, file), 'utf8');
  for (const name of ['nav', 'foot']) {
    if (html.includes(`<!-- include:${name} -->`)) {
      html = html.replace(`<!-- include:${name} -->`, (await readFile(join(ROOT, 'partials', `${name}.html`), 'utf8')).trim());
    }
  }
  const app = file.split('/').length > 1 ? file.split('/')[0] : null;
  if (app) html = html.replaceAll(`data-app="${app}"`, `data-app="${app}" aria-current="page"`);
  for (const [whole, quote, path] of [...html.matchAll(/(["'])(\/(?:assets|design)\/[\w./-]+\.(?:js|css))\?v=[\w.-]*\1/g)]) {
    const stamp = await stampOf(path);
    if (stamp) html = html.replaceAll(whole, `${quote}${path}?v=${stamp}${quote}`);
  }
  const apps = [...new Set([...html.matchAll(/%VERSION:(\w+)%/g)].map((m) => m[1]))];
  if (apps.length) {
    const known = versions ?? (await appVersions(apps));
    html = html.replace(/%VERSION:(\w+)%/g, (_, app) => known[app] ?? FALLBACK_VERSIONS[app] ?? '');
  }
  return html.replaceAll('%ORIGIN%', origin);
}

/** Videos stream with byte ranges: Safari will not play without them, and every browser seeks with them. */
function sendVideo(req, res, file, info) {
  const etag = `"${info.size.toString(36)}-${Math.floor(info.mtimeMs).toString(36)}"`;
  const head = { 'Content-Type': TYPES['.mp4'], 'Accept-Ranges': 'bytes', 'Cache-Control': 'public, max-age=86400', ETag: etag, ...SECURITY };
  if (req.headers['if-none-match'] === etag) {
    res.writeHead(304, head);
    return res.end();
  }
  let start = 0;
  let end = info.size - 1;
  const range = /^bytes=(\d*)-(\d*)$/.exec(String(req.headers.range ?? ''));
  if (req.headers.range && (!range || (range[1] === '' && range[2] === ''))) {
    res.writeHead(416, { 'Content-Range': `bytes */${info.size}`, ...SECURITY });
    return res.end();
  }
  if (range) {
    if (range[1] === '') start = Math.max(0, info.size - Number(range[2]));
    else {
      start = Number(range[1]);
      if (range[2] !== '') end = Math.min(end, Number(range[2]));
    }
    if (start > end || start >= info.size) {
      res.writeHead(416, { 'Content-Range': `bytes */${info.size}`, ...SECURITY });
      return res.end();
    }
  }
  res.writeHead(range ? 206 : 200, { ...head, 'Content-Length': end - start + 1, ...(range ? { 'Content-Range': `bytes ${start}-${end}/${info.size}` } : {}) });
  if (req.method === 'HEAD') return res.end();
  const stream = createReadStream(file, { start, end });
  stream.on('error', () => res.destroy());
  res.on('close', () => stream.destroy());
  stream.pipe(res);
}

function originOf(req) {
  const proto = String(req.headers['x-forwarded-proto'] ?? 'http').split(',')[0].trim();
  const host = String(req.headers.host ?? 'localhost').replace(/[^\w.:-]/g, '');
  return `${proto}://${host}`;
}

async function notFound(req, res) {
  const html = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Not found · lsuite</title><link rel="icon" href="/assets/img/lsuite.svg" type="image/svg+xml"><link rel="stylesheet" href="/design/tokens.css"><link rel="stylesheet" href="/assets/styles.css"></head><body>${(await readFile(join(ROOT, 'partials', 'nav.html'), 'utf8')).trim()}<main id="content" class="hero hero--center" style="min-height:60vh"><div class="hero__glow"></div><div class="wrap hero__in"><span class="eyebrow eyebrow--plain">404</span><h1 class="h2">Nothing at this address.</h1><p class="lede">Maybe one of the apps?</p><div class="hero__actions"><a class="btn btn--primary" href="/">lsuite home</a><a class="btn" href="/ryolune">ryolune</a><a class="btn" href="/kimchi">kimchi</a><a class="btn" href="/zenith">zenith</a></div></div></main></body></html>`;
  send(res, 404, html, TYPES['.html'], 'no-store', req);
}

/**
 * lsuite accounts and lsuite AI (AI.md, `ai.js`): accounts in `LSUITE_DATA_DIR/accounts.json`
 * (memory when unset), model requests forwarded with `LSUITE_ANTHROPIC_API_KEY` (a demo answer
 * without it).
 */
export const accounts = createAccounts({
  dataDir: process.env.LSUITE_DATA_DIR || null,
  anthropicKey: process.env.LSUITE_ANTHROPIC_API_KEY || '',
});

export async function handle(req, res) {
  // The API answers on any host name: an app's POST must not be lost to a redirect.
  if (String(req.url ?? '').startsWith('/api/')) {
    if (await accounts.handle(req, res, new URL(req.url, 'http://localhost'))) return;
  }
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    return send(res, 405, 'Method not allowed', 'text/plain; charset=utf-8', 'no-store');
  }
  const url = new URL(req.url ?? '/', 'http://localhost');
  if (url.pathname === '/health') return send(res, 200, 'ok', 'text/plain; charset=utf-8', 'no-store');

  const moved = hostRedirect(req.headers.host, req.url ?? '/');
  if (moved) return redirect(res, 301, moved, 'public, max-age=3600');

  let pathname;
  try {
    pathname = decodeURIComponent(url.pathname);
  } catch {
    return send(res, 400, 'Bad request', 'text/plain; charset=utf-8', 'no-store');
  }

  // One spelling per page: no trailing slash, no index.html.
  const bare = pathname.replace(/\/index\.html$/, '/').replace(/(.)\/+$/, '$1');
  if (bare !== pathname && PAGES[bare]) return redirect(res, 301, bare + url.search, 'public, max-age=3600');

  // ryolune was called Ondera until 0.11.
  if (pathname === '/ondera' || pathname.startsWith('/ondera/')) {
    return redirect(res, 301, '/ryolune' + pathname.slice('/ondera'.length) + url.search, 'public, max-age=86400');
  }

  if (PAGES[pathname]) {
    const html = await renderPage(PAGES[pathname], originOf(req));
    return send(res, 200, html, TYPES['.html'], 'no-cache', req);
  }

  const download = /^\/(ryolune|kimchi|zenith)\/download(?:\/([\w-]+))?$/.exec(pathname);
  if (download) return redirect(res, 302, await downloadTarget(download[1], download[2], req.headers['user-agent']));
  if (/^\/(?:(?:ryolune|kimchi|zenith)\/)?support$/.test(pathname)) {
    return redirect(res, 302, supportTarget());
  }
  if (pathname === '/favicon.ico') return redirect(res, 301, '/assets/img/lsuite.svg', 'public, max-age=86400');

  if (pathname === '/robots.txt') {
    return send(res, 200, `User-agent: *\nAllow: /\n\nSitemap: ${originOf(req)}/sitemap.xml\n`, TYPES['.txt'], 'public, max-age=3600', req);
  }
  if (pathname === '/sitemap.xml') {
    const urls = await Promise.all(
      Object.entries(PAGES).map(async ([path, file]) => {
        const { mtime } = await stat(join(ROOT, file));
        return `  <url><loc>${originOf(req)}${path}</loc><lastmod>${mtime.toISOString().slice(0, 10)}</lastmod></url>`;
      }),
    );
    const xml = `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls.join('\n')}\n</urlset>\n`;
    return send(res, 200, xml, TYPES['.xml'], 'public, max-age=3600', req);
  }

  if (DESIGN_FILES.has(pathname)) {
    const file = join(ROOT, pathname);
    const cache = url.searchParams.has('v') ? 'public, max-age=31536000, immutable' : 'public, max-age=300';
    return send(res, 200, await readFile(file), TYPES[extname(file)], cache, req);
  }

  // Static files: only under /assets/, never dotfiles, never outside ROOT.
  if (!pathname.startsWith('/assets/')) return notFound(req, res);
  const file = normalize(join(ROOT, pathname));
  if (!file.startsWith(join(ROOT, 'assets') + sep) || relative(ROOT, file).split(sep).some((part) => part.startsWith('.'))) return notFound(req, res);
  try {
    const info = await stat(file);
    if (!info.isFile()) return notFound(req, res);
    const ext = extname(file).toLowerCase();
    if (ext === '.mp4') return sendVideo(req, res, file, info);
    const cache = url.searchParams.has('v') ? 'public, max-age=31536000, immutable' : 'public, max-age=3600';
    send(res, 200, await readFile(file), TYPES[ext] ?? 'application/octet-stream', cache, req);
  } catch {
    notFound(req, res);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  createServer((req, res) =>
    handle(req, res).catch((err) => {
      console.error(err);
      if (!res.headersSent) send(res, 500, 'Server error', 'text/plain; charset=utf-8', 'no-store');
      else res.destroy();
    }),
  ).listen(PORT, HOST, () => console.log(`lsuite site listening on http://${HOST}:${PORT}`));
}
