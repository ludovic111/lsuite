// lsuite Cloud (CLOUD.md): every route and error type, the caps, folders, moves, paths, storage.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer, request } from 'node:http';
import { createHash, randomBytes } from 'node:crypto';
import { mkdtemp, readdir, readFile, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createAccounts } from '../ai.js';
import { createCloud, cloudPath, storageLabel } from '../cloud.js';
import { handle, plansHtml, APP_NAMES } from '../server.js';

const sha = (b) => createHash('sha256').update(b).digest('hex');
const enc = (path) => path.split('/').map(encodeURIComponent).join('/');

/** A running accounts service with lsuite Cloud, a cookie jar for the site, and helpers per route. */
async function start(options = {}) {
  const accounts = createAccounts({ demoDelayMs: 0, ...options });
  const server = createServer(async (req, res) => {
    if (!(await accounts.handle(req, res, new URL(req.url, 'http://localhost')))) {
      res.writeHead(404);
      res.end();
    }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  let jar = '';
  const site = async (path, body) => {
    const res = await fetch(base + path, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...(jar ? { cookie: jar } : {}) },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const set = res.headers.get('set-cookie');
    if (set) jar = set.split(';')[0];
    return res;
  };
  /** Signs in (a new account each email), picks a plan, and connects an app: its token. */
  const connect = async ({ plan = 'plus', email = 'ada@example.com' } = {}) => {
    jar = '';
    assert.equal((await site('/api/account/session', { email, name: 'Ada' })).status, 200);
    if (plan) assert.equal((await site('/api/account/checkout', { plan })).status, 200);
    const { code } = await (await site('/api/account/connect', { app: 'kimchi' })).json();
    return (await (await fetch(`${base}/api/account/token`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ code }) })).json()).token;
  };
  const call = (method, path, token, { body, headers = {} } = {}) =>
    fetch(base + path, { method, headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), ...headers }, body });
  const cloud = {
    status: (token) => call('GET', '/api/cloud', token),
    list: async (token) => (await call('GET', '/api/cloud/files', token)).json(),
    put: (token, path, data, headers = {}) => call('PUT', `/api/cloud/files/${enc(path)}`, token, { body: Buffer.from(data), headers }),
    get: (token, path, headers = {}) => call('GET', `/api/cloud/files/${enc(path)}`, token, { headers }),
    head: (token, path) => call('HEAD', `/api/cloud/files/${enc(path)}`, token),
    del: (token, path) => call('DELETE', `/api/cloud/files/${enc(path)}`, token),
    mkdir: (token, path) => call('POST', '/api/cloud/folders', token, { body: JSON.stringify({ path }), headers: { 'content-type': 'application/json' } }),
    move: (token, body) => call('POST', '/api/cloud/move', token, { body: JSON.stringify(body), headers: { 'content-type': 'application/json' } }),
  };
  /** A request with its path sent exactly as written (fetch would resolve `..`). */
  const raw = (method, path, token, body) =>
    new Promise((resolve, reject) => {
      const req = request({ host: '127.0.0.1', port: server.address().port, method, path, headers: { authorization: `Bearer ${token}` } }, (res) => {
        let text = '';
        res.on('data', (c) => (text += c));
        res.on('end', () => resolve({ status: res.statusCode, body: text ? JSON.parse(text) : null }));
      });
      req.on('error', reject);
      req.end(body);
    });
  return { accounts, base, site, connect, call, cloud, raw, close: () => new Promise((r) => server.close(r)), get jar() { return jar; } };
}

/** `{type, …}` of an error response, after checking its status. */
async function error(res, status) {
  assert.equal(res.status, status);
  const body = await res.json();
  assert.equal(body.type, 'error');
  assert.equal(typeof body.error.message, 'string');
  assert.ok(!body.error.message.includes('\n'), 'one line');
  return body.error;
}

