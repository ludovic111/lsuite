// The apps' builds (DISTRIBUTION.md, builds.js) against a fake GitHub API: every route, public,
// rewriting, caching, the token kept on the server, 503 without it, 404s and 502s.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { createBuilds, compareSemver, buildVersion, NOT_SET_UP, BUILDS_REPO } from '../builds.js';
import { appVersions, appsDocument, APP_NAMES } from '../server.js';

const GH_TOKEN = 'github_pat_secret';
const DL = (tag, name) => `https://github.com/${BUILDS_REPO}/releases/download/${tag}/${name}`;

const KIMCHI_MANIFEST = {
  version: '0.10.0',
  notes: 'Faster export.',
  pub_date: '2026-10-07T10:00:00Z',
  platforms: {
    'darwin-aarch64': { signature: 'sig-mac', url: DL('kimchi-v0.10.0', 'kimchi_0.10.0_aarch64.app.tar.gz') },
    'windows-x86_64': { signature: 'sig-win', url: DL('kimchi-v0.10.0', 'kimchi_0.10.0_x64-setup.exe') },
  },
};

/** A fake GitHub: the builds repository's releases, asset downloads (302 to a signed address), and counters. */
async function fakeGitHub() {
  const seen = { listing: 0, assets: {}, signed: {}, authOnSigned: [], authOnApi: [] };
  const state = { fail: false };
  let id = 0;
  const files = {};
  const asset = (name, content, size) => {
    const a = { id: ++id, name, size: size ?? Buffer.byteLength(content ?? 'x'), browser_download_url: `https://github.com/${BUILDS_REPO}/releases/download/x/${name}` };
    files[a.id] = content ?? 'binary';
    return a;
  };
  const releases = [
    // Listed before the newest on purpose: the newest is by semver, not by position.
    { tag_name: 'kimchi-v0.9.12', name: 'kimchi 0.9.12', body: 'Old.', draft: false, prerelease: false, assets: [asset('latest.json', JSON.stringify({ version: '0.9.12', platforms: {} }))] },
    { tag_name: 'kimchi-v0.11.0', name: 'kimchi 0.11.0', body: 'Draft.', draft: true, prerelease: false, assets: [asset('kimchi_0.11.0_aarch64.dmg', null, 9)] },
    { tag_name: 'kimchi-v0.12.0-rc.1', name: 'kimchi 0.12.0-rc.1', body: 'Pre.', draft: false, prerelease: true, assets: [] },
    { tag_name: 'kimchify-v9.0.0', name: 'not kimchi', body: '', draft: false, prerelease: false, assets: [] },
    {
      tag_name: 'kimchi-v0.10.0',
      name: 'kimchi 0.10.0',
      body: '## New\n- Faster export.',
      draft: false,
      prerelease: false,
      assets: [
        asset('kimchi_0.10.0_aarch64.dmg', null, 52_000_000),
        asset('kimchi_0.10.0_aarch64.app.tar.gz', null, 48_000_000),
        asset('kimchi_0.10.0_x64-setup.exe', null, 40_000_000),
        asset('latest.json', JSON.stringify(KIMCHI_MANIFEST)),
        asset('SHA256SUMS', 'abc  kimchi_0.10.0_aarch64.dmg\n'),
        asset('SHA256SUMS.sig', 'untrusted comment: sig\nRWQ...\n'),
      ],
    },
    { tag_name: 'nori-v0.1.0', name: 'nori 0.1.0', body: '', draft: false, prerelease: false, assets: [asset('nori_aarch64.dmg', null, 30_000_000)] },
    {
      tag_name: 'ryolune-v0.16.0',
      name: 'ryolune 0.16.0',
      body: 'Notes.',
      draft: false,
      prerelease: false,
      assets: [asset('ryolune-macos-arm64.zip', null, 60_000_000), asset('SHA256SUMS', 'def  ryolune-macos-arm64.zip\n'), asset('SHA256SUMS.sig', 'sig')],
    },
  ];
  const server = createServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    if (url.pathname.startsWith('/signed/')) {
      seen.authOnSigned.push(req.headers.authorization ?? null);
      const n = Number(url.pathname.split('/')[2]);
      seen.signed[n] = (seen.signed[n] ?? 0) + 1;
      res.writeHead(200, { 'Content-Type': 'application/octet-stream' });
      return res.end(files[n]);
    }
    if (url.pathname.startsWith('/hop/')) {
      seen.authOnSigned.push(req.headers.authorization ?? null);
      res.writeHead(302, { Location: `${base}/signed/${url.pathname.split('/')[2]}?X-Amz-Signature=short-lived` });
      return res.end();
    }
    seen.authOnApi.push(req.headers.authorization ?? null);
    if (req.headers.authorization !== `Bearer ${GH_TOKEN}`) return res.writeHead(401).end('{"message":"Bad credentials"}');
    if (state.fail) return res.writeHead(500).end('{"message":"boom"}');
    if (url.pathname === `/repos/${BUILDS_REPO}/releases` && url.searchParams.get('per_page') === '100') {
      seen.listing++;
      res.writeHead(200, { 'Content-Type': 'application/json' });
      return res.end(JSON.stringify(releases));
    }
    const m = new RegExp(`^/repos/${BUILDS_REPO}/releases/assets/(\\d+)$`).exec(url.pathname);
    if (m && req.headers.accept === 'application/octet-stream') {
      const n = Number(m[1]);
      seen.assets[n] = (seen.assets[n] ?? 0) + 1;
      // Like GitHub: a redirect to a short-lived address, which a second hop may redirect again.
      res.writeHead(302, { Location: `/hop/${n}` });
      return res.end();
    }
    res.writeHead(404).end('{"message":"Not Found"}');
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  const idOf = (tag, name) => releases.find((r) => r.tag_name === tag).assets.find((a) => a.name === name).id;
  return { base, seen, state, idOf, close: () => new Promise((r) => server.close(r)) };
}

