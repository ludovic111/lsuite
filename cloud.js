// lsuite Cloud (CLOUD.md): each account's files, dependency-free. `ai.js` authenticates and routes
// `/api/cloud…` here.
//
// - Storage: `<root>/<userId>/index.json` (`{format: 1, files: {path: {sha256, size, modifiedAt}},
//   folders: {path: {createdAt}}}`, 0600, written atomically) and `blobs/<sha256>`, one per distinct
//   content; a blob is removed once no path of the index names it. `<root>` is
//   `<dataDir>/cloud` (`production-cloud` in production), else a temporary folder removed at exit.
// - Uploads stream to `<root>/.tmp/` while being hashed and counted; the size is known up front
//   (Content-Length is required), so a file that is too large or doesn't fit is refused before
//   a byte is stored, and the stream stops if the body runs past its Content-Length.
// - One user's changes run one at a time (a promise chain per user); each builds a new index and
//   swaps it in once written, so reads always see a whole one.
// - Blobs can live in an S3-compatible object store instead (`objectStoreConfig()`, AWS Signature
//   V4 by hand): the key is `[<prefix>/]<cloud|production-cloud>/<userId>/blobs/<sha256>`, the
//   index stays in the data dir. With a store, production applies the plans' own quotas.
import { createHash, createHmac, randomBytes } from 'node:crypto';
import { createReadStream, createWriteStream, rmSync } from 'node:fs';
import { mkdir, mkdtemp, open, readdir, readFile, rename, rm, stat, statfs, writeFile } from 'node:fs/promises';
import { request as httpRequest } from 'node:http';
import { request as httpsRequest } from 'node:https';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

/** Storage is counted in decimal units: 1 GB is 1e9 bytes. */
export const GB = 1e9;
/** The largest file any plan takes. */
export const MAX_FILE = 5 * GB;
/** Files and folders one account can hold, whatever their size (each change rewrites the index). */
export const MAX_ENTRIES = 100000;

/** A byte count from the environment: a positive integer, else `fallback`. */
export const bytesFrom = (v, fallback) => (/^\d+$/.test(String(v ?? '').trim()) && Number(v) > 0 ? Number(v) : fallback);

/**
 * Demo caps (CLOUD.md), from `LSUITE_CLOUD_DEMO_QUOTA`, `…_MAX_FILE` and `…_TOTAL` (bytes): sized
 * for the site's 500 MB volume, which also holds the accounts.
 */
export function demoCaps(env = process.env) {
  return {
    quota: bytesFrom(env.LSUITE_CLOUD_DEMO_QUOTA, 100e6),
    maxFile: bytesFrom(env.LSUITE_CLOUD_DEMO_MAX_FILE, 25e6),
    total: bytesFrom(env.LSUITE_CLOUD_DEMO_TOTAL, 300e6),
  };
}

/** Free space always left on the data disk (`LSUITE_CLOUD_DISK_RESERVE`, bytes): the accounts live there too. */
export const diskReserve = (env = process.env) => bytesFrom(env.LSUITE_CLOUD_DISK_RESERVE, 100e6);

/** "50 GB", "1 TB", "12.5 MB"; "None" for 0. */
export function storageLabel(bytes) {
  if (!bytes) return 'None';
  const units = [[1e12, 'TB'], [1e9, 'GB'], [1e6, 'MB'], [1e3, 'KB']];
  const [size, unit] = units.find(([u]) => bytes >= u) ?? [1, 'bytes'];
  return `${Math.round((bytes / size) * 10) / 10} ${unit}`;
}

/** A refusal in CLOUD.md's terms: `ai.js` answers it with `fail()`. */
export class CloudError extends Error {
  constructor(status, type, message, extra = {}) {
    super(message);
    this.status = status;
    this.type = type;
    this.extra = extra;
  }
}

const invalid = (path, why) => new CloudError(400, 'invalid_path', `${why} (${JSON.stringify(String(path).slice(0, 200))}).`, { path: String(path) });
const notFound = (path) => new CloudError(404, 'not_found_error', `Nothing in lsuite Cloud at ${path}.`);
const conflict = (path, message) => new CloudError(409, 'conflict_error', message, { path });
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/**
 * A path checked against CLOUD.md's rules. `encoded`: the URL form, each segment percent-encoded
 * (decoded one segment at a time, so `%2F` can never make a new segment).
 */