test('upload, replace, download, HEAD, ETag and If-None-Match', async () => {
  const t = await start();
  try {
    const token = await t.connect();
    const status = await (await t.cloud.status(token)).json();
    assert.deepEqual(
      { ...status, manageUrl: undefined },
      { plan: 'plus', planName: 'Plus', quota: 100e6, used: 0, files: 0, folders: 0, maxFile: 25e6, demo: true, manageUrl: undefined },
    );
    assert.match(status.manageUrl, /\/account$/);

    // New: 201, parents appear by themselves, the checksum is checked, the date kept.
    const one = Buffer.from('first version');
    const put = await t.cloud.put(token, 'Projects/kimchi/Demo cut é.kimchi', one, { 'x-lsuite-sha256': sha(one).toUpperCase(), 'x-lsuite-modified': '2026-10-01T12:30:00+02:00' });
    assert.equal(put.status, 201);
    const created = await put.json();
    assert.deepEqual(created.file, { path: 'Projects/kimchi/Demo cut é.kimchi', size: one.length, sha256: sha(one), modifiedAt: '2026-10-01T10:30:00.000Z' });
    assert.equal(created.used, one.length);
    assert.equal(created.quota, 100e6);

    const get = await t.cloud.get(token, 'Projects/kimchi/Demo cut é.kimchi');
    assert.equal(get.status, 200);
    assert.equal(get.headers.get('content-type'), 'application/octet-stream');
    assert.equal(get.headers.get('content-length'), String(one.length));
    assert.equal(get.headers.get('etag'), `"${sha(one)}"`);
    assert.equal(get.headers.get('x-lsuite-modified'), '2026-10-01T10:30:00.000Z');
    assert.equal(get.headers.get('content-disposition'), `attachment; filename="Demo cut _.kimchi"; filename*=UTF-8''Demo%20cut%20%C3%A9.kimchi`);
    assert.equal(get.headers.get('cache-control'), 'no-store');
    assert.deepEqual(Buffer.from(await get.arrayBuffer()), one);

    const head = await t.cloud.head(token, 'Projects/kimchi/Demo cut é.kimchi');
    assert.equal(head.status, 200);
    assert.equal(head.headers.get('content-length'), String(one.length));
    assert.equal(head.headers.get('etag'), `"${sha(one)}"`);
    assert.equal((await head.arrayBuffer()).byteLength, 0);

    // A conditional GET: unchanged → 304.
    const same = await t.cloud.get(token, 'Projects/kimchi/Demo cut é.kimchi', { 'if-none-match': `"${sha(one)}"` });
    assert.equal(same.status, 304);
    assert.equal(same.headers.get('etag'), `"${sha(one)}"`);
    assert.equal((await t.cloud.get(token, 'Projects/kimchi/Demo cut é.kimchi', { 'if-none-match': '"other"' })).status, 200);

    // Replaced: 200, the old size freed, the date now.
    const two = randomBytes(3 * 1024 * 1024 + 17);
    const before = Date.now();
    const replaced = await t.cloud.put(token, 'Projects/kimchi/Demo cut é.kimchi', two);
    assert.equal(replaced.status, 200);
    const r = await replaced.json();
    assert.equal(r.used, two.length);
    assert.ok(Date.parse(r.file.modifiedAt) >= before - 1000);
    assert.equal(sha(Buffer.from(await (await t.cloud.get(token, 'Projects/kimchi/Demo cut é.kimchi')).arrayBuffer())), sha(two));

    // If-None-Match: * refuses to replace, and creates when nothing is there.
    const exists = await error(await t.cloud.put(token, 'Projects/kimchi/Demo cut é.kimchi', 'x', { 'if-none-match': '*' }), 412);
    assert.equal(exists.type, 'precondition_failed');
    assert.equal((await t.cloud.put(token, 'Projects/new.txt', 'x', { 'if-none-match': '*' })).status, 201);

    // A body that doesn't match its checksum is not saved.
    const bad = await error(await t.cloud.put(token, 'Projects/bad.txt', 'hello', { 'x-lsuite-sha256': sha('other') }), 400);
    assert.equal(bad.type, 'checksum_mismatch');
    assert.equal((await error(await t.cloud.put(token, 'Projects/bad.txt', 'hello', { 'x-lsuite-sha256': 'nothex' }), 400)).type, 'invalid_request_error');
    assert.equal((await error(await t.cloud.put(token, 'Projects/bad.txt', 'hello', { 'x-lsuite-modified': 'yesterday' }), 400)).type, 'invalid_request_error');
    assert.equal((await error(await t.cloud.get(token, 'Projects/bad.txt'), 404)).type, 'not_found_error');

    // The flat list, sorted, and the status counts.
    const list = await t.cloud.list(token);
    assert.deepEqual(list.files.map((f) => f.path), ['Projects/kimchi/Demo cut é.kimchi', 'Projects/new.txt']);
    assert.deepEqual(Object.keys(list.files[0]).sort(), ['modifiedAt', 'path', 'sha256', 'size']);
    assert.deepEqual(list.folders, []);
    assert.equal(list.used, two.length + 1);
    assert.equal(list.quota, 100e6);
    const after = await (await t.cloud.status(token)).json();
    assert.equal(after.files, 2);
    assert.equal(after.folders, 2, 'Projects and Projects/kimchi');

    // Zero bytes is a file too.
    assert.equal((await t.cloud.put(token, 'empty', '')).status, 201);
    assert.equal((await t.cloud.get(token, 'empty')).headers.get('etag'), `"${sha('')}"`);

    // Unknown routes and methods.
    assert.equal((await t.call('POST', '/api/cloud/files/x', token)).status, 405);
    assert.equal((await t.call('GET', '/api/cloud/nothing', token)).status, 404);
    assert.equal((await t.call('GET', '/api/cloud/move', token)).status, 405);
  } finally {
    await t.close();
  }
});

