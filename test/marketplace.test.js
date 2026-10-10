// The lsuite Marketplace (MARKETPLACE.md): every route and error, bundles checked from real
// .tar.gz files (written here by a tiny ustar writer), review, caps and storage across restarts.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer, request } from 'node:http';
import { createHash, randomBytes } from 'node:crypto';
import { mkdtemp, readdir, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { createAccounts, plansDocument } from '../ai.js';
import { compareVersions, parseToml } from '../marketplace.js';
import { tarHeader, targz } from './tar.js';

const sha = (b) => createHash('sha256').update(b).digest('hex');
const ADMIN = 'root@lsuite.xyz';

const manifestText = ({ id, version, app = 'ryolune', abi = 1, library = { macos: 'libtape.dylib', linux: 'libtape.so', windows: 'tape.dll' }, extra = '' }) =>
  [
    '# A plugin bundle, as plugin.publishLocal writes it',
    `id = "${id}"`,
    'name = "Tape warmth"',
    `version = "${version}"`,
    `app = "${app}"`,
    'kind = "effect"',
    `abi = ${abi}`,
    'description = "Saturation like a tape machine." # one line',
    'authors = [',
    '  "Ada",',
    "  'Grace', # literal",
    ']',
    extra,
    '',
    '[library]',
    ...Object.entries(library).map(([k, v]) => `${k} = "${v}"`),
    '',
  ].join('\n');

/** A bundle for `id`/`version`, with the libraries of `libs` (and anything else in `entries`). */
function bundle({ id, version, top = 'tape-warmth', libs = ['libtape.dylib', 'libtape.so', 'tape.dll'], lib = Buffer.from('\x7fELF fake library'), entries = [], manifest, ...rest }) {
  return targz([
    { path: `${top}/`, type: '5' },
    { path: `${top}/plugin.toml`, data: manifest ?? manifestText({ id, version, ...rest }) },
    ...libs.map((name) => ({ path: `${top}/${name}`, data: lib })),
    ...entries,
  ]);
}

/** The accounts service with the marketplace, a site cookie jar, and helpers per route. */
async function start(options = {}) {
  const accounts = createAccounts({ demoDelayMs: 0, admins: [ADMIN], ...options });
  const server = createServer(async (req, res) => {
    if (!(await accounts.handle(req, res, new URL(req.url, 'http://localhost')))) {
      res.writeHead(404);
      res.end();
    }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const port = server.address().port;
  const base = `http://127.0.0.1:${port}`;
  let jar = '';
  const site = async (path, body, headers = {}) => {
    const res = await fetch(base + path, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...(jar ? { cookie: jar } : {}), ...headers },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const set = res.headers.get('set-cookie');
    if (set) jar = set.split(';')[0];
    return res;
  };
  /** Signs in on the site (one account per email), picks `plan` (none: Free), and connects an app: its token. */
  const connect = async ({ plan = 'plus', email = 'ada@example.com', name = 'Ada' } = {}) => {
    jar = '';
    assert.equal((await site('/api/account/session', { email, name })).status, 200);
    if (plan) assert.equal((await site('/api/account/checkout', { plan })).status, 200);
    const { code } = await (await site('/api/account/connect', { app: 'lsuite' })).json();
    return (await (await fetch(`${base}/api/account/token`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ code }) })).json()).token;
  };
  const call = (method, path, token, { body, headers = {} } = {}) => fetch(base + path, { method, headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), ...headers }, body });
  const market = {
    catalogue: async (query = '') => (await call('GET', `/api/marketplace${query}`)).json(),
    submit: (token, body) => call('POST', '/api/marketplace/submit', token, { body: JSON.stringify(body), headers: { 'content-type': 'application/json' } }),
    put: (token, id, version, platform, data, headers = {}) => call('PUT', `/api/marketplace/submit/${id}/${version}/${platform}`, token, { body: data, headers }),
    mine: async (token) => (await call('GET', '/api/marketplace/mine', token)).json(),
    download: (token, id, query, headers = {}) => call('GET', `/api/marketplace/plugins/${id}/download${query}`, token, { headers }),
    review: (token, body) => call('POST', '/api/marketplace/review', token, { body: JSON.stringify(body), headers: { 'content-type': 'application/json' } }),
  };
  return { accounts, base, port, site, connect, call, market, close: () => new Promise((r) => server.close(r)), get jar() { return jar; } };
}

