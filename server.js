// Dependency-free server for lsuite.xyz. Railway runs `npm start`.
//
// Pages are plain HTML (`index.html`, `<app>/index.html`) with two includes, `<!-- include:nav -->`
// and `<!-- include:foot -->`, filled from `partials/` when served (and `<!-- include:plans -->`,
// `<!-- include:marketplace -->` from the API's data). Everything static lives in `assets/`. Old
// app domains (ryolune.com) answer with a 301 to the app's page here.
import { createServer } from 'node:http';
import { createReadStream } from 'node:fs';
import { readFile, stat } from 'node:fs/promises';
import { dirname, extname, join, normalize, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { liveConfig } from './live.js';
import { createAccounts, plansDocument } from './ai.js';
import { objectStoreConfig } from './cloud.js';
import { MARKET_APPS, PLATFORMS } from './marketplace.js';
import { createBuilds } from './builds.js';

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

/**
 * The pages, by path. Each app page is `<app>/index.html`; the launcher's is `pages/launcher.html`,
 * as `launcher/` holds its Rust workspace (never served: only `/assets/` is static).
 */
export const PAGES = {
  '/': 'index.html',
  '/ryolune': 'ryolune/index.html',
  '/kimchi': 'kimchi/index.html',
  '/zenith': 'zenith/index.html',
  '/nori': 'nori/index.html',
  '/folio': 'folio/index.html',
  '/launcher': 'pages/launcher.html',
  '/pass': 'pass/index.html',
  '/marketplace': 'marketplace/index.html',
  '/account': 'account/index.html',
  '/account/connect': 'account/connect.html',
  '/account/checkout': 'account/checkout.html',
  '/design': 'design/index.html',
};
/** Pages that stay out of the sitemap (steps of a flow, not places to land). */
const UNLISTED = new Set(['/account/connect', '/account/checkout']);

const escapeHtml = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

/**
 * The plans of lsuite Pass (`/api/ai/plans`) as cards (`<!-- include:plans -->`): per paid plan,
 * lsuite AI's credits and models, lsuite Cloud's storage and the lsuite Marketplace.
 */
export function plansHtml() {
  const { plans } = plansDocument();
  const cards = plans.map((p) => {
    const free = p.id === 'free';
    const items = free
      ? ['Every feature of every app', 'Your subscriptions and keys, as they are', 'Browse the marketplace', 'Nothing to pay, ever']
      : [
          `<b>${p.credits.toLocaleString('en-US')} credits</b> a month of lsuite AI`,
          escapeHtml(p.families.join(', ')),
          ...(p.storage ? [`<b>${escapeHtml(p.storageLabel)}</b> lsuite Cloud`] : []),
          ...(p.marketplace ? ['The lsuite Marketplace'] : []),
          ...(p.priority ? ['Priority when it’s busy'] : []),
        ];
    const action = free
      ? `<a class="btn" href="/account">Create a free account</a>`
      : `<a class="btn${p.id === 'pro' ? ' btn--primary' : ''}" href="/account/checkout?plan=${p.id}">Choose ${escapeHtml(p.name)}</a>`;
    return `<div class="plan${p.id === 'pro' ? ' plan--lit' : ''}" data-plan="${p.id}"><div class="plan__head"><h3>${escapeHtml(p.name)}</h3><span class="plan__price">${free ? '<b>$0</b>' : `<b>$${p.price}</b> / month`}</span></div><p class="plan__say">${escapeHtml(p.summary)}</p><ul class="plan__list">${items.map((i) => `<li><span>${i}</span></li>`).join('')}</ul>${action}</div>`;
  });
  return `<div class="plans">${cards.join('')}</div>`;
}

const PLATFORM_NAMES = { 'macos-arm64': 'macOS Apple silicon', 'macos-x86_64': 'macOS Intel', 'linux-x86_64': 'Linux', 'windows-x86_64': 'Windows' };

/**
 * The marketplace's approved plugins (`GET /api/marketplace`'s `plugins`) as cards, with links
 * filtering by app (`/marketplace?app=<app>`, no script) and an empty state
 * (`<!-- include:marketplace -->`). `app`: the filter, or null for every app.
 */
export function marketplaceHtml(plugins, app = null) {
  const link = (id, label, icon) =>
    `<a href="/marketplace${id ? `?app=${id}` : ''}#plugins"${(id ?? null) === app ? ' aria-current="page"' : ''}>${icon ? `<img src="/assets/img/icons/${id}.webp" width="18" height="18" alt="">` : ''}${label}</a>`;
  const filters = `<nav class="filters" aria-label="Plugins by app">${link(null, 'Every app')}${MARKET_APPS.map((id) => link(id, id, true)).join('')}</nav>`;
  if (!plugins.length) {
    const what = app ? `No ${escapeHtml(app)} plugins yet.` : 'No plugins yet.';
    return `${filters}<div class="empty"><p class="empty__title">The first plugins are on their way — publish yours</p><p class="muted">${what} Build one with your app's agent and publish it: every version is reviewed before anyone can install it.</p><a class="btn" href="#publish">Publish a plugin</a></div>`;
  }
  const cards = plugins.map((p) => {
    const platforms = Object.keys(PLATFORMS).filter((k) => p.platforms[k]).map((k) => PLATFORM_NAMES[k]);
    const author = p.author.verified ? `lsuite <span class="badge badge--sm">by lsuite</span>` : escapeHtml(p.author.name);
    return `<article class="plugin" data-plugin="${escapeHtml(p.id)}"><div class="plugin__top"><img src="/assets/img/icons/${escapeHtml(p.app)}.webp" width="28" height="28" alt=""><span class="plugin__app">${escapeHtml(p.app)}</span><span class="plugin__kind">${escapeHtml(p.kind)}</span></div><h3 class="plugin__name">${escapeHtml(p.name)}</h3><p class="plugin__by">by ${author}</p><p class="plugin__say">${escapeHtml(p.description)}</p><dl class="plugin__meta"><dt>Version</dt><dd>${escapeHtml(p.version)}</dd><dt>Platforms</dt><dd>${platforms.map(escapeHtml).join(', ')}</dd><dt>Downloads</dt><dd>${Number(p.downloads).toLocaleString('en-US')}</dd></dl><p class="plugin__id"><code>${escapeHtml(p.id)}</code></p></article>`;
  });
  return `${filters}<div class="plugins">${cards.join('')}</div>`;
}

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

/**
 * Each app's downloads (and the launcher's): its repository, an asset pattern per platform, and the
 * platform each OS gets by default. `/launcher/download/<platform>` looks the asset up in the latest
 * published release; a platform with no matching asset goes to the release page. The five apps
 * come only through the lsuite app (DISTRIBUTION.md): their `/<app>/download[/…]` lands on
 * `/launcher`, and their tables only say which platforms `GET /api/apps` lists. `published:
 * false` keeps an app's route table ready before its first release (the route lands on the app's
 * page). `tagPrefix`: the release tags (`v` by default), for a repository that releases more than one thing.
 */
export const DOWNLOADS = {
  // Release filenames are stable; the GitHub release tag carries the version.
  ryolune: {
    repo: 'ludovic111/ryolune',
    patterns: {
      'macos-arm64': /\/ryolune-macos-arm64\.zip$/,
      'macos-x86_64': /\/ryolune-macos-x86_64\.zip$/,
      'windows-x86_64': /\/ryolune-windows-x86_64\.exe$/,
      'windows-zip': /\/ryolune-windows-x86_64\.zip$/,
      'linux-x86_64': /\/ryolune-linux-x86_64\.tar\.gz$/,
    },
    byOs: { macos: 'macos-arm64', windows: 'windows-x86_64', linux: 'linux-x86_64' },
  },
  kimchi: {
    repo: 'ludovic111/kimchi',
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
  },
  zenith: {
    repo: 'ludovic111/zenith',
    patterns: {
      'macos-arm64': /\/zenith-macos-arm64\.zip$/,
      'macos-x86_64': /\/zenith-macos-x86_64\.zip$/,
      'linux-x86_64': /\/zenith-linux-x86_64\.tar\.gz$/,
    },
    byOs: { macos: 'macos-arm64', linux: 'linux-x86_64' },
  },
  // Published image and design app builds.
  nori: {
    repo: 'ludovic111/nori',
    published: true,
    patterns: {
      'macos-arm64': /\/nori_aarch64\.dmg$/,
      'macos-x86_64': /\/nori_x64\.dmg$/,
      'windows-x86_64': /\/nori_x64-setup\.exe$/,
      'linux-x86_64': /\/nori_amd64\.AppImage$/,
    },
    byOs: { macos: 'macos-arm64', windows: 'windows-x86_64', linux: 'linux-x86_64' },
  },
  // Published office app builds.
  folio: {
    repo: 'ludovic111/folio',
    published: true,
    patterns: {
      'macos-arm64': /\/folio-macos-arm64\.(zip|dmg)$/,
      'macos-x86_64': /\/folio-macos-x86_64\.(zip|dmg)$/,
      'windows-x86_64': /\/folio-windows-x86_64\.(exe|zip)$/,
      'linux-x86_64': /\/folio-linux-x86_64\.(tar\.gz|AppImage)$/,
    },
    byOs: { macos: 'macos-arm64', windows: 'windows-x86_64', linux: 'linux-x86_64' },
  },
  // The lsuite launcher (`launcher/`), released from this site's repository as `launcher-vX.Y.Z`.
  launcher: {
    repo: 'ludovic111/lsuite',
    tagPrefix: 'launcher-v',
    patterns: {
      'macos-arm64': /\/lsuite-macos-arm64\.dmg$/,
      'macos-x86_64': /\/lsuite-macos-x86_64\.dmg$/,
      'windows-x86_64': /\/lsuite-windows-x86_64-setup\.exe$/,
      'windows-zip': /\/lsuite-windows-x86_64\.zip$/,
      'linux-x86_64': /\/lsuite-linux-x86_64\.AppImage$/,
      'linux-tar': /\/lsuite-linux-x86_64\.tar\.gz$/,
    },
    byOs: { macos: 'macos-arm64', windows: 'windows-x86_64', linux: 'linux-x86_64' },
  },
};
/** The five apps (`/api/apps`, `/<app>/support`). The launcher has downloads but isn't one of them. */
export const APP_NAMES = ['ryolune', 'kimchi', 'zenith', 'nori', 'folio'];

const releaseCache = new Map();
/**
 * A repository's latest published release (`tag`, asset `urls`), cached for ten minutes. With a
 * `prefix`, the newest published release whose tag starts with it (the repository's "latest" may
 * be something else).
 */
async function latestRelease(repo, prefix = null) {
  const key = prefix ? `${repo}#${prefix}` : repo;
  const hit = releaseCache.get(key);
  if (hit && Date.now() - hit.at < 10 * 60 * 1000) return hit;
  try {
    const res = await fetch(`https://api.github.com/repos/${repo}/releases${prefix ? '?per_page=30' : '/latest'}`, {
      headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'lsuite-site' },
      signal: AbortSignal.timeout(4000),
    });
    if (!res.ok) throw new Error(String(res.status));
    const json = await res.json();
    const found = prefix ? json.find((r) => !r.draft && !r.prerelease && String(r.tag_name).startsWith(prefix)) : json;
    if (!found) throw new Error('no release');
    const release = { at: Date.now(), tag: found.tag_name, urls: found.assets.map((a) => a.browser_download_url) };
    releaseCache.set(key, release);
    return release;
  } catch {
    return hit ?? { tag: null, urls: [] };
  }
}