test('auth: the app token for everything, the session cookie for reading only', async () => {
  const t = await start();
  try {
    assert.equal((await error(await t.cloud.status(null), 401)).type, 'authentication_error');
    assert.equal((await error(await t.cloud.put('lsk_nope_nope_nope_nope_nope_nope', 'a.txt', 'x'), 401)).type, 'authentication_error');
    const token = await t.connect();
    await t.cloud.put(token, 'a.txt', 'hello');
    const cookie = { cookie: t.jar };
    assert.equal((await t.call('GET', '/api/cloud', null, { headers: cookie })).status, 200);
    assert.equal((await t.call('GET', '/api/cloud/files', null, { headers: cookie })).status, 200);
    assert.equal(await (await t.call('GET', '/api/cloud/files/a.txt', null, { headers: cookie })).text(), 'hello');
    assert.equal((await t.call('PUT', '/api/cloud/files/b.txt', null, { headers: cookie, body: 'x' })).status, 401);
    assert.equal((await t.call('DELETE', '/api/cloud/files/a.txt', null, { headers: cookie })).status, 401);
  } finally {
    await t.close();
  }
});

test('Content-Length is required: a chunked body gets 411', async () => {
  const t = await start();
  try {
    const token = await t.connect();
    const body = new ReadableStream({
      start(c) {
        c.enqueue(new TextEncoder().encode('chunk one, '));
        c.enqueue(new TextEncoder().encode('chunk two'));
        c.close();
      },
    });
    const res = await fetch(`${t.base}/api/cloud/files/chunked.txt`, { method: 'PUT', headers: { authorization: `Bearer ${token}` }, body, duplex: 'half' });
    assert.equal((await error(res, 411)).type, 'length_required');
    assert.deepEqual((await t.cloud.list(token)).files, []);
  } finally {
    await t.close();
  }
});