/** `{type, …}` of an error response, after checking its status. */
async function error(res, status, type) {
  assert.equal(res.status, status);
  const body = await res.json();
  assert.equal(body.type, 'error');
  assert.ok(!body.error.message.includes('\n'), 'one line');
  if (type) assert.equal(body.error.type, type, body.error.message);
  return body.error;
}

const SUB = { id: 'com.example.tape-warmth', app: 'ryolune', name: 'Tape warmth', kind: 'effect', version: '1.0.0', abi: 1, description: 'Saturation like a tape machine.', notes: 'First release.' };

/** Submits SUB at `version`, uploads macOS and Linux bundles, and approves it as the admin. */
async function publish(t, token, admin, version = '1.0.0', sub = SUB) {
  assert.equal((await t.market.submit(token, { ...sub, version })).status, 201);
  for (const platform of ['macos-arm64', 'linux-x86_64']) assert.equal((await t.market.put(token, sub.id, version, platform, bundle({ id: sub.id, version, app: sub.app }))).status, 200, platform);
  assert.equal((await t.market.review(admin, { id: sub.id, version, decision: 'approve' })).status, 200);
}

test('plugin.toml: the TOML a manifest uses', () => {
  const toml = parseToml(manifestText({ id: 'a.b', version: '1.0.0', extra: 'tags = ["x", "y"]\nmeta.size = 1_024\nflag = true\nlong = """\nTwo\nlines"""' }));
  assert.equal(toml.id, 'a.b');
  assert.equal(toml.abi, 1);
  assert.deepEqual(toml.authors, ['Ada', 'Grace']);
  assert.deepEqual(toml.library, { macos: 'libtape.dylib', linux: 'libtape.so', windows: 'tape.dll' });
  assert.equal(toml.meta.size, 1024);
  assert.equal(toml.flag, true);
  assert.equal(toml.long, 'Two\nlines');
  assert.equal(parseToml('a = "q\\"\\u00e9"\n[t]\nb = { c = 1, d = \'x\' }').a, 'q"é');
  assert.throws(() => parseToml('a = "never closed'), /line 1/);
  assert.throws(() => parseToml('a = 1\na = 2'), /twice/);
  assert.throws(() => parseToml('__proto__ = 1'), /Reserved/);
  assert.ok(compareVersions('1.10.0', '1.9.0') > 0 && compareVersions('1.0.0-beta.2', '1.0.0') < 0 && compareVersions('1.0.0-beta.10', '1.0.0-beta.2') > 0);
});

test('the catalogue: empty at first, the Pass plans, the apps; the plans say what includes it', async (t) => {
  const s = await start();
  t.after(s.close);
  assert.deepEqual(await s.market.catalogue(), { plugins: [], apps: ['ryolune', 'kimchi', 'nori', 'folio'], pass: { required: true, plans: ['plus', 'pro', 'studio'] } });
  await error(await s.call('GET', '/api/marketplace?app=photoshop'), 400, 'invalid_request_error');
  await error(await s.call('GET', '/api/marketplace/plugins/com.example.nothing'), 404, 'not_found_error');
  await error(await s.call('POST', '/api/marketplace'), 405, 'invalid_request_error');
  await error(await s.call('GET', '/api/marketplace/nope'), 404, 'not_found_error');
  const plans = plansDocument();
  assert.equal(plans.product.name, 'lsuite Pass');
  assert.deepEqual(plans.plans.map((p) => [p.id, p.marketplace]), [['free', false], ['plus', true], ['pro', true], ['studio', true]]);
});