export function cloudPath(raw, encoded = false) {
  if (typeof raw !== 'string') throw invalid(raw ?? '', 'A path is a string');
  const segments = raw.split('/').map((segment) => {
    if (!encoded) return segment;
    try {
      return decodeURIComponent(segment);
    } catch {
      throw invalid(raw, 'This path is not percent-encoded UTF-8');
    }
  });
  const path = segments.join('/');
  if (!path) throw invalid(raw, 'A path is needed');
  if (!path.isWellFormed() || Buffer.byteLength(path) > 1024) throw invalid(path, 'A path is 1 to 1024 bytes of UTF-8');
  for (const s of segments) {
    if (!s) throw invalid(path, 'A path has no empty segment and no leading or trailing /');
    if (s === '.' || s === '..') throw invalid(path, 'A path has no . or .. segment');
    if (s.includes('/') || s.includes('\\')) throw invalid(path, 'A name has no / or \\');
    if (CONTROL.test(s)) throw invalid(path, 'A name has no control characters');
    if (s !== s.trim()) throw invalid(path, 'A name has no leading or trailing spaces');
    if (Buffer.byteLength(s) > 255) throw invalid(path, 'A name is at most 255 bytes');
  }
  return path;
}

/** `Content-Disposition` for a download, the name in ASCII and in UTF-8. */
export function disposition(path) {
  const name = path.split('/').at(-1);
  const ascii = name.replace(/[^\x20-\x7e]|["\\]/g, '_');
  const utf8 = encodeURIComponent(name).replace(/['()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
  return `attachment; filename="${ascii}"; filename*=UTF-8''${utf8}`;
}

const SHA = /^[0-9a-f]{64}$/;
const USER_ID = /^[\w-]{1,64}$/;
const emptyIndex = () => ({ format: 1, files: {}, folders: {} });
const byPath = (a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
const usedOf = (index) => Object.values(index.files).reduce((sum, f) => sum + f.size, 0);
const clone = (index) => ({ format: 1, files: { ...index.files }, folders: { ...index.folders } });
const under = (path, folder) => path.startsWith(`${folder}/`);
const ancestors = (path) => path.split('/').slice(0, -1).map((_, i, parts) => parts.slice(0, i + 1).join('/'));

/** An index as read from disk, keeping only well-formed entries. */
function parseIndex(data) {
  const index = emptyIndex();
  if (data?.format !== 1) return index;
  for (const [path, f] of Object.entries(data.files ?? {})) {
    if (f && SHA.test(f.sha256) && Number.isSafeInteger(f.size) && f.size >= 0) index.files[path] = { sha256: f.sha256, size: f.size, modifiedAt: String(f.modifiedAt ?? new Date(0).toISOString()) };
  }
  for (const [path, f] of Object.entries(data.folders ?? {})) index.folders[path] = { createdAt: String(f?.createdAt ?? new Date(0).toISOString()) };
  return index;
}

/** `'file'`, `'folder'` (explicit, or holding something) or null. */
function kindOf(index, path) {
  if (index.files[path]) return 'file';
  if (index.folders[path]) return 'folder';
  for (const p in index.files) if (under(p, path)) return 'folder';
  for (const p in index.folders) if (under(p, path)) return 'folder';
  return null;
}

/** Refuses a path whose parent folders include a file. */
function checkParents(index, path) {
  const file = ancestors(path).find((a) => index.files[a]);
  if (file) throw conflict(file, `${file} is a file, so it can't hold ${path}.`);
}

/** Every folder: the explicit ones and every parent of a file or folder. */
function allFolders(index) {
  const set = new Set(Object.keys(index.folders));
  for (const p of [...Object.keys(index.files), ...Object.keys(index.folders)]) for (const a of ancestors(p)) set.add(a);
  return set;
}

const fileEntry = (path, f) => ({ path, size: f.size, sha256: f.sha256, modifiedAt: f.modifiedAt });

/**
 * Writes the body to `file` while hashing it; refuses a body that isn't exactly `length` bytes.
 * Also used by the marketplace (`marketplace.js`). → `{size, sha256}`.
 */
export function receive(req, file, length) {
  return new Promise((resolve, reject) => {
    const hash = createHash('sha256');
    const out = createWriteStream(file, { flags: 'wx', mode: 0o600 });
    let size = 0;
    let done = false;
    const stop = (err) => {
      if (done) return;
      done = true;
      req.removeListener('data', onData);
      out.destroy();
      reject(err);
    };
    const onData = (chunk) => {
      size += chunk.length;
      if (size > length) return stop(new CloudError(400, 'invalid_request_error', 'The body is longer than its Content-Length.'));
      hash.update(chunk);
      if (!out.write(chunk)) {
        req.pause();
        out.once('drain', () => req.resume());
      }
    };
    req.on('data', onData);
    req.on('end', () => (size === length ? out.end() : stop(new CloudError(400, 'invalid_request_error', 'The body is shorter than its Content-Length.'))));
    req.on('close', () => !req.complete && stop(new CloudError(400, 'invalid_request_error', 'The upload was interrupted.')));
    req.on('error', stop);
    out.on('error', stop);
    out.on('finish', () => {
      if (done) return;
      done = true;
      resolve({ size, sha256: hash.digest('hex') });
    });
  });
}

// ---- The object store: any S3-compatible service (AWS S3, Cloudflare R2, Railway buckets) ----

const EMPTY_SHA = createHash('sha256').digest('hex');
const STORE_VARS = { endpoint: 'ENDPOINT', bucket: 'BUCKET', accessKeyId: 'ACCESS_KEY_ID', secretAccessKey: 'SECRET_ACCESS_KEY' };

/**
 * The object store from `LSUITE_CLOUD_S3_*` (CLOUD.md), or null when none is set. Endpoint,
 * bucket and both keys go together: a partial setup throws rather than quietly use the disk.
 */
export function objectStoreConfig(env = process.env) {
  const v = (name) => String(env[`LSUITE_CLOUD_S3_${name}`] ?? '').trim();
  const config = Object.fromEntries(Object.entries(STORE_VARS).map(([k, name]) => [k, v(name)]));
  const missing = Object.entries(STORE_VARS).filter(([k]) => !config[k]).map(([, name]) => `LSUITE_CLOUD_S3_${name}`);
  if (missing.length === 4 && !v('REGION') && !v('PREFIX') && !v('PATH_STYLE')) return null;
  if (missing.length) throw new Error(`lsuite Cloud's object store also needs ${missing.join(', ')}.`);
  if (!/^https?:\/\/[^/]/.test(config.endpoint)) throw new Error('LSUITE_CLOUD_S3_ENDPOINT must be an http(s) URL.');
  return { ...config, region: v('REGION'), prefix: v('PREFIX'), pathStyle: /^(1|true|yes|on)$/i.test(v('PATH_STYLE')) };
}

/** RFC 3986 encoding, as Signature V4 wants it. */
const uriEncode = (s) => encodeURIComponent(s).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
const hmac = (key, data) => createHmac('sha256', key).update(data).digest();
/** A store that failed or didn't answer: a 502 the apps can show as it is. */
const unavailable = (why) => new CloudError(502, 'api_error', `lsuite Cloud's storage can't be reached right now (${why}). Try again in a moment.`);

/** A response's body as text (error pages, listings). */
async function textOf(res) {
  let text = '';
  for await (const chunk of res) if (text.length < 1e6) text += chunk;
  return text;
}

/**
 * A small S3 client, requests signed with AWS Signature V4 and streamed both ways. `config` is
 * `objectStoreConfig()`'s; the region defaults to the AWS endpoint's, else `auto` (R2, Railway).
 */
export function objectStore(config, { timeoutMs = 60000 } = {}) {
  const endpoint = new URL(config.endpoint);
  const region = config.region || (/\.amazonaws\.com$/.test(endpoint.hostname) ? (/[.-]([a-z]{2}(?:-[a-z]+)+-\d+)\.amazonaws\.com$/.exec(endpoint.hostname)?.[1] ?? 'us-east-1') : 'auto');
  const base = endpoint.pathname.replace(/\/+$/, '');
  const hostname = config.pathStyle ? endpoint.hostname : `${config.bucket}.${endpoint.hostname}`;
  const host = endpoint.port ? `${hostname}:${endpoint.port}` : hostname;
  const transport = endpoint.protocol === 'https:' ? httpsRequest : httpRequest;

  /** One signed request; resolves with the response (its body not read yet). */
  function send(method, key, { query = {}, body = null, length = 0, sha256 = EMPTY_SHA } = {}) {
    const keyPath = key ? `/${key.split('/').map(uriEncode).join('/')}` : '';
    const path = `${base}${config.pathStyle ? `/${uriEncode(config.bucket)}` : ''}${keyPath}` || '/';
    const qs = Object.keys(query).sort().map((k) => `${uriEncode(k)}=${uriEncode(query[k])}`).join('&');
    const amzDate = new Date().toISOString().replace(/[-:]|\.\d{3}/g, '');
    const day = amzDate.slice(0, 8);
    const headers = { host, 'x-amz-content-sha256': sha256, 'x-amz-date': amzDate };
    const names = Object.keys(headers).sort();
    const canonical = [method, path, qs, names.map((h) => `${h}:${headers[h]}\n`).join(''), names.join(';'), sha256].join('\n');
    const scope = `${day}/${region}/s3/aws4_request`;
    const toSign = ['AWS4-HMAC-SHA256', amzDate, scope, createHash('sha256').update(canonical).digest('hex')].join('\n');
    const signingKey = ['s3', 'aws4_request'].reduce(hmac, hmac(hmac(`AWS4${config.secretAccessKey}`, day), region));
    const authorization = `AWS4-HMAC-SHA256 Credential=${config.accessKeyId}/${scope}, SignedHeaders=${names.join(';')}, Signature=${hmac(signingKey, toSign).toString('hex')}`;
    return new Promise((resolve, reject) => {
      const req = transport({
        hostname: hostname.replace(/^\[|\]$/g, ''),
        port: endpoint.port || undefined,
        method,
        path: qs ? `${path}?${qs}` : path,
        headers: { ...headers, authorization, ...(body ? { 'content-length': length } : {}) },
      });
      req.setTimeout(timeoutMs, () => req.destroy(new Error('timed out')));
      req.on('response', resolve);
      req.on('error', (err) => reject(unavailable(err.message.split('\n')[0])));
      if (!body) return req.end();
      body.on('error', (err) => req.destroy(err));
      body.pipe(req);
    });
  }

  /** Refuses any answer but `ok`, reading the error body away. */
  async function expect(res, ...ok) {
    if (ok.includes(res.statusCode)) return res;
    const code = /<Code>([^<]{1,64})<\/Code>/.exec(await textOf(res).catch(() => ''))?.[1];
    throw unavailable(`the object store answered ${res.statusCode}${code ? ` ${code}` : ''}`);
  }

  return {
    prefix: String(config.prefix ?? '').replace(/^\/+|\/+$/g, ''),
    region,

    /** Uploads `file` (`size` bytes, its SHA-256 in hex) as `key`, streamed from disk. */
    async put(key, file, size, sha256) {
      (await expect(await send('PUT', key, { body: createReadStream(file), length: size, sha256 }), 200)).resume();
    },

    /** A stream of `key`'s bytes, or null if it isn't there. */
    async get(key) {
      const res = await send('GET', key);
      if (res.statusCode === 404) {
        res.resume();
        return null;
      }
      return expect(res, 200);
    },

    async remove(key) {
      (await expect(await send('DELETE', key), 200, 204, 404)).resume();
    },

    /** Every key under `prefix` (ListObjectsV2, a page at a time). */
    async list(prefix) {
      const keys = [];
      let token = null;
      do {
        const res = await expect(await send('GET', '', { query: { 'list-type': '2', prefix, ...(token ? { 'continuation-token': token } : {}) } }), 200);
        const xml = await textOf(res);
        for (const [, key] of xml.matchAll(/<Key>([^<]*)<\/Key>/g)) keys.push(key.replace(/&amp;/g, '&'));
        token = /<IsTruncated>true<\/IsTruncated>/.test(xml) ? /<NextContinuationToken>([^<]+)<\/NextContinuationToken>/.exec(xml)?.[1] ?? null : null;
      } while (token);
      return keys;
    },
  };
}

// Temporary roots (no data dir) are removed when the process ends, signals included.
const temporary = new Set();
function removeTemporary() {
  for (const dir of temporary) rmSync(dir, { recursive: true, force: true });
  temporary.clear();
}
let hooked = false;
function hookExit() {
  if (hooked) return;
  hooked = true;
  process.once('exit', removeTemporary);
  for (const signal of ['SIGINT', 'SIGTERM']) {
    process.once(signal, () => {
      removeTemporary();
      process.kill(process.pid, signal);
    });
  }
}

/** A temporary folder removed when the process ends (exit, SIGINT, SIGTERM): the root without a data dir. */
export async function temporaryDir(prefix) {
  const dir = await mkdtemp(join(tmpdir(), prefix));
  temporary.add(dir);
  hookExit();
  return dir;
}

/** Free bytes on the disk holding `dir`, or null when the system can't tell. */
export async function freeSpace(dir, statfsImpl = statfs) {
  try {
    const fs = await statfsImpl(dir);
    return Number(fs.bavail) * Number(fs.bsize);
  } catch {
    return null;
  }
}

/**
 * The cloud store. Options: `dataDir` (else a temporary folder), `production`, `caps`
 * (`{quota, maxFile, total}`, `demoCaps()` by default), `diskReserve` (bytes, `diskReserve()` by
 * default) and `statfs` (tests), `store` (`objectStoreConfig()`: blobs go
 * to the object store; needs `dataDir`, where the index stays). The caps apply in demo mode, and
 * in production without a store; production with a store applies the plans' own sizes.
 * Plans are `{id, storage}` (bytes; 0 on Free).
 */
export function createCloud(options = {}) {
  const caps = { ...demoCaps(), ...(options.caps ?? {}) };
  if (options.store && !options.dataDir) throw new Error('lsuite Cloud\'s object store needs LSUITE_DATA_DIR: the index of every account lives there.');
  const store = options.store ? objectStore(options.store, options.storeOptions) : null;
  const planSized = Boolean(options.production && store);
  const area = options.production ? 'production-cloud' : 'cloud';
  /** A blob's key in the object store, laid out like the data dir. */
  const blobKey = (userId, sha) => [store.prefix, area, userId, 'blobs', sha].filter(Boolean).join('/');
  const users = new Map();
  const totals = new Map();
  let reservedTotal = 0;
  let base = null;

  /** The root folder, made (and every account's total counted) on first use. */
  function root() {
    base ??= (async () => {
      const dir = options.dataDir ? join(options.dataDir, area) : await temporaryDir('lsuite-cloud-');
      await mkdir(dir, { recursive: true, mode: 0o700 });
      await rm(join(dir, '.tmp'), { recursive: true, force: true });
      await mkdir(join(dir, '.tmp'), { mode: 0o700 });
      for (const entry of await readdir(dir, { withFileTypes: true })) {
        if (!entry.isDirectory() || !USER_ID.test(entry.name)) continue;
        try {
          totals.set(entry.name, usedOf(parseIndex(JSON.parse(await readFile(join(dir, entry.name, 'index.json'), 'utf8')))));
        } catch {}
      }
      return dir;
    })();
    return base;
  }

  const totalUsed = () => [...totals.values()].reduce((a, b) => a + b, 0);

  /** A user's state: `{dir, index, used, chain, reserved}`, loaded once. */
  function userState(userId) {
    if (!USER_ID.test(userId)) return Promise.reject(new Error('Invalid user id.'));
    let state = users.get(userId);
    if (!state) {
      state = (async () => {
        const dir = join(await root(), userId);
        let index = emptyIndex();
        try {
          index = parseIndex(JSON.parse(await readFile(join(dir, 'index.json'), 'utf8')));
        } catch (err) {
          if (err.code !== 'ENOENT') throw err;
        }
        const loaded = { dir, index, used: usedOf(index), chain: Promise.resolve(), reserved: 0 };
        // Blobs no path names (a write cut short) are dropped: in the store, before the user's
        // first change and only if the store answers.
        if (store) {
          locked(loaded, async () => {
            const named = new Set(Object.values(loaded.index.files).map((f) => f.sha256));
            const keys = await store.list(`${blobKey(userId, '')}/`);
            await Promise.all(keys.filter((k) => !named.has(k.split('/').at(-1))).map((k) => store.remove(k)));
          }).catch(() => {});
          return loaded;
        }
        const named = new Set(Object.values(index.files).map((f) => f.sha256));
        const blobs = await readdir(join(dir, 'blobs')).catch(() => []);
        await Promise.all(blobs.filter((b) => !named.has(b)).map((b) => rm(join(dir, 'blobs', b), { force: true })));
        return loaded;
      })();
      users.set(userId, state);
      state.catch(() => users.delete(userId));
    }
    return state;
  }

  /** Runs `work` after the user's other changes. */
  function locked(state, work) {
    const run = state.chain.catch(() => {}).then(work);
    state.chain = run.catch(() => {});
    return run;
  }

  /** Writes `next` (0600, atomically), swaps it in and drops the blobs it no longer names. */
  async function commit(state, userId, next, dropped = []) {
    if (Object.keys(next.files).length + Object.keys(next.folders).length > MAX_ENTRIES) {
      throw new CloudError(507, 'storage_full', `lsuite Cloud holds at most ${MAX_ENTRIES.toLocaleString('en-US')} files and folders per account.`, { used: state.used });
    }
    await mkdir(state.dir, { recursive: true, mode: 0o700 });
    const file = join(state.dir, 'index.json');
    const tmp = `${file}.${process.pid}.${randomBytes(4).toString('hex')}.tmp`;
    await writeFile(tmp, JSON.stringify(next), { mode: 0o600 });
    await rename(tmp, file);
    state.index = next;
    state.used = usedOf(next);
    totals.set(userId, state.used);
    const named = new Set(Object.values(next.files).map((f) => f.sha256));
    const gone = [...new Set(dropped)].filter((sha) => !named.has(sha));
    // The index is written, so a blob the store fails to delete is only left over: swept at next load.
    if (store) await Promise.all(gone.map((sha) => store.remove(blobKey(userId, sha)).catch(() => {})));
    else await Promise.all(gone.map((sha) => rm(join(state.dir, 'blobs', sha), { force: true })));
  }

  /** `{quota, maxFile}` for a plan: its own, within the caps. */
  function limits(plan) {
    if (!plan?.storage) return { quota: 0, maxFile: 0 };
    if (planSized) return { quota: plan.storage, maxFile: MAX_FILE };
    return { quota: Math.min(plan.storage, caps.quota), maxFile: Math.min(MAX_FILE, caps.maxFile) };
  }

  /**
   * Refuses `size` more bytes (`freed` given back) beyond the quota or the caps' total; `pending`
   * also counts the uploads still streaming.
   */
  function checkRoom(state, plan, size, freed, pending = false) {
    const { quota } = limits(plan);
    if (state.used + (pending ? state.reserved : 0) - freed + size > quota) {
      throw new CloudError(507, 'storage_full', `This file doesn't fit in your lsuite Cloud storage: ${storageLabel(state.used)} of ${storageLabel(quota)} used.`, { used: state.used, quota });
    }
    if (!planSized && totalUsed() + (pending ? reservedTotal : 0) - freed + size > caps.total) {
      throw new CloudError(507, 'storage_full', `The lsuite Cloud demo is full (${storageLabel(caps.total)} for every account together). Delete files, or try again later.`, { used: state.used, quota });
    }
  }

  /**
   * Refuses an upload that would leave less than the reserve free on the disk the uploads stream
   * to (whatever the caps say: a disk can be smaller than they are, and the accounts share it).
   */
  async function checkDisk(size) {
    const reserve = options.diskReserve ?? diskReserve();
    const free = await freeSpace(await root(), options.statfs);
    if (free === null) return;
    if (free - reservedTotal - size < reserve) {
      throw new CloudError(507, 'storage_full', 'lsuite Cloud has no room left on this server for now. Delete files, or try again later.', {});
    }
  }

  /** Refuses an upload to `path` that would break the tree or `If-None-Match`. */
  function checkUpload(index, path, ifNoneMatch) {
    checkParents(index, path);
    if (kindOf(index, path) === 'folder') throw conflict(path, `${path} is a folder.`);
    const current = index.files[path];
    if (current && ifNoneMatch && (ifNoneMatch.includes('*') || ifNoneMatch.includes(`"${current.sha256}"`))) {
      throw new CloudError(412, 'precondition_failed', `${path} already exists in lsuite Cloud.`);
    }
  }

  return {
    caps,
    limits,
    /** `'object-store'` or `'disk'`: where the blobs live. */
    backend: store ? 'object-store' : 'disk',
    ready: () => root(),

    /** `{used, quota, files}` (`GET /api/account/me`). */
    async summary(userId, plan) {
      const state = await userState(userId);
      return { used: state.used, quota: limits(plan).quota, files: Object.keys(state.index.files).length };
    },

    /** `GET /api/cloud`, without the account's own fields. */
    async status(userId, plan) {
      const state = await userState(userId);
      return { ...limits(plan), used: state.used, files: Object.keys(state.index.files).length, folders: allFolders(state.index).size };
    },

    /** `GET /api/cloud/files`. */
    async list(userId, plan) {
      const { index, used } = await userState(userId);
      return {
        files: Object.entries(index.files).map(([path, f]) => fileEntry(path, f)).sort(byPath),
        folders: Object.entries(index.folders).map(([path, f]) => ({ path, createdAt: f.createdAt })).sort(byPath),
        used,
        quota: limits(plan).quota,
      };
    },

    /** The file at `path` (`{path, size, sha256, modifiedAt}`). */
    async find(userId, path) {
      const { index } = await userState(userId);
      if (index.files[path]) return fileEntry(path, index.files[path]);
      if (kindOf(index, path) === 'folder') throw conflict(path, `${path} is a folder: download its files one by one.`);
      throw notFound(path);
    },

    /** A stream of a file's bytes, opened before it is returned. */
    async read(userId, file) {
      const state = await userState(userId);
      if (store) {
        const res = await store.get(blobKey(userId, file.sha256));
        if (!res) throw notFound(file.path);
        if (res.headers['content-length'] !== undefined && Number(res.headers['content-length']) !== file.size) {
          res.destroy();
          throw unavailable(`the object store holds ${res.headers['content-length']} bytes for a ${file.size}-byte file`);
        }
        return res;
      }
      try {
        const handle = await open(join(state.dir, 'blobs', file.sha256), 'r');
        return handle.createReadStream();
      } catch (err) {
        if (err.code === 'ENOENT') throw notFound(file.path);
        throw err;
      }
    },

    /**
     * `PUT /api/cloud/files/<path>`: streams `req` into the file. → `{created, file, used, quota}`.
     * Headers: `content-length` (required), `x-lsuite-sha256`, `x-lsuite-modified`, `if-none-match`.
     */
    async upload(userId, plan, path, req) {
      const { quota, maxFile } = limits(plan);
      const declared = req.headers['content-length'];
      if (declared === undefined || !/^\d+$/.test(declared)) throw new CloudError(411, 'length_required', 'Send the file’s size in Content-Length: lsuite Cloud doesn’t take chunked uploads.');
      const length = Number(declared);
      const expected = req.headers['x-lsuite-sha256'] === undefined ? null : String(req.headers['x-lsuite-sha256']).trim().toLowerCase();
      if (expected !== null && !SHA.test(expected)) throw new CloudError(400, 'invalid_request_error', 'X-Lsuite-Sha256 must be the SHA-256 of the body, 64 hex digits.');
      let modifiedAt = new Date().toISOString();
      if (req.headers['x-lsuite-modified'] !== undefined) {
        const value = String(req.headers['x-lsuite-modified']).trim();
        const t = Date.parse(value);
        if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:?\d{2})$/i.test(value) || !Number.isFinite(t)) {
          throw new CloudError(400, 'invalid_request_error', 'X-Lsuite-Modified must be an ISO 8601 date and time with its time zone.');
        }
        modifiedAt = new Date(t).toISOString();
      }
      const ifNoneMatch = req.headers['if-none-match'] === undefined ? null : String(req.headers['if-none-match']).split(',').map((s) => s.trim().replace(/^W\//, ''));
      if (length > maxFile) throw new CloudError(413, 'request_too_large', `This file is ${storageLabel(length)}; the largest file lsuite Cloud takes on your plan is ${storageLabel(maxFile)}.`, { max_file: maxFile });

      const state = await userState(userId);
      checkUpload(state.index, path, ifNoneMatch);
      checkRoom(state, plan, length, state.index.files[path]?.size ?? 0, true);
      await checkDisk(length);
      // Held until the body is in, so two uploads at once can't both take the last of the room.
      state.reserved += length;
      reservedTotal += length;
      const tmp = join(await root(), '.tmp', randomBytes(12).toString('hex'));
      let got;
      try {
        got = await receive(req, tmp, length);
      } catch (err) {
        await rm(tmp, { force: true });
        throw err;
      } finally {
        state.reserved -= length;
        reservedTotal -= length;
      }
      if (expected && expected !== got.sha256) {
        await rm(tmp, { force: true });
        throw new CloudError(400, 'checksum_mismatch', `The upload doesn't match X-Lsuite-Sha256 (received ${got.sha256}). Nothing was saved; send it again.`);
      }

      return locked(state, async () => {
        try {
          // Again, now that it's this upload's turn.
          checkUpload(state.index, path, ifNoneMatch);
          const old = state.index.files[path];
          checkRoom(state, plan, got.size, old?.size ?? 0);
          // Under the lock, so no change can drop the blob between this and the commit. In the
          // store, content one of the user's paths already names is there: no second upload.
          let stored = false;
          if (store) {
            if (!Object.values(state.index.files).some((f) => f.sha256 === got.sha256)) {
              await store.put(blobKey(userId, got.sha256), tmp, got.size, got.sha256);
              stored = true;
            }
          } else {
            await mkdir(join(state.dir, 'blobs'), { recursive: true, mode: 0o700 });
            const blob = join(state.dir, 'blobs', got.sha256);
            if (!(await stat(blob).catch(() => null))) await rename(tmp, blob);
          }
          const next = clone(state.index);
          next.files[path] = { sha256: got.sha256, size: got.size, modifiedAt };
          try {
            await commit(state, userId, next, old ? [old.sha256] : []);
          } catch (err) {
            if (stored && state.index !== next) await store.remove(blobKey(userId, got.sha256)).catch(() => {});
            throw err;
          }
          return { created: !old, file: fileEntry(path, next.files[path]), used: state.used, quota };
        } finally {
          await rm(tmp, { force: true });
        }
      });
    },

    /** `DELETE /api/cloud/files/<path>`: a file, or a folder and all it holds. → `{deleted, used, quota}`. */
    async remove(userId, plan, path) {
      const state = await userState(userId);
      return locked(state, async () => {
        const kind = kindOf(state.index, path);
        if (!kind) throw notFound(path);
        const next = clone(state.index);
        const dropped = [];
        for (const p of Object.keys(next.files)) {
          if (p === path || under(p, path)) {
            dropped.push(next.files[p].sha256);
            delete next.files[p];
          }
        }
        for (const p of Object.keys(next.folders)) if (p === path || under(p, path)) delete next.folders[p];
        await commit(state, userId, next, dropped);
        return { deleted: dropped.length, used: state.used, quota: limits(plan).quota };
      });
    },

    /** `POST /api/cloud/folders`. → `{created, folder}`; an existing folder becomes one of its own. */
    async mkdir(userId, plan, path) {
      const state = await userState(userId);
      return locked(state, async () => {
        checkParents(state.index, path);
        const kind = kindOf(state.index, path);
        if (kind === 'file') throw conflict(path, `${path} is a file.`);
        if (state.index.folders[path]) return { created: false, folder: { path, createdAt: state.index.folders[path].createdAt } };
        const next = clone(state.index);
        next.folders[path] = { createdAt: new Date().toISOString() };
        await commit(state, userId, next);
        return { created: kind === null, folder: { path, createdAt: next.folders[path].createdAt } };
      });
    },

    /**
     * `POST /api/cloud/move`: renames a file, or a folder with all it holds. With `overwrite`, a
     * file replaces the one at `to` and a folder merges into the one at `to`. → `{moved, used, quota}`.
     */
    async move(userId, plan, from, to, overwrite = false) {
      const state = await userState(userId);
      return locked(state, async () => {
        const index = state.index;
        const kind = kindOf(index, from);
        if (!kind) throw notFound(from);
        const result = (moved) => ({ moved, used: state.used, quota: limits(plan).quota });
        if (from === to) return result(0);
        if (kind === 'folder' && under(to, from)) throw conflict(to, `A folder can't be moved into itself (${from} → ${to}).`);
        checkParents(index, to);
        const target = kindOf(index, to);
        if (target && target !== kind) throw conflict(to, `${to} is a ${target}; ${from} is a ${kind}.`);
        if (target && !overwrite) throw conflict(to, `${to} already exists. Move with overwrite to replace it.`);

        const next = clone(index);
        const dropped = [];
        const moves = kind === 'file' ? [[from, to]] : Object.keys(index.files).filter((p) => under(p, from)).map((p) => [p, to + p.slice(from.length)]);
        for (const [a] of moves) delete next.files[a];
        for (const [a, b] of moves) {
          if (next.files[b]) dropped.push(next.files[b].sha256);
          next.files[b] = index.files[a];
        }
        if (kind === 'folder') {
          for (const p of Object.keys(index.folders)) {
            if (p !== from && !under(p, from)) continue;
            delete next.folders[p];
            next.folders[to + p.slice(from.length)] ??= index.folders[p];
          }
        }
        // A merge must not put a file where a folder is, or the reverse.
        for (const p of [...Object.keys(next.files), ...Object.keys(next.folders)]) {
          const file = ancestors(p).find((a) => next.files[a]);
          if (file) throw conflict(file, `${file} is a file in one folder and a folder in the other: nothing was moved.`);
          if (next.files[p] && next.folders[p]) throw conflict(p, `${p} is a file in one folder and a folder in the other: nothing was moved.`);
        }
        await commit(state, userId, next, dropped);
        return result(moves.length);
      });
    },
  };
}