test('caps: the largest file (413), the quota (507, replacing frees the old size) and the demo total', async () => {
  const t = await start({ cloud: { quota: 1000, maxFile: 600, total: 1500 } });
  try {
    const ada = await t.connect();
    const plans = await (await fetch(`${t.base}/api/ai/plans`)).json();
    assert.deepEqual(plans.cloudDemo, { quota: 1000, maxFile: 600 });

    const big = await error(await t.cloud.put(ada, 'big.bin', Buffer.alloc(601)), 413);
    assert.equal(big.type, 'request_too_large');
    assert.equal(big.max_file, 600);

    assert.equal((await t.cloud.put(ada, 'a.bin', Buffer.alloc(600))).status, 201);
    const full = await error(await t.cloud.put(ada, 'b.bin', Buffer.alloc(401)), 507);
    assert.equal(full.type, 'storage_full');
    assert.equal(full.used, 600);
    assert.equal(full.quota, 1000);
    assert.match(full.manage_url, /\/account$/);
    assert.equal(full.plan, 'plus');
    assert.equal((await t.cloud.put(ada, 'b.bin', Buffer.alloc(400))).status, 201);
    // Replacing a.bin frees its 600 bytes first: 400 + 600 fits again.
    assert.equal((await t.cloud.put(ada, 'a.bin', Buffer.alloc(600, 1))).status, 200);
    assert.equal((await (await t.cloud.status(ada)).json()).used, 1000);

    // Every demo account together: 1500 bytes. Ada has 1000.
    const bob = await t.connect({ email: 'bob@example.com' });
    assert.equal((await t.cloud.put(bob, 'c.bin', Buffer.alloc(500))).status, 201);
    const total = await error(await t.cloud.put(bob, 'd.bin', Buffer.alloc(1)), 507);
    assert.equal(total.type, 'storage_full');
    assert.match(total.message, /demo is full/);
    assert.equal(total.used, 500);
    assert.equal(total.quota, 1000);
    // Deleting frees room for everyone.
    assert.equal((await t.cloud.del(ada, 'b.bin')).status, 200);
    assert.equal((await t.cloud.put(bob, 'd.bin', Buffer.alloc(400))).status, 201);
  } finally {
    await t.close();
  }
});

test('the disk keeps its reserve whatever the caps say (the accounts share it)', async () => {
  // A disk with 1100 bytes free and 1000 kept back: 100 bytes of room for uploads.
  let free = 1100;
  const statfs = async () => ({ bavail: free, bsize: 1 });
  const t = await start({ cloud: { quota: 1e9, maxFile: 1e9, total: 1e9 }, cloudDiskReserve: 1000, cloudStatfs: statfs });
  try {
    const token = await t.connect();
    assert.equal((await t.cloud.put(token, 'small.bin', Buffer.alloc(100))).status, 201);
    free = 1000;
    const full = await error(await t.cloud.put(token, 'more.bin', Buffer.alloc(1)), 507);
    assert.equal(full.type, 'storage_full');
    assert.match(full.message, /no room left on this server/);
    assert.equal((await t.cloud.list(token)).files.length, 1);
  } finally {
    await t.close();
  }
});

test('two uploads at once cannot both take the last of the room', async () => {
  const t = await start({ cloud: { quota: 1000, maxFile: 1000, total: 1e9 } });
  try {
    const token = await t.connect();
    const results = await Promise.all([t.cloud.put(token, 'one.bin', Buffer.alloc(600)), t.cloud.put(token, 'two.bin', Buffer.alloc(600))]);
    assert.deepEqual(results.map((r) => r.status).sort(), [201, 507]);
    assert.equal((await (await t.cloud.status(token)).json()).used, 600);
  } finally {
    await t.close();
  }
});

test('Free: nothing new (plan_required), but list, download and delete still work', async () => {
  const t = await start();
  try {
    const token = await t.connect({ plan: 'pro' });
    await t.cloud.put(token, 'Keep/a.txt', 'kept');
    await t.cloud.put(token, 'Keep/b.txt', 'gone');
    assert.equal((await t.site('/api/account/checkout', { plan: 'free' })).status, 200);

    const status = await (await t.cloud.status(token)).json();
    assert.equal(status.plan, 'free');
    assert.equal(status.quota, 0);
    assert.equal(status.maxFile, 0);
    assert.equal(status.files, 2);
    for (const res of [await t.cloud.put(token, 'new.txt', 'x'), await t.cloud.mkdir(token, 'New'), await t.cloud.move(token, { from: 'Keep/a.txt', to: 'Keep/c.txt' })]) {
      const err = await error(res, 403);
      assert.equal(err.type, 'plan_required');
      assert.match(err.message, /^lsuite Cloud comes with lsuite Pass\./);
      assert.match(err.manage_url, /\/account$/);
      assert.equal(err.plan, 'free');
    }
    assert.equal((await t.cloud.list(token)).files.length, 2);
    assert.equal(await (await t.cloud.get(token, 'Keep/a.txt')).text(), 'kept');
    const del = await (await t.cloud.del(token, 'Keep/b.txt')).json();
    assert.deepEqual(del, { deleted: 1, used: 4, quota: 0 });
  } finally {
    await t.close();
  }
});