test('publish, review, list and download: the whole path, downloads counted', async (t) => {
  const s = await start();
  t.after(s.close);
  const ada = await s.connect();
  const admin = await s.connect({ email: ADMIN, name: 'lsuite', plan: null });
  const res = await s.market.submit(ada, SUB);
  assert.equal(res.status, 201);
  const { submission } = await res.json();
  assert.equal(submission.status, 'pending');
  assert.equal(submission.verified, false);
  assert.deepEqual(submission.platforms, {});
  assert.deepEqual((await s.market.catalogue()).plugins, [], 'pending is not listed');

  const mac = bundle({ id: SUB.id, version: '1.0.0', entries: [{ path: `tape-warmth/presets/${'deep/'.repeat(20)}warm.json`, data: '{}' }] });
  const up = await s.market.put(ada, SUB.id, '1.0.0', 'macos-arm64', mac, { 'x-lsuite-sha256': sha(mac) });
  assert.equal(up.status, 200);
  assert.deepEqual((await up.json()).submission.platforms['macos-arm64'].sha256, sha(mac));
  assert.equal((await s.market.put(ada, SUB.id, '1.0.0', 'linux-x86_64', bundle({ id: SUB.id, version: '1.0.0' }))).status, 200);
  const [mine] = await s.market.mine(ada);
  assert.deepEqual([mine.id, mine.version, mine.app, mine.name, mine.status, mine.note, Object.keys(mine.platforms).sort()], [SUB.id, '1.0.0', 'ryolune', 'Tape warmth', 'pending', null, ['linux-x86_64', 'macos-arm64']]);
  assert.match(mine.submittedAt, /^\d{4}-\d\d-\d\dT/);

  // Review: admins only; the manifest, the files and the bytes to look at.
  await error(await s.call('GET', '/api/marketplace/review', ada), 403, 'permission_error');
  await error(await s.market.review(ada, { id: SUB.id, version: '1.0.0', decision: 'approve' }), 403, 'permission_error');
  await error(await s.call('GET', '/api/marketplace/review'), 401, 'authentication_error');
  const pending = await (await s.call('GET', '/api/marketplace/review', admin)).json();
  assert.equal(pending.length, 1);
  assert.deepEqual(pending[0].author, { name: 'Ada', email: 'ada@example.com', verified: false });
  const macFile = pending[0].platforms['macos-arm64'];
  assert.equal(macFile.manifest.id, SUB.id);
  assert.deepEqual(macFile.manifest.library, { macos: 'libtape.dylib', linux: 'libtape.so', windows: 'tape.dll' });
  assert.ok(macFile.files.includes('libtape.dylib') && macFile.files.includes('plugin.toml'));
  assert.ok(macFile.files.some((f) => f.endsWith('/warm.json')), 'a long path, through the ustar prefix');
  const look = await s.call('GET', `/api/marketplace/review/${SUB.id}/1.0.0/macos-arm64`, admin);
  assert.equal(look.status, 200);
  assert.deepEqual(Buffer.from(await look.arrayBuffer()), mac);
  await error(await s.call('GET', `/api/marketplace/review/${SUB.id}/1.0.0/windows-x86_64`, admin), 404, 'not_found_error');

  const approved = await s.market.review(admin, { id: SUB.id, version: '1.0.0', decision: 'approve', note: 'Clean.' });
  assert.equal(approved.status, 200);
  assert.equal((await approved.json()).submission.status, 'approved');
  assert.deepEqual(await (await s.call('GET', '/api/marketplace/review', admin)).json(), []);

  const { plugins } = await s.market.catalogue();
  assert.equal(plugins.length, 1);
  const [listing] = plugins;
  assert.deepEqual(
    { ...listing, updatedAt: typeof listing.updatedAt },
    { id: SUB.id, app: 'ryolune', name: 'Tape warmth', kind: 'effect', description: SUB.description, version: '1.0.0', abi: 1, author: { name: 'Ada', verified: false }, platforms: { 'linux-x86_64': listing.platforms['linux-x86_64'], 'macos-arm64': { size: mac.length, sha256: sha(mac) } }, downloads: 0, updatedAt: 'string', notes: 'First release.' },
  );
  assert.equal((await s.market.catalogue('?app=ryolune')).plugins.length, 1);
  assert.equal((await s.market.catalogue('?app=kimchi')).plugins.length, 0);

  // Downloads: a paid plan, the file as listed, counted once per body sent.
  const got = await s.market.download(ada, SUB.id, '?platform=macos-arm64');
  assert.equal(got.status, 200);
  assert.equal(got.headers.get('content-type'), 'application/gzip');
  assert.equal(got.headers.get('etag'), `"${sha(mac)}"`);
  assert.equal(got.headers.get('x-lsuite-sha256'), sha(mac));
  assert.equal(got.headers.get('content-length'), String(mac.length));
  assert.match(got.headers.get('content-disposition'), /filename="com\.example\.tape-warmth-1\.0\.0-macos-arm64\.tar\.gz"/);
  assert.deepEqual(Buffer.from(await got.arrayBuffer()), mac);
  assert.equal((await s.market.download(ada, SUB.id, '?platform=macos-arm64', { 'if-none-match': `"${sha(mac)}"` })).status, 304);
  assert.equal((await s.call('HEAD', `/api/marketplace/plugins/${SUB.id}/download?platform=linux-x86_64`, ada)).status, 200);
  assert.equal((await s.market.catalogue()).plugins[0].downloads, 1, '304 and HEAD are not downloads');

  // A Free account browses but doesn't install.
  const bob = await s.connect({ email: 'bob@example.com', name: 'Bob', plan: null });
  const free = await error(await s.market.download(bob, SUB.id, '?platform=macos-arm64'), 403, 'plan_required');
  assert.match(free.message, /^The marketplace comes with lsuite Pass\./);
  assert.equal(free.plan, 'free');
  assert.match(free.manage_url, /\/account$/);
  await error(await s.market.download(null, SUB.id, '?platform=macos-arm64'), 401, 'authentication_error');
  await error(await s.market.download(ada, SUB.id, '?platform=windows-x86_64'), 404, 'not_found_error');
  await error(await s.market.download(ada, SUB.id, '?platform=amiga'), 404, 'not_found_error');
  await error(await s.market.download(ada, SUB.id, ''), 400, 'invalid_request_error');
  await error(await s.market.download(ada, SUB.id, '?platform=macos-arm64&version=9.9.9'), 404, 'not_found_error');
  await error(await s.market.download(ada, 'com.example.nothing', '?platform=macos-arm64'), 404, 'not_found_error');

  // A new version: pending until approved, then the listing shows it and both versions are there.
  assert.equal((await s.market.submit(ada, { ...SUB, version: '1.1.0', notes: 'Warmer.' })).status, 201);
  const v11 = bundle({ id: SUB.id, version: '1.1.0' });
  assert.equal((await s.market.put(ada, SUB.id, '1.1.0', 'macos-arm64', v11)).status, 200);
  assert.equal((await s.market.catalogue()).plugins[0].version, '1.0.0', 'the newest approved');
  assert.equal((await s.market.review(admin, { id: SUB.id, version: '1.1.0', decision: 'approve' })).status, 200);
  const detail = await (await s.call('GET', `/api/marketplace/plugins/${SUB.id}`)).json();
  assert.equal(detail.version, '1.1.0');
  assert.equal(detail.notes, 'Warmer.');
  assert.deepEqual(Object.keys(detail.platforms), ['macos-arm64']);
  assert.deepEqual(detail.versions.map((v) => [v.version, v.notes, Object.keys(v.platforms).length]), [['1.1.0', 'Warmer.', 1], ['1.0.0', 'First release.', 2]]);
  assert.match(detail.versions[0].approvedAt, /^\d{4}-/);
  assert.equal((await s.market.download(ada, SUB.id, '?platform=macos-arm64')).headers.get('x-lsuite-sha256'), sha(v11));
  const old = await s.market.download(ada, SUB.id, '?platform=linux-x86_64&version=1.0.0');
  assert.equal(old.status, 200);
  await old.arrayBuffer();
  await error(await s.market.download(ada, SUB.id, '?platform=linux-x86_64'), 404, 'not_found_error');
  assert.equal((await s.market.catalogue()).plugins[0].downloads, 3);
});