// Shown when GitHub cannot be reached. Pages say `%VERSION:<app>%` and get the version of the
// latest published release, so the page never announces a version you cannot get yet. The five
// apps' versions come from the private builds (`builds.js`, DISTRIBUTION.md) once
// `LSUITE_BUILDS_TOKEN` is set; the launcher's from its public `launcher-vX.Y.Z` releases in ludovic111/lsuite.
const FALLBACK_VERSIONS = { ryolune: '0.15.3', kimchi: '0.10.0', zenith: '0.4.0', nori: '0.1.0', folio: '0.1.0', launcher: '0.1.1' };
const REPOS = { ryolune: 'ludovic111/ryolune', kimchi: 'ludovic111/kimchi', zenith: 'ludovic111/zenith', nori: 'ludovic111/nori', folio: 'ludovic111/folio', launcher: 'ludovic111/lsuite' };

/** What `GET /api/apps` says of each app besides its downloads (the home page's cards). */
const APP_INFO = {
  ryolune: { kind: 'music', summary: 'The DAW your AI can drive.' },
  kimchi: { kind: 'video', summary: 'A video editor where generation is part of the cut.' },
  zenith: { kind: 'code', summary: 'An app for coding with agents.' },
  nori: { kind: 'image', summary: 'Pixels, vectors and pages in one document.' },
  folio: { kind: 'office', summary: 'Documents, spreadsheets and slides in one app.' },
};