test('folders: implicit and explicit, never also a file, deleted with everything in them', async () => {
  const t = await start();
  try {
    const token = await t.connect();
    const made = await t.cloud.mkdir(token, 'Empty/Inner');
    assert.equal(made.status, 201);
    const { folder } = await made.json();
    assert.equal(folder.path, 'Empty/Inner');
    assert.ok(Date.parse(folder.createdAt));
    assert.equal((await t.cloud.mkdir(token, 'Empty/Inner')).status, 200, 'already there');
    await t.cloud.put(token, 'Music/song.ryolune', 'la la');
    assert.equal((await t.cloud.mkdir(token, 'Music')).status, 200, 'there, through its file');

    const list = await t.cloud.list(token);
    assert.deepEqual(list.folders.map((f) => f.path), ['Empty/Inner', 'Music'], 'only folders created on their own');
    assert.equal((await (await t.cloud.status(token)).json()).folders, 3, 'Empty, Empty/Inner, Music');

    // A path is a file or a folder, never both.
    let err = await error(await t.cloud.put(token, 'Music', 'x'), 409);
    assert.equal(err.type, 'conflict_error');
    assert.equal(err.path, 'Music');
    assert.equal((await error(await t.cloud.put(token, 'Empty/Inner', 'x'), 409)).path, 'Empty/Inner');
    err = await error(await t.cloud.put(token, 'Music/song.ryolune/inside.txt', 'x'), 409);
    assert.equal(err.path, 'Music/song.ryolune');
    assert.equal((await error(await t.cloud.mkdir(token, 'Music/song.ryolune'), 409)).type, 'conflict_error');
    assert.equal((await error(await t.cloud.mkdir(token, 'Music/song.ryolune/Sub'), 409)).path, 'Music/song.ryolune');
    assert.equal((await error(await t.cloud.get(token, 'Music'), 409)).type, 'conflict_error');

    // Deleting a folder takes everything in it.
    await t.cloud.put(token, 'Empty/Inner/deep/one.txt', '1');
    await t.cloud.put(token, 'Empty/two.txt', '22');
    await t.cloud.put(token, 'Emptyish.txt', '333');
    const del = await (await t.cloud.del(token, 'Empty')).json();
    assert.deepEqual(del, { deleted: 2, used: 5 + 3, quota: 100e6 });
    const left = await t.cloud.list(token);
    assert.deepEqual(left.files.map((f) => f.path), ['Emptyish.txt', 'Music/song.ryolune']);
    assert.deepEqual(left.folders.map((f) => f.path), ['Music']);
    assert.equal((await error(await t.cloud.del(token, 'Empty'), 404)).type, 'not_found_error');
    // An empty folder of its own goes too, with nothing deleted.
    await t.cloud.mkdir(token, 'Lonely');
    assert.deepEqual(await (await t.cloud.del(token, 'Lonely')).json(), { deleted: 0, used: 8, quota: 100e6 });
  } finally {
    await t.close();
  }
});