test('owners publish their versions; xyz.lsuite. is lsuite’s; an approved version never changes', async (t) => {
  const s = await start();
  t.after(s.close);
  const ada = await s.connect();
  const bob = await s.connect({ email: 'bob@example.com', name: 'Bob' });
  const admin = await s.connect({ email: ADMIN, name: 'lsuite', plan: null });
  assert.equal((await s.market.submit(ada, SUB)).status, 201);
  await error(await s.market.submit(bob, { ...SUB, version: '2.0.0' }), 403, 'not_owner');
  await error(await s.market.put(bob, SUB.id, '1.0.0', 'macos-arm64', bundle({ id: SUB.id, version: '1.0.0' })), 403, 'not_owner');
  await error(await s.market.submit(ada, { ...SUB, id: 'xyz.lsuite.ryolune.tape' }), 403, 'not_owner');
  await error(await s.market.submit(ada, { ...SUB, app: 'kimchi', version: '1.0.1' }), 409, 'conflict_error');
  // Resubmitting a pending version updates it.
  const again = await (await s.market.submit(ada, { ...SUB, description: 'Better words.' })).json();
  assert.equal(again.submission.description, 'Better words.');
  assert.equal((await s.market.mine(ada)).length, 1);

  assert.equal((await s.market.put(ada, SUB.id, '1.0.0', 'macos-arm64', bundle({ id: SUB.id, version: '1.0.0' }))).status, 200);
  assert.equal((await s.market.review(admin, { id: SUB.id, version: '1.0.0', decision: 'approve' })).status, 200);
  await error(await s.market.submit(ada, SUB), 409, 'conflict_error');
  await error(await s.market.put(ada, SUB.id, '1.0.0', 'macos-arm64', bundle({ id: SUB.id, version: '1.0.0', lib: 'changed' })), 409, 'conflict_error');
  await error(await s.market.put(ada, SUB.id, '1.0.0', 'linux-x86_64', bundle({ id: SUB.id, version: '1.0.0' })), 409, 'conflict_error');
  await error(await s.market.review(admin, { id: SUB.id, version: '1.0.0', decision: 'approve' }), 409, 'conflict_error');

  // An admin's own: approved at once, verified, listed once a file is there; still never replaced.
  const own = { ...SUB, id: 'xyz.lsuite.ryolune.reverb', name: 'Reverb', notes: '' };
  const made = await s.market.submit(admin, own);
  assert.equal(made.status, 201);
  const { submission } = await made.json();
  assert.equal(submission.status, 'approved');
  assert.equal(submission.verified, true);
  assert.equal((await s.market.catalogue()).plugins.length, 1, 'no file yet: not listed');
  const top = 'reverb';
  assert.equal((await s.market.put(admin, own.id, '1.0.0', 'macos-arm64', bundle({ id: own.id, version: '1.0.0', top }))).status, 200);
  assert.equal((await s.market.put(admin, own.id, '1.0.0', 'linux-x86_64', bundle({ id: own.id, version: '1.0.0', top }))).status, 200, 'an admin adds a platform');
  await error(await s.market.put(admin, own.id, '1.0.0', 'macos-arm64', bundle({ id: own.id, version: '1.0.0', top, lib: 'other' })), 409, 'conflict_error');
  const listed = (await s.market.catalogue()).plugins.find((p) => p.id === own.id);
  assert.deepEqual(listed.author, { name: 'lsuite', verified: true });
  assert.deepEqual(Object.keys(listed.platforms), ['linux-x86_64', 'macos-arm64']);
  assert.deepEqual(await (await s.call('GET', '/api/marketplace/review', admin)).json(), [], 'nothing waits for review');
});