/** The version a release tag names: `v0.15.3` → `0.15.3`, `launcher-v0.1.0` → `0.1.0` (prefix `launcher-v`). */
export function versionOf(tag, prefix = 'v') {
  return tag.startsWith(prefix) ? tag.slice(prefix.length) : tag.replace(/^v/, '');
}

/**
 * `{ app: version }` for every app (or the launcher) whose version a page asks for. The five apps:
 * from `source` (the private builds, `builds.js`) when it is configured, else their public releases
 * while those last; the fallback when neither answers.
 */
export async function appVersions(apps, source = builds) {
  const out = {};
  await Promise.all(
    apps.map(async (app) => {
      if (APP_NAMES.includes(app) && source?.configured) {
        out[app] = (await source.version(app)) ?? FALLBACK_VERSIONS[app] ?? '';
        return;
      }
      const prefix = DOWNLOADS[app]?.tagPrefix;
      const { tag } = REPOS[app] ? await latestRelease(REPOS[app], prefix) : {};
      out[app] = tag ? versionOf(tag, prefix) : FALLBACK_VERSIONS[app] ?? '';
    }),
  );
  return out;
}

/**
 * `GET /api/apps`: the suite's apps for the lsuite launcher (CLOUD.md), with the version of each
 * one's latest published release and the platforms `/<app>/download/<platform>` knows.
 */