test('move: rename, new parents, overwrite, conflicts, folders with their content', async () => {
  const t = await start();
  try {
    const token = await t.connect();
    await t.cloud.put(token, 'a.txt', 'aaa');
    await t.cloud.put(token, 'b.txt', 'bb');
    const names = async () => (await t.cloud.list(token)).files.map((f) => `${f.path}=${f.size}`);

    // A file: renamed, then into folders that don't exist yet.
    assert.deepEqual(await (await t.cloud.move(token, { from: 'a.txt', to: 'c.txt' })).json(), { moved: 1, used: 5, quota: 100e6 });
    assert.equal((await t.cloud.move(token, { from: 'c.txt', to: 'New/Parent/c.txt' })).status, 200);
    assert.deepEqual(await names(), ['New/Parent/c.txt=3', 'b.txt=2']);
    assert.equal((await error(await t.cloud.move(token, { from: 'nope.txt', to: 'x.txt' }), 404)).type, 'not_found_error');

    // Onto an existing file: refused, unless overwrite (its size is freed).
    let err = await error(await t.cloud.move(token, { from: 'b.txt', to: 'New/Parent/c.txt' }), 409);
    assert.equal(err.type, 'conflict_error');
    assert.equal(err.path, 'New/Parent/c.txt');
    const over = await (await t.cloud.move(token, { from: 'b.txt', to: 'New/Parent/c.txt', overwrite: true })).json();
    assert.deepEqual(over, { moved: 1, used: 2, quota: 100e6 });
    assert.deepEqual(await names(), ['New/Parent/c.txt=2']);

    // A file onto a folder, or under a file: refused even with overwrite.
    await t.cloud.put(token, 'f.txt', 'f');
    assert.equal((await error(await t.cloud.move(token, { from: 'f.txt', to: 'New', overwrite: true }), 409)).path, 'New');
    assert.equal((await error(await t.cloud.move(token, { from: 'New', to: 'f.txt', overwrite: true }), 409)).path, 'f.txt');
    assert.equal((await error(await t.cloud.move(token, { from: 'New', to: 'f.txt/New' }), 409)).path, 'f.txt');

    // A folder with its content (and its own empty folders); never into itself.
    await t.cloud.mkdir(token, 'New/Empty');
    assert.equal((await error(await t.cloud.move(token, { from: 'New', to: 'New/Inside' }), 409)).path, 'New/Inside');
    const moved = await (await t.cloud.move(token, { from: 'New', to: 'Archive/2026' })).json();
    assert.equal(moved.moved, 1);
    assert.deepEqual(await names(), ['Archive/2026/Parent/c.txt=2', 'f.txt=1']);
    assert.deepEqual((await t.cloud.list(token)).folders.map((f) => f.path), ['Archive/2026/Empty']);

    // A folder onto a folder: refused, or merged with overwrite.
    await t.cloud.put(token, 'Other/Parent/c.txt', 'replaced!');
    await t.cloud.put(token, 'Other/Parent/d.txt', 'dd');
    assert.equal((await error(await t.cloud.move(token, { from: 'Other', to: 'Archive/2026' }), 409)).path, 'Archive/2026');
    const merged = await (await t.cloud.move(token, { from: 'Other', to: 'Archive/2026', overwrite: true })).json();
    assert.equal(merged.moved, 2);
    assert.deepEqual(await names(), ['Archive/2026/Parent/c.txt=9', 'Archive/2026/Parent/d.txt=2', 'f.txt=1']);
    // A merge that would put a file where a folder is moves nothing.
    await t.cloud.put(token, 'Clash/Parent', 'a file named like the folder');
    assert.equal((await error(await t.cloud.move(token, { from: 'Clash', to: 'Archive/2026', overwrite: true }), 409)).type, 'conflict_error');
    assert.deepEqual(await names(), ['Archive/2026/Parent/c.txt=9', 'Archive/2026/Parent/d.txt=2', 'Clash/Parent=28', 'f.txt=1']);
    // Moving onto itself changes nothing.
    assert.equal((await (await t.cloud.move(token, { from: 'f.txt', to: 'f.txt' })).json()).moved, 0);
  } finally {
    await t.close();
  }
});