test('bundles are checked: gzip, one folder, the manifest, the library; nothing kept when refused', async (t) => {
  const s = await start();
  t.after(s.close);
  const ada = await s.connect();
  assert.equal((await s.market.submit(ada, SUB)).status, 201);
  const put = (data, platform = 'macos-arm64', headers) => s.market.put(ada, SUB.id, '1.0.0', platform, data, headers);
  const ok = { id: SUB.id, version: '1.0.0' };
  const refused = async (data, pattern, platform) => assert.match((await error(await put(data, platform), 400, 'invalid_plugin')).message, pattern);

  await refused(Buffer.from('PK\x03\x04 a zip'), /gzip/);
  await refused(gzipSync(Buffer.from('not a tar at all '.repeat(64))), /tar/);
  await refused(gzipSync(Buffer.alloc(0)), /empty/);
  await refused(bundle({ ...ok, id: 'com.example.other' }), /id = "com\.example\.other"/);
  await refused(bundle({ ...ok, version: '1.0.1' }), /version/);
  await refused(bundle({ ...ok, app: 'kimchi' }), /app/);
  await refused(bundle({ ...ok, abi: 2 }), /abi/);
  await refused(bundle({ ...ok, library: { macos: 'libtape.dylib' } }), /\[library\] linux/, 'linux-x86_64');
  await refused(bundle({ ...ok, libs: ['libtape.so'] }), /libtape\.dylib/);
  await refused(bundle({ ...ok, manifest: 'id = "unclosed' }), /TOML/);
  await refused(targz([{ path: 'tape-warmth/libtape.dylib', data: 'x' }]), /no plugin\.toml/);
  await refused(targz([{ path: 'plugin.toml', data: manifestText(ok) }, { path: 'libtape.dylib', data: 'x' }]), /one top folder/);
  await refused(bundle({ ...ok, entries: [{ path: 'second/readme.txt', data: 'x' }] }), /one top folder/);
  await refused(bundle({ ...ok, entries: [{ path: 'tape-warmth/../../evil.sh', data: 'x' }] }), /unsafe path/);
  await refused(bundle({ ...ok, entries: [{ path: 'tape-warmth/link', type: '2', link: '/etc/passwd' }] }), /links/);
  const truncated = gzipSync(Buffer.concat([tarHeader('tape-warmth/plugin.toml', 4000), Buffer.alloc(100)]));
  await refused(truncated, /ends in the middle/);

  const good = bundle(ok);
  await error(await put(good, 'macos-arm64', { 'x-lsuite-sha256': '0'.repeat(64) }), 400, 'checksum_mismatch');
  await error(await put(good, 'macos-arm64', { 'x-lsuite-sha256': 'nope' }), 400, 'invalid_request_error');
  await error(await put(good, 'amiga'), 400, 'invalid_request_error');
  await error(await s.market.put(ada, SUB.id, '9.9.9', 'macos-arm64', good), 404, 'not_found_error');
  await error(await s.market.put(ada, 'com.example.nothing', '1.0.0', 'macos-arm64', good), 404, 'not_found_error');
  await error(await s.call('PUT', `/api/marketplace/submit/${SUB.id}/1.0.0/macos-arm64`, null, { body: good }), 401, 'authentication_error');
  assert.deepEqual((await s.market.mine(ada))[0].platforms, {}, 'nothing kept');
  assert.equal(s.accounts.marketplace.usage().used, 0);
  assert.equal((await put(good)).status, 200);
  // A ./-prefixed archive (as `tar -C dir .` writes) is the same bundle.
  assert.equal((await put(targz([{ path: './', type: '5' }, { path: './tape-warmth/plugin.toml', data: manifestText(ok) }, { path: './tape-warmth/libtape.so', data: 'x' }]), 'linux-x86_64')).status, 200);
});