export async function appsDocument(origin, versions) {
  const known = versions ?? (await appVersions(APP_NAMES));
  return {
    apps: APP_NAMES.map((id) => ({
      id,
      name: id,
      kind: APP_INFO[id].kind,
      summary: APP_INFO[id].summary,
      page: `${origin}/${id}`,
      repo: REPOS[id] ?? DOWNLOADS[id].repo,
      version: known[id] || null,
      published: DOWNLOADS[id].published !== false,
      platforms: Object.keys(DOWNLOADS[id].patterns),
    })),
  };
}

/**
 * Where `/<app>/download[/<platform>]` sends the visitor. The five apps come only through the
 * lsuite app (DISTRIBUTION.md): their downloads land on its page; the launcher's are its releases.
 */
export async function downloadTarget(app, wanted, userAgent) {
  const d = DOWNLOADS[app];
  if (!d) return null;
  if (APP_NAMES.includes(app)) return '/launcher';
  // Not released yet: the download link lands on the app's page ("First build coming").
  if (d.published === false) return `/${app}`;
  // GitHub's "latest" is the repository's; a tag prefix needs that release's own page.
  const releases = (tag) => (d.tagPrefix ? `https://github.com/${d.repo}/releases${tag ? `/tag/${tag}` : ''}` : `https://github.com/${d.repo}/releases/latest`);
  const pattern = d.patterns[wanted || d.byOs[osFor(userAgent)]];
  if (!pattern) return releases();
  const release = await latestRelease(d.repo, d.tagPrefix);
  return release.urls.find((u) => pattern.test(u)) ?? releases(release.tag);
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

/**
 * A page as served: includes filled, its app marked current in the nav, versions stamped. `query`:
 * the request's search parameters (the marketplace's `?app=` filter).
 */
export async function renderPage(file, origin, versions, production = process.env.LSUITE_MODE === 'production', query = new URLSearchParams()) {
  let html = await readFile(join(ROOT, file), 'utf8');
  html = html.replace(/<!-- demo:start -->([\s\S]*?)<!-- demo:end -->/g, (_, content) => production ? '' : content)
    .replace(/<!-- live:start -->([\s\S]*?)<!-- live:end -->/g, (_, content) => production ? content : '');
  for (const name of ['nav', 'foot']) {
    if (html.includes(`<!-- include:${name} -->`)) {
      html = html.replace(`<!-- include:${name} -->`, (await readFile(join(ROOT, 'partials', `${name}.html`), 'utf8')).trim());
    }
  }
  if (html.includes('<!-- include:plans -->')) html = html.replace('<!-- include:plans -->', plansHtml());
  if (html.includes('<!-- include:marketplace -->')) {
    const app = MARKET_APPS.includes(query.get('app')) ? query.get('app') : null;
    const { plugins } = await accounts.marketplace.catalogue(app);
    html = html.replace('<!-- include:marketplace -->', () => marketplaceHtml(plugins, app));
  }
  // The page's app is its path's first segment (`/kimchi`, `/launcher`, `/account/connect`).
  const route = Object.keys(PAGES).find((path) => PAGES[path] === file);
  const app = route ? route.split('/')[1] || null : file.includes('/') ? file.split('/')[0] : null;
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
  const html = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Not found · lsuite</title><link rel="icon" href="/assets/img/lsuite.svg" type="image/svg+xml"><link rel="stylesheet" href="/design/tokens.css"><link rel="stylesheet" href="/assets/styles.css"></head><body>${(await readFile(join(ROOT, 'partials', 'nav.html'), 'utf8')).trim()}<main id="content" class="hero hero--center" style="min-height:60vh"><div class="hero__glow"></div><div class="wrap hero__in"><span class="eyebrow eyebrow--plain">404</span><h1 class="h2">Nothing at this address.</h1><p class="lede">Maybe one of the apps?</p><div class="hero__actions"><a class="btn btn--primary" href="/">lsuite home</a><a class="btn" href="/ryolune">ryolune</a><a class="btn" href="/kimchi">kimchi</a><a class="btn" href="/zenith">zenith</a><a class="btn" href="/nori">nori</a><a class="btn" href="/folio">folio</a></div></div></main></body></html>`;
  send(res, 404, html, TYPES['.html'], 'no-store', req);
}

/**
 * lsuite accounts and lsuite Pass (PASS.md, `ai.js`): accounts in `LSUITE_DATA_DIR/accounts.json`
 * (memory when unset), lsuite AI's model requests forwarded in production, lsuite Cloud and the
 * lsuite Marketplace's files in the data dir (or an object store), admins from `LSUITE_ADMIN_EMAILS`.
 */
export const accounts = createAccounts({
  mode: process.env.LSUITE_MODE || 'demo',
  live: liveConfig(),
  dataDir: process.env.LSUITE_DATA_DIR || null,
  anthropicKey: process.env.LSUITE_ANTHROPIC_API_KEY || '',
  // lsuite Cloud's blobs in an S3-compatible store (`LSUITE_CLOUD_S3_*`, CLOUD.md), else on the data dir.
  cloudStore: objectStoreConfig(),
});
/**
 * The apps' builds (DISTRIBUTION.md, `builds.js`): `/api/apps/<app>/…` for signed-in apps, read
 * from the private `ludovic111/lsuite-builds` with `LSUITE_BUILDS_TOKEN` (unset: 503).
 */
export const builds = createBuilds({
  token: process.env.LSUITE_BUILDS_TOKEN || '',
  auth: accounts.appAccount,
  origin: accounts.publicOrigin,
});
const DOWNLOAD_ROUTE = new RegExp(`^/(${Object.keys(DOWNLOADS).join('|')})/download(?:/([\\w-]+))?$`);
const SUPPORT_ROUTE = new RegExp(`^/(?:(?:${APP_NAMES.join('|')})/)?support$`);

export async function handle(req, res) {
  // The API answers on any host name: an app's POST must not be lost to a redirect.
  if (/^\/api\/apps(?:\?|$)/.test(String(req.url ?? ''))) {
    if (req.method !== 'GET' && req.method !== 'HEAD') {
      return send(res, 405, JSON.stringify({ type: 'error', error: { type: 'invalid_request_error', message: 'Use GET.' } }), TYPES['.json'], 'no-store');
    }
    return send(res, 200, JSON.stringify(await appsDocument(originOf(req))), TYPES['.json'], 'public, max-age=300', req);
  }
  if (String(req.url ?? '').startsWith('/api/apps/')) {
    if (await builds.handle(req, res, new URL(req.url, 'http://localhost'))) return;
  }
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

  // The subscription's page was /ai until it became lsuite Pass (2026-10-07).
  if (/^\/ai(\/|\/index\.html)?$/.test(pathname)) return redirect(res, 301, '/pass' + url.search, 'public, max-age=86400');

  // ryolune was called Ondera until 0.11.
  if (pathname === '/ondera' || pathname.startsWith('/ondera/')) {
    return redirect(res, 301, '/ryolune' + pathname.slice('/ondera'.length) + url.search, 'public, max-age=86400');
  }

  if (PAGES[pathname]) {
    const html = await renderPage(PAGES[pathname], originOf(req), undefined, undefined, url.searchParams);
    return send(res, 200, html, TYPES['.html'], 'no-cache', req);
  }

  const download = DOWNLOAD_ROUTE.exec(pathname);
  if (download) return redirect(res, 302, await downloadTarget(download[1], download[2], req.headers['user-agent']));
  if (SUPPORT_ROUTE.test(pathname)) {
    return redirect(res, 302, supportTarget());
  }
  if (pathname === '/favicon.ico') return redirect(res, 301, '/assets/img/lsuite.svg', 'public, max-age=86400');

  if (pathname === '/robots.txt') {
    return send(res, 200, `User-agent: *\nAllow: /\nDisallow: /api/\nDisallow: /account/\n\nSitemap: ${originOf(req)}/sitemap.xml\n`, TYPES['.txt'], 'public, max-age=3600', req);
  }
  if (pathname === '/sitemap.xml') {
    const urls = await Promise.all(
      Object.entries(PAGES).filter(([path]) => !UNLISTED.has(path)).map(async ([path, file]) => {
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