test('paths: the rules of CLOUD.md, in URLs and in JSON', async () => {
  const t = await start();
  try {
    const token = await t.connect();
    const refused = async (res, path) => {
      const err = await error(res, 400);
      assert.equal(err.type, 'invalid_path', JSON.stringify(path));
      assert.equal(typeof err.path, 'string');
      return err;
    };
    // Sent as they are: fetch would resolve these before they leave.
    for (const path of ['a/../b.txt', '../b.txt', 'a/./b.txt', 'a/%2e%2e/b.txt', 'a/%2E%2E', 'a%2Fb.txt', 'a/%5Cb.txt', 'bad%zz', 'bad%e9', 'nul%00.txt', 'a//b.txt', 'a/', 'tab%09.txt', 'space%20', '%20space']) {
      const res = await t.raw('PUT', `/api/cloud/files/${path}`, token, 'x');
      assert.equal(res.status, 400, path);
      assert.equal(res.body.error.type, 'invalid_path', path);
    }
    assert.equal((await t.raw('GET', '/api/cloud/files/a/../../ai/plans', token)).body.error.type, 'invalid_path');
    assert.equal((await t.raw('DELETE', '/api/cloud/files/..', token)).body.error.type, 'invalid_path');
    // Lengths in UTF-8 bytes: 255 per name, 1024 in all.
    assert.equal((await t.cloud.put(token, 'é'.repeat(127) + 'a', 'x')).status, 201, '255 bytes');
    await refused(await t.cloud.put(token, 'é'.repeat(128), 'x'), '256 bytes');
    await refused(await t.cloud.put(token, Array(6).fill('n'.repeat(200)).join('/'), 'x'), 'over 1024');
    // Case-sensitive: two files.
    assert.equal((await t.cloud.put(token, 'Case.txt', '1')).status, 201);
    assert.equal((await t.cloud.put(token, 'case.txt', '2')).status, 201);
    // JSON bodies take plain paths, with the same rules.
    for (const path of ['', '/abs', 'trail/', '.', 'a/..', 'a\\b', 'a\u0007b', ' lead', 42, null]) await refused(await t.cloud.mkdir(token, path), path);
    await refused(await t.cloud.move(token, { from: 'Case.txt', to: '../out.txt' }), 'to');
    await refused(await t.cloud.move(token, { from: 'Case.txt' }), 'missing to');
    // Percent signs in a JSON path are just characters.
    assert.equal((await t.cloud.mkdir(token, '100%25 sure')).status, 201);
    assert.ok((await t.cloud.list(token)).folders.some((f) => f.path === '100%25 sure'));
    // The function itself.
    assert.equal(cloudPath('Projects/r%C3%A9sum%C3%A9.folio', true), 'Projects/résumé.folio');
    assert.equal(cloudPath('Projects/r%C3%A9sum%C3%A9.folio'), 'Projects/r%C3%A9sum%C3%A9.folio');
  } finally {
    await t.close();
  }
});

test('storage: content stored once, counted per name, 0600, kept across restarts; over quota after a downgrade', async (ctx) => {
  const dir = await mkdtemp(join(tmpdir(), 'lsuite-cloud-test-'));
  ctx.after(() => rm(dir, { recursive: true, force: true }));
  const t = await start({ dataDir: dir, cloud: { quota: 1000, maxFile: 1000, total: 10000 } });
  let token;
  let userDir;
  const data = Buffer.from('the same content, twice');
  try {
    token = await t.connect();
    const user = Object.values(t.accounts.store.data.users)[0];
    userDir = join(dir, 'cloud', user.id);
    assert.equal((await t.cloud.put(token, 'one.txt', data)).status, 201);
    const second = await (await t.cloud.put(token, 'Copies/two.txt', data)).json();
    assert.equal(second.used, data.length * 2, 'counted once per name');
    assert.deepEqual(await readdir(join(userDir, 'blobs')), [sha(data)], 'stored once');
    assert.equal((await stat(join(userDir, 'index.json'))).mode & 0o777, 0o600);
    assert.equal((await stat(join(userDir, 'blobs', sha(data)))).mode & 0o777, 0o600);
    const index = JSON.parse(await readFile(join(userDir, 'index.json'), 'utf8'));
    assert.equal(index.format, 1);
    assert.deepEqual(Object.keys(index.files).sort(), ['Copies/two.txt', 'one.txt']);
    assert.deepEqual(index.folders, {});
    // One name gone: the content stays for the other.
    await t.cloud.del(token, 'one.txt');
    assert.deepEqual(await readdir(join(userDir, 'blobs')), [sha(data)]);
    await t.cloud.put(token, 'big.bin', Buffer.alloc(900));
    assert.equal((await readdir(join(userDir, 'blobs'))).length, 2);
    // Replaced by other bytes: the old content goes.
    await t.cloud.put(token, 'Copies/two.txt', 'different');
    assert.deepEqual((await readdir(join(userDir, 'blobs'))).sort(), [sha(Buffer.alloc(900)), sha('different')].sort());
    assert.deepEqual(await readdir(join(dir, 'cloud', '.tmp')), [], 'no upload left behind');
  } finally {
    await t.close();
  }

  // A new process on the same data: everything is there. With a smaller quota (a downgrade),
  // uploads are refused but deleting still works.
  const again = await start({ dataDir: dir, cloud: { quota: 500, maxFile: 1000, total: 10000 } });
  try {
    const list = await again.cloud.list(token);
    assert.deepEqual(list.files.map((f) => f.path), ['Copies/two.txt', 'big.bin']);
    assert.equal(list.used, 909);
    assert.equal(await (await again.cloud.get(token, 'Copies/two.txt')).text(), 'different');
    const me = await (await again.call('GET', '/api/account/me', token)).json();
    assert.deepEqual(me.cloud, { used: 909, quota: 500, files: 2 });
    const full = await error(await again.cloud.put(token, 'tiny.txt', 'x'), 507);
    assert.equal(full.type, 'storage_full');
    assert.equal(full.used, 909);
    assert.equal(full.quota, 500);
    assert.equal((await again.cloud.del(token, 'big.bin')).status, 200);
    assert.equal((await again.cloud.put(token, 'tiny.txt', 'x')).status, 201);
    assert.deepEqual((await readdir(join(userDir, 'blobs'))).sort(), [sha('different'), sha('x')].sort());
  } finally {
    await again.close();
  }
});