test('submissions: fields checked, the session works for the account page', async (t) => {
  const s = await start();
  t.after(s.close);
  const ada = await s.connect();
  for (const [field, value] of [['id', 'Tape Warmth'], ['id', 'nodots'], ['version', '1.0'], ['app', 'photoshop'], ['abi', 0], ['abi', '1'], ['name', ''], ['kind', 'Effect!'], ['description', 'two\nlines'], ['notes', 'x'.repeat(2001)]]) {
    await error(await s.market.submit(ada, { ...SUB, [field]: value }), 400, 'invalid_request_error');
  }
  await error(await s.market.submit(null, SUB), 401, 'authentication_error');
  await error(await s.call('POST', '/api/marketplace/submit', ada, { body: '{', headers: { 'content-type': 'application/json' } }), 400, 'invalid_request_error');

  // The account page: the session cookie reads, and posts from the same site only.
  const res = await s.site('/api/marketplace/submit', SUB);
  assert.equal(res.status, 201);
  assert.equal((await (await s.site('/api/marketplace/mine')).json()).length, 1);
  await error(await s.site('/api/marketplace/submit', { ...SUB, version: '1.0.1' }, { origin: 'https://evil.example' }), 403, 'permission_error');
  // Uploads take the app token only.
  await error(await s.call('PUT', `/api/marketplace/submit/${SUB.id}/1.0.0/macos-arm64`, null, { body: bundle({ id: SUB.id, version: '1.0.0' }), headers: { cookie: s.jar } }), 401, 'authentication_error');
  const me = await (await s.site('/api/account/me')).json();
  assert.equal(me.admin, false);
});

