// lsuite Cloud's object store (CLOUD.md, "Storage on the server"): a fake S3 server that checks
// every request's Signature V4 from scratch, and the cloud API on top of it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer, request } from 'node:http';
import { createHash, createHmac } from 'node:crypto';
import { mkdtemp, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createAccounts } from '../ai.js';
import { createCloud, objectStore, objectStoreConfig, MAX_FILE } from '../cloud.js';

const sha = (b) => createHash('sha256').update(b).digest('hex');
const hmac = (key, data) => createHmac('sha256', key).update(data).digest();
const encode = (s) => encodeURIComponent(s).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
const KEYS = { accessKeyId: 'AKIDLSUITETEST', secretAccessKey: 'test/secret+key', region: 'us-east-1', bucket: 'lsuite-cloud' };

/**
 * An S3-compatible server in memory (path-style). It recomputes each request's signature from
 * what it received and refuses any that doesn't match, like S3 does.
 */
async function fakeS3() {
  const objects = new Map();
  const log = [];
  const s3 = { objects, log, failing: false };
  const reply = (res, status, code) => {
    const xml = code ? `<?xml version="1.0" encoding="UTF-8"?><Error><Code>${code}</Code><Message>${code}</Message></Error>` : '';
    res.writeHead(status, { 'content-type': 'application/xml', 'content-length': Buffer.byteLength(xml) });
    res.end(xml);
  };
  const server = createServer(async (req, res) => {
    const chunks = [];
    for await (const c of req) chunks.push(c);
    const body = Buffer.concat(chunks);
    const [path, qs = ''] = req.url.split('?');
    const query = Object.fromEntries(qs ? qs.split('&').map((p) => p.split('=').map(decodeURIComponent)) : []);
    log.push({ method: req.method, path, query, headers: req.headers, size: body.length });

    const auth = /^AWS4-HMAC-SHA256 Credential=([^/]+)\/(\d{8})\/([^/]+)\/s3\/aws4_request, SignedHeaders=([a-z0-9;-]+), Signature=([0-9a-f]{64})$/.exec(req.headers.authorization ?? '');
    if (!auth || auth[1] !== KEYS.accessKeyId) return reply(res, 403, 'InvalidAccessKeyId');
    const [, , day, region, signedHeaders, signature] = auth;
    const signed = signedHeaders.split(';');
    if (region !== KEYS.region || !['host', 'x-amz-content-sha256', 'x-amz-date'].every((h) => signed.includes(h))) return reply(res, 403, 'AuthorizationHeaderMalformed');
    const amzDate = req.headers['x-amz-date'];
    if (!amzDate?.startsWith(day) || Math.abs(Date.parse(amzDate.replace(/^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z$/, '$1-$2-$3T$4:$5:$6Z')) - Date.now()) > 15 * 60e3) return reply(res, 403, 'RequestTimeTooSkewed');
    const canonicalQuery = Object.keys(query).map((k) => [encode(k), encode(query[k])]).sort(([a], [b]) => (a < b ? -1 : 1)).map(([k, v]) => `${k}=${v}`).join('&');
    const canonical = [req.method, path, canonicalQuery, signed.map((h) => `${h}:${String(req.headers[h]).trim()}\n`).join(''), signedHeaders, req.headers['x-amz-content-sha256']].join('\n');
    const toSign = ['AWS4-HMAC-SHA256', amzDate, `${day}/${region}/s3/aws4_request`, sha(canonical)].join('\n');
    const key = hmac(hmac(hmac(hmac(`AWS4${KEYS.secretAccessKey}`, day), region), 's3'), 'aws4_request');
    if (hmac(key, toSign).toString('hex') !== signature) return reply(res, 403, 'SignatureDoesNotMatch');
    if (req.headers['x-amz-content-sha256'] !== sha(body)) return reply(res, 400, 'XAmzContentSHA256Mismatch');
    if (s3.failing) return reply(res, 503, 'SlowDown');

    if (!path.startsWith(`/${KEYS.bucket}`)) return reply(res, 404, 'NoSuchBucket');
    const name = path.slice(KEYS.bucket.length + 2).split('/').map(decodeURIComponent).join('/');
    if (req.method === 'GET' && !name && query['list-type'] === '2') {
      // Two keys a page, to walk the continuation tokens.
      const all = [...objects.keys()].filter((k) => k.startsWith(query.prefix ?? '')).sort();
      const from = query['continuation-token'] ? Number(query['continuation-token']) : 0;
      const page = all.slice(from, from + 2);
      const more = from + 2 < all.length;
      const xml = `<?xml version="1.0" encoding="UTF-8"?><ListBucketResult><IsTruncated>${more}</IsTruncated>${page.map((k) => `<Contents><Key>${k}</Key></Contents>`).join('')}${more ? `<NextContinuationToken>${from + 2}</NextContinuationToken>` : ''}</ListBucketResult>`;
      res.writeHead(200, { 'content-type': 'application/xml', 'content-length': Buffer.byteLength(xml) });
      return res.end(xml);
    }
    if (req.method === 'PUT') {
      if (Number(req.headers['content-length']) !== body.length) return reply(res, 400, 'IncompleteBody');
      objects.set(name, body);
      res.writeHead(200, { etag: `"${createHash('md5').update(body).digest('hex')}"`, 'content-length': 0 });
      return res.end();
    }
    if (req.method === 'GET') {
      if (!objects.has(name)) return reply(res, 404, 'NoSuchKey');
      res.writeHead(200, { 'content-type': 'application/octet-stream', 'content-length': objects.get(name).length });
      return res.end(objects.get(name));
    }
    if (req.method === 'DELETE') {
      objects.delete(name);
      res.writeHead(204);
      return res.end();
    }
    reply(res, 405, 'MethodNotAllowed');
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  s3.config = { endpoint: `http://127.0.0.1:${server.address().port}`, bucket: KEYS.bucket, region: KEYS.region, accessKeyId: KEYS.accessKeyId, secretAccessKey: KEYS.secretAccessKey, prefix: 'test', pathStyle: true };
  let closed = null;
  s3.close = () => (closed ??= new Promise((r) => server.close(r)));
  s3.requests = (method) => log.filter((l) => l.method === method);
  return s3;
}

/** The accounts service with lsuite Cloud on `store`, and a signed-in app token per call to `connect`. */
async function start(options) {
  const accounts = createAccounts({ demoDelayMs: 0, ...options });
  const server = createServer(async (req, res) => {
    if (!(await accounts.handle(req, res, new URL(req.url, 'http://localhost')))) {
      res.writeHead(404);
      res.end();
    }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  const connect = async ({ plan = 'plus', email = 'ada@example.com' } = {}) => {
    let jar = '';
    const site = async (path, body) => {
      const res = await fetch(base + path, { method: 'POST', headers: { 'content-type': 'application/json', ...(jar ? { cookie: jar } : {}) }, body: JSON.stringify(body) });
      if (res.headers.get('set-cookie')) jar = res.headers.get('set-cookie').split(';')[0];
      return res;
    };
    await site('/api/account/session', { email, name: 'Ada' });
    if (plan) await site('/api/account/checkout', { plan });
    const { code } = await (await site('/api/account/connect', { app: 'lsuite' })).json();
    return (await (await site('/api/account/token', { code })).json()).token;
  };
  const call = (method, path, token, body, headers = {}) => fetch(`${base}/api/cloud${path}`, { method, headers: { authorization: `Bearer ${token}`, ...headers }, body });
  const userId = () => Object.values(accounts.store.data.users)[0].id;
  return { accounts, connect, call, userId, close: () => new Promise((r) => server.close(r)) };
}

async function error(res, status) {
  assert.equal(res.status, status);
  const body = await res.json();
  assert.equal(body.type, 'error');
  assert.ok(!body.error.message.includes('\n'), 'one line');
  return body.error;
}

async function scratch(ctx) {
  const dir = await mkdtemp(join(tmpdir(), 'lsuite-cloud-store-'));
  ctx.after(() => rm(dir, { recursive: true, force: true }));
  return dir;
}

test('object store: signed, streamed uploads and downloads, content stored once, blobs gone with their last name', async (ctx) => {
  const s3 = await fakeS3();
  ctx.after(() => s3.close());
  const dir = await scratch(ctx);
  const t = await start({ dataDir: dir, cloudStore: s3.config });
  try {
    assert.equal(t.accounts.cloud.backend, 'object-store');
    const token = await t.connect();
    const key = (data) => `test/cloud/${t.userId()}/blobs/${sha(data)}`;
    // Demo mode keeps the demo caps, whatever the backend.
    const status = await (await t.call('GET', '', token)).json();
    assert.equal(status.quota, 100e6);
    assert.equal(status.maxFile, 25e6);

    const data = Buffer.from('a mix, uploaded to the object store');
    const put = await t.call('PUT', '/files/Mixes/one.flac', token, data);
    assert.equal(put.status, 201);
    assert.equal((await put.json()).file.sha256, sha(data));
    assert.deepEqual([...s3.objects.keys()], [key(data)]);
    const [sent] = s3.requests('PUT');
    assert.equal(sent.path, `/lsuite-cloud/${key(data)}`);
    assert.equal(sent.headers['content-length'], String(data.length));
    assert.equal(sent.headers['x-amz-content-sha256'], sha(data), 'the hash already computed, as the payload hash');
    assert.equal(sent.headers['transfer-encoding'], undefined, 'not chunked');
    // The index stays on the data dir, no blob there.
    const userDir = join(dir, 'cloud', t.userId());
    assert.deepEqual(await readdir(userDir), ['index.json']);
    assert.deepEqual(await readdir(join(dir, 'cloud', '.tmp')), [], 'no upload left behind');

    const get = await t.call('GET', '/files/Mixes/one.flac', token);
    assert.equal(get.status, 200);
    assert.equal(get.headers.get('content-length'), String(data.length));
    assert.equal(get.headers.get('etag'), `"${sha(data)}"`);
    assert.deepEqual(Buffer.from(await get.arrayBuffer()), data);
    const gets = s3.requests('GET').length;
    assert.equal((await t.call('HEAD', '/files/Mixes/one.flac', token)).status, 200);
    assert.equal((await t.call('GET', '/files/Mixes/one.flac', token, undefined, { 'if-none-match': `"${sha(data)}"` })).status, 304);
    assert.equal(s3.requests('GET').length, gets, 'HEAD and 304 never reach the store');

    // The same content under a second name: no second upload, counted twice.
    const copy = await (await t.call('PUT', '/files/Backup/one.flac', token, data)).json();
    assert.equal(copy.used, data.length * 2);
    assert.equal(s3.requests('PUT').length, 1, 'deduplicated');

    // Replaced by other bytes: the old content stays while another name has it.
    const other = Buffer.from('another take');
    assert.equal((await t.call('PUT', '/files/Mixes/one.flac', token, other)).status, 200);
    assert.deepEqual([...s3.objects.keys()].sort(), [key(data), key(other)].sort());
    // A move keeps the blob; deleting the last name drops it.
    assert.equal((await t.call('POST', '/move', token, JSON.stringify({ from: 'Backup', to: 'Archive' }), { 'content-type': 'application/json' })).status, 200);
    assert.ok(s3.objects.has(key(data)));
    assert.equal((await t.call('DELETE', '/files/Archive', token)).status, 200);
    assert.deepEqual([...s3.objects.keys()], [key(other)]);
    assert.equal((await t.call('DELETE', '/files/Mixes/one.flac', token)).status, 200);
    assert.deepEqual([...s3.objects.keys()], []);
    assert.equal((await (await t.call('GET', '', token)).json()).used, 0);
  } finally {
    await t.close();
    await s3.close();
  }
});

test('object store: failures are a one-line 502, nothing half-saved, leftovers swept at the next load', async (ctx) => {
  const s3 = await fakeS3();
  ctx.after(() => s3.close());
  const dir = await scratch(ctx);
  const t = await start({ dataDir: dir, cloudStore: s3.config });
  let token;
  try {
    token = await t.connect();
    const kept = Buffer.from('kept');
    assert.equal((await t.call('PUT', '/files/kept.txt', token, kept)).status, 201);

    s3.failing = true;
    const failed = await error(await t.call('PUT', '/files/new.txt', token, 'new'), 502);
    assert.equal(failed.type, 'api_error');
    assert.match(failed.message, /storage can't be reached right now \(the object store answered 503 SlowDown\)/);
    const down = await error(await t.call('GET', '/files/kept.txt', token), 502);
    assert.equal(down.type, 'api_error');
    s3.failing = false;
    const list = await (await t.call('GET', '/files', token)).json();
    assert.deepEqual(list.files.map((f) => f.path), ['kept.txt'], 'the failed upload saved nothing');
    assert.deepEqual(await readdir(join(dir, 'cloud', '.tmp')), []);

    // A delete whose blob the store can't remove still deletes: the blob is only left over.
    s3.failing = true;
    assert.equal((await t.call('DELETE', '/files/kept.txt', token)).status, 200);
    s3.failing = false;
    assert.equal(s3.objects.size, 1);
  } finally {
    await t.close();
  }

  // A new process: the leftover (and anything else no path names) goes before the first change.
  const stray = `test/cloud/${t.userId()}/blobs/${sha('stray')}`;
  s3.objects.set(stray, Buffer.from('stray'));
  s3.objects.set(`test/cloud/${t.userId()}/blobs/${sha('stray 2')}`, Buffer.from('stray 2'));
  s3.objects.set('test/cloud/someone-else/blobs/abc', Buffer.from('not this user'));
  const again = await start({ dataDir: dir, cloudStore: s3.config });
  try {
    assert.equal((await again.call('PUT', '/files/fresh.txt', token, 'fresh')).status, 201);
    assert.deepEqual([...s3.objects.keys()].sort(), [`test/cloud/${t.userId()}/blobs/${sha('fresh')}`, 'test/cloud/someone-else/blobs/abc'].sort());
    assert.ok(s3.requests('GET').some((r) => r.query['continuation-token']), 'listed page by page');
  } finally {
    await again.close();
  }

  // Wrong keys or no store at all: the same clean 502.
  const wrong = await start({ dataDir: dir, cloudStore: { ...s3.config, secretAccessKey: 'not the secret' } });
  try {
    assert.match((await error(await wrong.call('PUT', '/files/x.txt', token, 'x'), 502)).message, /403 SignatureDoesNotMatch/);
  } finally {
    await wrong.close();
  }
  await s3.close();
  const gone = await start({ dataDir: dir, cloudStore: s3.config });
  try {
    assert.equal((await error(await gone.call('PUT', '/files/x.txt', token, 'x'), 502)).type, 'api_error');
  } finally {
    await gone.close();
  }
});

test('production with an object store applies the plans\' own sizes; without one, the caps', async (ctx) => {
  const s3 = await fakeS3();
  ctx.after(() => s3.close());
  const dir = await scratch(ctx);
  const caps = { quota: 10, maxFile: 10, total: 10 };
  const cloud = createCloud({ dataDir: dir, production: true, caps, store: s3.config });
  const plus = { id: 'plus', storage: 50e9 };
  assert.deepEqual(cloud.limits(plus), { quota: 50e9, maxFile: MAX_FILE });
  assert.deepEqual(cloud.limits({ id: 'pro', storage: 250e9 }), { quota: 250e9, maxFile: MAX_FILE });
  assert.deepEqual(cloud.limits({ id: 'studio', storage: 1e12 }), { quota: 1e12, maxFile: MAX_FILE });
  assert.deepEqual(cloud.limits({ id: 'free', storage: 0 }), { quota: 0, maxFile: 0 });
  assert.deepEqual(createCloud({ dataDir: dir, production: true, caps }).limits(plus), { quota: 10, maxFile: 10 }, 'production on a volume');
  assert.deepEqual(createCloud({ dataDir: dir, caps, store: s3.config }).limits(plus), { quota: 10, maxFile: 10 }, 'demo with a store');
  assert.throws(() => createCloud({ store: s3.config }), /LSUITE_DATA_DIR/);

  // Straight to the cloud: far past the demo caps (and their total), up to the largest file.
  const server = createServer(async (req, res) => {
    try {
      const result = await cloud.upload('user-1', plus, 'big.bin', req);
      res.writeHead(201, { 'content-type': 'application/json' });
      res.end(JSON.stringify(result));
    } catch (err) {
      res.writeHead(err.status ?? 500, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ type: err.type, message: err.message }));
    }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  try {
    const base = `http://127.0.0.1:${server.address().port}`;
    const data = Buffer.alloc(5000, 7);
    const ok = await fetch(base, { method: 'PUT', body: data });
    assert.equal(ok.status, 201);
    assert.equal((await ok.json()).quota, 50e9);
    assert.ok(s3.objects.has(`test/production-cloud/user-1/blobs/${sha(data)}`), 'production blobs apart from the demo');
    // Larger than the largest file: refused from the headers alone.
    const refused = await new Promise((resolve, reject) => {
      const req = request(base, { method: 'PUT', headers: { 'content-length': String(MAX_FILE + 1) } }, (res) => {
        let text = '';
        res.on('data', (c) => (text += c));
        res.on('end', () => resolve({ status: res.statusCode, body: JSON.parse(text) }));
      });
      req.on('error', reject);
      req.write('x');
    });
    assert.equal(refused.status, 413);
    assert.equal(refused.body.type, 'request_too_large');
  } finally {
    server.closeAllConnections();
    await new Promise((r) => server.close(r));
    await s3.close();
  }
});

test('object store settings: all or nothing, regions and URL styles', () => {
  assert.equal(objectStoreConfig({}), null);
  assert.throws(() => objectStoreConfig({ LSUITE_CLOUD_S3_BUCKET: 'b' }), /LSUITE_CLOUD_S3_ENDPOINT, LSUITE_CLOUD_S3_ACCESS_KEY_ID, LSUITE_CLOUD_S3_SECRET_ACCESS_KEY/);
  const env = { LSUITE_CLOUD_S3_ENDPOINT: 'https://t3.storageapi.dev', LSUITE_CLOUD_S3_BUCKET: 'b', LSUITE_CLOUD_S3_ACCESS_KEY_ID: 'id', LSUITE_CLOUD_S3_SECRET_ACCESS_KEY: 'secret' };
  assert.deepEqual(objectStoreConfig(env), { endpoint: 'https://t3.storageapi.dev', bucket: 'b', accessKeyId: 'id', secretAccessKey: 'secret', region: '', prefix: '', pathStyle: false });
  assert.equal(objectStoreConfig({ ...env, LSUITE_CLOUD_S3_PATH_STYLE: 'true', LSUITE_CLOUD_S3_PREFIX: '/lsuite/' }).pathStyle, true);
  assert.equal(objectStore({ ...objectStoreConfig({ ...env, LSUITE_CLOUD_S3_PREFIX: '/lsuite/' }) }).prefix, 'lsuite');
  assert.throws(() => objectStoreConfig({ ...env, LSUITE_CLOUD_S3_ENDPOINT: 't3.storageapi.dev' }), /http\(s\) URL/);
  const region = (endpoint, explicit = '') => objectStore({ ...objectStoreConfig({ ...env, LSUITE_CLOUD_S3_ENDPOINT: endpoint, LSUITE_CLOUD_S3_REGION: explicit }) }).region;
  assert.equal(region('https://t3.storageapi.dev'), 'auto');
  assert.equal(region('https://0123abc.r2.cloudflarestorage.com'), 'auto');
  assert.equal(region('https://s3.eu-west-3.amazonaws.com'), 'eu-west-3');
  assert.equal(region('https://s3.amazonaws.com'), 'us-east-1');
  assert.equal(region('https://s3.amazonaws.com', 'ap-south-1'), 'ap-south-1');
});