test('production keeps its own folder, apart from the demo', async (ctx) => {
  const dir = await mkdtemp(join(tmpdir(), 'lsuite-cloud-test-'));
  ctx.after(() => rm(dir, { recursive: true, force: true }));
  await createCloud({ dataDir: dir, production: true }).ready();
  await createCloud({ dataDir: dir }).ready();
  assert.deepEqual((await readdir(dir)).sort(), ['cloud', 'production-cloud']);
});

test('me.cloud, the plans\' storage and the plan cards', async () => {
  const t = await start();
  try {
    const token = await t.connect({ plan: 'studio' });
    await t.cloud.put(token, 'x.txt', 'hello');
    const me = await (await t.call('GET', '/api/account/me', token)).json();
    assert.deepEqual(me.cloud, { used: 5, quota: 100e6, files: 1 });
    const site = await (await t.site('/api/account/me')).json();
    assert.deepEqual(site.cloud, me.cloud);

    const plans = await (await fetch(`${t.base}/api/ai/plans`)).json();
    assert.deepEqual(plans.plans.map((p) => [p.id, p.storage, p.storageLabel]), [['free', 0, 'None'], ['plus', 50e9, '50 GB'], ['pro', 250e9, '250 GB'], ['studio', 1e12, '1 TB']]);
    assert.deepEqual(plans.cloudDemo, { quota: 100e6, maxFile: 25e6 });
    assert.equal(storageLabel(12.34e6), '12.3 MB');

    const html = plansHtml();
    for (const label of ['50 GB', '250 GB', '1 TB']) assert.ok(html.includes(`<b>${label}</b> lsuite Cloud`), label);
    assert.equal((html.match(/lsuite Cloud/g) ?? []).length, 3, 'not on Free');
  } finally {
    await t.close();
  }
});

test('GET /api/apps: the five apps for the launcher', async () => {
  const server = createServer((req, res) => handle(req, res));
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  try {
    const base = `http://127.0.0.1:${server.address().port}`;
    const res = await fetch(`${base}/api/apps`);
    assert.equal(res.status, 200);
    assert.equal(res.headers.get('cache-control'), 'public, max-age=300');
    assert.match(res.headers.get('content-type'), /^application\/json/);
    const { apps } = await res.json();
    assert.deepEqual(apps.map((a) => a.id), APP_NAMES);
    assert.deepEqual(apps.map((a) => a.kind), ['music', 'video', 'code', 'image', 'office']);
    for (const app of apps) {
      assert.deepEqual(Object.keys(app).sort(), ['id', 'kind', 'name', 'page', 'platforms', 'published', 'repo', 'summary', 'version']);
      assert.equal(app.name, app.id);
      assert.equal(app.page, `${base}/${app.id}`);
      assert.equal(app.repo, `ludovic111/${app.id}`);
      assert.match(app.version, /^\d+\.\d+\.\d+/);
      assert.equal(app.published, true);
      assert.ok(app.platforms.includes('macos-arm64'));
      assert.ok(app.summary && !app.summary.includes('\n'));
    }
    assert.equal((await fetch(`${base}/api/apps`, { method: 'POST' })).status, 405);
  } finally {
    server.close();
  }
});