test('review: reject keeps the note and drops the files; resubmit; take down; admins on the site', async (t) => {
  const s = await start();
  t.after(s.close);
  const ada = await s.connect();
  const admin = await s.connect({ email: ADMIN, name: 'lsuite', plan: null });
  assert.equal((await (await s.site('/api/account/me')).json()).admin, true, 'the site session is the admin’s now');
  assert.equal((await s.market.submit(ada, SUB)).status, 201);
  await error(await s.market.review(admin, { id: SUB.id, version: '1.0.0', decision: 'approve' }), 400, 'invalid_request_error');
  await error(await s.market.review(admin, { id: SUB.id, version: '1.0.0', decision: 'maybe' }), 400, 'invalid_request_error');
  await error(await s.market.review(admin, { id: SUB.id, version: '2.0.0', decision: 'reject' }), 404, 'not_found_error');
  assert.equal((await s.market.put(ada, SUB.id, '1.0.0', 'macos-arm64', bundle({ id: SUB.id, version: '1.0.0' }))).status, 200);
  assert.ok(s.accounts.marketplace.usage().used > 0);

  // Rejected from the account page (session, same origin).
  await error(await s.site('/api/marketplace/review', { id: SUB.id, version: '1.0.0', decision: 'reject', note: 'x' }, { origin: 'https://evil.example' }), 403, 'permission_error');
  const rejected = await s.site('/api/marketplace/review', { id: SUB.id, version: '1.0.0', decision: 'reject', note: 'The library writes outside its folder.' });
  assert.equal(rejected.status, 200);
  const [mine] = await s.market.mine(ada);
  assert.deepEqual([mine.status, mine.note, mine.platforms], ['rejected', 'The library writes outside its folder.', {}]);
  assert.equal(s.accounts.marketplace.usage().used, 0, 'files dropped');
  await error(await s.market.put(ada, SUB.id, '1.0.0', 'macos-arm64', bundle({ id: SUB.id, version: '1.0.0' })), 409, 'conflict_error');
  await error(await s.market.review(admin, { id: SUB.id, version: '1.0.0', decision: 'reject' }), 409, 'conflict_error');

  // Submitted again: pending, the note cleared; then approved, then taken down.
  const again = await (await s.market.submit(ada, SUB)).json();
  assert.deepEqual([again.submission.status, again.submission.note], ['pending', null]);
  assert.equal((await s.market.put(ada, SUB.id, '1.0.0', 'macos-arm64', bundle({ id: SUB.id, version: '1.0.0' }))).status, 200);
  assert.equal((await s.market.review(admin, { id: SUB.id, version: '1.0.0', decision: 'approve' })).status, 200);
  assert.equal((await s.market.catalogue()).plugins.length, 1);
  assert.equal((await s.market.review(admin, { id: SUB.id, version: '1.0.0', decision: 'reject', note: 'Taken down.' })).status, 200);
  assert.equal((await s.market.catalogue()).plugins.length, 0);
});