/** The builds on a server, as server.js wires them, with a clock for the cache. */
async function start(gh, options = {}) {
  const clock = { t: 1_000_000 };
  const builds = createBuilds({ token: GH_TOKEN, api: gh?.base, now: () => clock.t, ...options });
  const server = createServer(async (req, res) => {
    if (await builds.handle(req, res, new URL(req.url, 'http://localhost'))) return;
    res.writeHead(404).end();
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  const get = (path, { headers = {}, method = 'GET' } = {}) => fetch(base + path, { method, redirect: 'manual', headers });
  return { base, builds, clock, get, close: () => new Promise((r) => server.close(r)) };
}

async function error(res, status, type, message) {
  assert.equal(res.status, status);
  assert.match(res.headers.get('content-type'), /application\/json/);
  const body = await res.json();
  assert.equal(body.type, 'error');
  assert.equal(body.error.type, type);
  if (message) assert.equal(body.error.message, message);
  return body.error.message;
}

const ROUTES = ['/api/apps/kimchi/latest', '/api/apps/kimchi/latest.json', '/api/apps/kimchi/releases/latest', '/api/apps/kimchi/files/kimchi-v0.10.0/latest.json'];

test('semver of the tags', () => {
  assert.equal(buildVersion('kimchi', 'kimchi-v0.10.0'), '0.10.0');
  assert.equal(buildVersion('kimchi', 'kimchify-v1.0.0'), null);
  assert.equal(buildVersion('kimchi', 'nori-v0.1.0'), null);
  assert.equal(buildVersion('kimchi', 'kimchi-vlatest'), null);
  assert.ok(compareSemver('0.10.0', '0.9.12') > 0);
  assert.ok(compareSemver('1.0.0-rc.1', '1.0.0') < 0);
  assert.ok(compareSemver('1.0.0-rc.2', '1.0.0-rc.10') < 0);
  assert.equal(compareSemver('2.1.3', '2.1.3'), 0);
});

test('every route is public: no token, and an old app still sending one, both get the build', async () => {
  const gh = await fakeGitHub();
  const t = await start(gh);
  try {
    for (const path of ROUTES) {
      assert.ok([200, 302].includes((await t.get(path)).status), path);
      assert.ok([200, 302].includes((await t.get(path, { headers: { authorization: 'Bearer lsk_old-account-token' } })).status), path);
    }
    await error(await t.get('/api/apps/kimchi/latest', { method: 'POST' }), 405, 'invalid_request_error');
    assert.equal(gh.seen.authOnApi.filter(Boolean).length, gh.seen.authOnApi.length, 'GitHub always got the server token');
  } finally {
    await t.close();
    await gh.close();
  }
});

test('without LSUITE_BUILDS_TOKEN: 503, and no versions', async () => {
  const t = await start(null, { token: '', fetch: () => { throw new Error('no GitHub without a token'); } });
  try {
    for (const path of ROUTES) await error(await t.get(path), 503, 'api_error', NOT_SET_UP);
    assert.equal(t.builds.configured, false);
    assert.equal(await t.builds.version('kimchi'), null);
  } finally {
    await t.close();
  }
});

test('latest: the newest published release by semver, its manifest on the file route, the checksums as text', async () => {
  const gh = await fakeGitHub();
  const t = await start(gh);
  try {
    const res = await t.get('/api/apps/kimchi/latest');
    assert.equal(res.status, 200);
    assert.equal(res.headers.get('cache-control'), 'no-store');
    const body = await res.json();
    assert.deepEqual(Object.keys(body), ['app', 'version', 'tag', 'notes', 'files', 'manifest', 'sha256sums', 'sha256sumsSig']);
    assert.equal(body.app, 'kimchi');
    assert.equal(body.version, '0.10.0', 'not the draft 0.11.0, the pre-release 0.12.0-rc.1 or kimchify');
    assert.equal(body.tag, 'kimchi-v0.10.0');
    assert.equal(body.notes, '## New\n- Faster export.');
    assert.deepEqual(body.files[0], { name: 'kimchi_0.10.0_aarch64.dmg', size: 52_000_000 });
    assert.equal(body.files.length, 6);
    assert.equal(body.sha256sums, 'abc  kimchi_0.10.0_aarch64.dmg\n');
    assert.equal(body.sha256sumsSig, 'untrusted comment: sig\nRWQ...\n');
    // Every url rewritten onto this server; everything else (signatures) untouched.
    assert.deepEqual(body.manifest, {
      ...KIMCHI_MANIFEST,
      platforms: {
        'darwin-aarch64': { signature: 'sig-mac', url: `${t.base}/api/apps/kimchi/files/kimchi-v0.10.0/kimchi_0.10.0_aarch64.app.tar.gz` },
        'windows-x86_64': { signature: 'sig-win', url: `${t.base}/api/apps/kimchi/files/kimchi-v0.10.0/kimchi_0.10.0_x64-setup.exe` },
      },
    });
    assert.ok(!JSON.stringify(body).includes(GH_TOKEN));
    // Small files are fetched through GitHub's redirects, and the signed address never gets the token.
    assert.ok(gh.seen.authOnSigned.length >= 3);
    assert.ok(gh.seen.authOnSigned.every((h) => h === null), 'no token past GitHub');

    // An app with a release but none of the small files.
    const nori = await (await t.get('/api/apps/nori/latest')).json();
    assert.equal(nori.version, '0.1.0');
    assert.equal(nori.manifest, null);
    assert.equal(nori.sha256sums, null);
    assert.equal(nori.sha256sumsSig, null);
    // An app with no release at all.
    await error(await t.get('/api/apps/folio/latest'), 404, 'not_found_error');
    await error(await t.get('/api/apps/photoshop/latest'), 404, 'not_found_error');
    await error(await t.get('/api/apps/kimchi/oldest'), 404, 'not_found_error');
  } finally {
    await t.close();
    await gh.close();
  }
});

test('latest.json: the rewritten manifest alone, 404 without one; addresses on the public origin', async () => {
  const gh = await fakeGitHub();
  const t = await start(gh, { origin: () => 'https://lsuite.xyz' });
  try {
    const manifest = await (await t.get('/api/apps/kimchi/latest.json')).json();
    assert.equal(manifest.version, '0.10.0');
    assert.equal(manifest.platforms['darwin-aarch64'].url, 'https://lsuite.xyz/api/apps/kimchi/files/kimchi-v0.10.0/kimchi_0.10.0_aarch64.app.tar.gz');
    assert.equal(manifest.platforms['darwin-aarch64'].signature, 'sig-mac');
    await error(await t.get('/api/apps/nori/latest.json'), 404, 'not_found_error');
    const head = await t.get('/api/apps/kimchi/latest.json', { method: 'HEAD' });
    assert.equal(head.status, 200);
  } finally {
    await t.close();
    await gh.close();
  }
});

test("releases/latest: GitHub's shape, every asset on the file route", async () => {
  const gh = await fakeGitHub();
  const t = await start(gh);
  try {
    const release = await (await t.get('/api/apps/ryolune/releases/latest')).json();
    assert.deepEqual(release, {
      tag_name: 'v0.16.0',
      name: 'ryolune 0.16.0',
      body: 'Notes.',
      assets: ['ryolune-macos-arm64.zip', 'SHA256SUMS', 'SHA256SUMS.sig'].map((name, i) => ({
        name,
        size: [60_000_000, 29, 3][i],
        browser_download_url: `${t.base}/api/apps/ryolune/files/ryolune-v0.16.0/${name}`,
      })),
    });
    // tag_name as the apps' own releases had it (`v…`); the builds tag in the addresses and in /latest.
    assert.equal((await (await t.get('/api/apps/ryolune/latest')).json()).tag, 'ryolune-v0.16.0');
  } finally {
    await t.close();
    await gh.close();
  }
});

test("files: 302 to GitHub's short-lived address, never the file or the token; only that app's files", async () => {
  const gh = await fakeGitHub();
  const t = await start(gh);
  try {
    const res = await t.get('/api/apps/kimchi/files/kimchi-v0.10.0/kimchi_0.10.0_aarch64.dmg');
    assert.equal(res.status, 302);
    const id = gh.idOf('kimchi-v0.10.0', 'kimchi_0.10.0_aarch64.dmg');
    // The API's own Location, as GitHub gave it (followed by the app, not by the server).
    assert.equal(res.headers.get('location'), `/hop/${id}`);
    assert.ok(!res.headers.get('location').includes(GH_TOKEN));
    assert.equal(res.headers.get('cache-control'), 'no-store');
    assert.equal(await res.text(), '');
    assert.equal(gh.seen.assets[id], 1);
    assert.equal(gh.seen.signed[id], undefined, 'big files are never fetched by the server');
    // An older release's files, and names that need encoding, work too.
    assert.equal((await t.get('/api/apps/kimchi/files/kimchi-v0.9.12/latest.json')).status, 302);
    assert.equal((await t.get(`/api/apps/kimchi/files/${encodeURIComponent('kimchi-v0.10.0')}/${encodeURIComponent('SHA256SUMS.sig')}`)).status, 302);

    await error(await t.get('/api/apps/kimchi/files/kimchi-v0.10.0/nope.dmg'), 404, 'not_found_error');
    await error(await t.get('/api/apps/kimchi/files/kimchi-v9.9.9/latest.json'), 404, 'not_found_error');
    await error(await t.get('/api/apps/kimchi/files/kimchi-v0.11.0/kimchi_0.11.0_aarch64.dmg'), 404, 'not_found_error', 'kimchi has no release kimchi-v0.11.0.');
    await error(await t.get('/api/apps/kimchi/files/ryolune-v0.16.0/ryolune-macos-arm64.zip'), 404, 'not_found_error', "ryolune-v0.16.0 isn't a release of kimchi.");
    await error(await t.get('/api/apps/ryolune/files/kimchi-v0.10.0/latest.json'), 404, 'not_found_error');
    await error(await t.get('/api/apps/kimchi/files/kimchi-v0.10.0/%E0%A4%A'), 404, 'not_found_error');
    await error(await t.get('/api/apps/kimchi/files/kimchi-v0.10.0/a/b'), 404, 'not_found_error');
  } finally {
    await t.close();
    await gh.close();
  }
});

test('cached 5 minutes: one listing and one download per small file, then fresh again', async () => {
  const gh = await fakeGitHub();
  const t = await start(gh);
  try {
    await Promise.all([t.get('/api/apps/kimchi/latest'), t.get('/api/apps/kimchi/latest.json')]);
    await t.get('/api/apps/kimchi/latest');
    await t.get('/api/apps/ryolune/releases/latest');
    await t.get('/api/apps/kimchi/files/kimchi-v0.10.0/latest.json');
    const manifest = gh.idOf('kimchi-v0.10.0', 'latest.json');
    assert.equal(gh.seen.listing, 1, 'one listing for every app and route');
    assert.equal(gh.seen.signed[manifest], 1, 'latest.json downloaded once');
    assert.equal(await t.builds.version('kimchi'), '0.10.0');
    assert.equal(gh.seen.listing, 1);

    t.clock.t += 5 * 60 * 1000 + 1;
    await t.get('/api/apps/kimchi/latest');
    assert.equal(gh.seen.listing, 2);
    assert.equal(gh.seen.signed[manifest], 2);

    // GitHub down: the last good answer keeps serving.
    gh.state.fail = true;
    t.clock.t += 5 * 60 * 1000 + 1;
    assert.equal((await t.get('/api/apps/kimchi/latest')).status, 200);
  } finally {
    await t.close();
    await gh.close();
  }
});

test('GitHub errors are a 502 in one line; the version falls back to null', async () => {
  const gh = await fakeGitHub();
  gh.state.fail = true;
  const t = await start(gh);
  try {
    for (const path of ROUTES) {
      const message = await error(await t.get(path), 502, 'api_error');
      assert.ok(!message.includes('\n') && message.includes('HTTP 500'), message);
      assert.ok(!message.includes(GH_TOKEN));
    }
    assert.equal(await t.builds.version('kimchi'), null);
  } finally {
    await t.close();
    await gh.close();
  }
  // A wrong token on the server is GitHub's 401: still a 502 for the app, not a sign-in error.
  const gh2 = await fakeGitHub();
  const t2 = await start(gh2, { token: 'wrong' });
  try {
    await error(await t2.get('/api/apps/kimchi/latest'), 502, 'api_error');
  } finally {
    await t2.close();
    await gh2.close();
  }
  // Unreachable.
  const t3 = await start({ base: 'http://127.0.0.1:9' });
  try {
    await error(await t3.get('/api/apps/kimchi/latest'), 502, 'api_error');
  } finally {
    await t3.close();
  }
});

test("the pages' versions and GET /api/apps read the builds when configured, else the fallback", async () => {
  const gh = await fakeGitHub();
  const builds = createBuilds({ token: GH_TOKEN, api: gh.base });
  try {
    assert.deepEqual(await appVersions(['kimchi', 'nori', 'ryolune', 'folio'], builds), { kimchi: '0.10.0', nori: '0.1.0', ryolune: '0.16.0', folio: '0.1.0' });
    const versions = await appVersions(APP_NAMES, builds);
    const { apps } = await appsDocument('https://lsuite.xyz', versions);
    assert.equal(apps.find((a) => a.id === 'kimchi').version, '0.10.0');
    assert.equal(apps.find((a) => a.id === 'ryolune').version, '0.16.0');
    // GitHub down and nothing cached: the fallback versions.
    gh.state.fail = true;
    const cold = createBuilds({ token: GH_TOKEN, api: gh.base });
    assert.deepEqual(await appVersions(['ryolune', 'folio'], cold), { ryolune: '0.15.3', folio: '0.1.0' });
  } finally {
    await gh.close();
  }
});