test('caps: Content-Length, the largest file, the total, the disk', async (t) => {
  const statfs = { free: 1e12 };
  const s = await start({ market: { maxFile: 6000, total: 9000 }, cloudDiskReserve: 1000, cloudStatfs: async () => ({ bavail: statfs.free, bsize: 1 }) });
  t.after(s.close);
  const ada = await s.connect();
  for (const version of ['1.0.0', '1.0.1', '1.0.2']) assert.equal((await s.market.submit(ada, { ...SUB, version })).status, 201);
  const big = (version, n) => bundle({ id: SUB.id, version, libs: ['libtape.dylib'], lib: randomBytes(n) });

  // No Content-Length: a chunked body.
  const chunked = await new Promise((resolve, reject) => {
    const req = request({ host: '127.0.0.1', port: s.port, method: 'PUT', path: `/api/marketplace/submit/${SUB.id}/1.0.0/macos-arm64`, headers: { authorization: `Bearer ${ada}`, 'transfer-encoding': 'chunked' } }, (res) => {
      let text = '';
      res.on('data', (c) => (text += c));
      res.on('end', () => resolve({ status: res.statusCode, body: JSON.parse(text) }));
    });
    req.on('error', reject);
    req.end(big('1.0.0', 100));
  });
  assert.equal(chunked.status, 411);
  assert.equal(chunked.body.error.type, 'length_required');

  const tooBig = await error(await s.market.put(ada, SUB.id, '1.0.0', 'macos-arm64', big('1.0.0', 7000)), 413, 'request_too_large');
  assert.equal(tooBig.max_file, 6000);
  assert.equal((await s.market.put(ada, SUB.id, '1.0.0', 'macos-arm64', big('1.0.0', 4500))).status, 200);
  const full = await error(await s.market.put(ada, SUB.id, '1.0.1', 'macos-arm64', big('1.0.1', 4500)), 507, 'storage_full');
  assert.match(full.message, /full/);
  // Replacing a pending file frees its room first.
  assert.equal((await s.market.put(ada, SUB.id, '1.0.0', 'macos-arm64', big('1.0.0', 4400))).status, 200);
  statfs.free = 1200;
  await error(await s.market.put(ada, SUB.id, '1.0.2', 'macos-arm64', big('1.0.2', 100)), 507, 'storage_full');
});

test('the index and the files outlive a restart; leftovers are swept', async (t) => {
  const dataDir = await mkdtemp(join(tmpdir(), 'lsuite-market-test-'));
  t.after(() => rm(dataDir, { recursive: true, force: true }));
  const first = await start({ dataDir });
  const ada = await first.connect();
  const admin = await first.connect({ email: ADMIN, name: 'lsuite', plan: null });
  await publish(first, ada, admin);
  assert.equal((await first.market.submit(ada, { ...SUB, version: '1.1.0' })).status, 201);
  const got = await first.market.download(ada, SUB.id, '?platform=macos-arm64');
  const bytes = Buffer.from(await got.arrayBuffer());
  await new Promise((r) => setTimeout(r, 400)); // download counts are written a moment later
  await first.close();

  const root = join(dataDir, 'marketplace');
  assert.equal((await stat(join(root, 'index.json'))).mode & 0o777, 0o600);
  const files = await readdir(join(root, 'files'));
  assert.ok(files.includes(sha(bytes)));
  await writeFile(join(root, 'files', 'f'.repeat(64)), 'left over');

  const second = await start({ dataDir });
  t.after(second.close);
  const { plugins } = await second.market.catalogue();
  assert.deepEqual([plugins[0].id, plugins[0].version, plugins[0].downloads], [SUB.id, '1.0.0', 1]);
  const ada2 = await second.connect();
  assert.deepEqual((await second.market.mine(ada2)).map((m) => [m.version, m.status]), [['1.1.0', 'pending'], ['1.0.0', 'approved']]);
  const again = await second.market.download(ada2, SUB.id, '?platform=macos-arm64');
  assert.deepEqual(Buffer.from(await again.arrayBuffer()), bytes);
  assert.ok(!(await readdir(join(root, 'files'))).includes('f'.repeat(64)), 'swept');
});
