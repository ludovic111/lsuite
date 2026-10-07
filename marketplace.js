// The lsuite Marketplace (MARKETPLACE.md): plugins people publish for the five apps, every version
// reviewed before anyone can install it; dependency-free. `ai.js` authenticates and routes
// `/api/marketplace…` here.
//
// - Storage: `<root>/index.json` (`{format: 1, listings: {id: {…, versions: {version: {…}}}},
//   downloads: {id: {total, versions: {version: n}}}}`, 0600, written atomically) and
//   `files/<sha256>`, one per distinct bundle; a file goes once no version names it. `<root>` is
//   `<dataDir>/marketplace` (`production-marketplace` in production), else a temporary folder
//   removed at exit.
// - Uploads stream to `<root>/.tmp/` while being hashed and counted (cloud.js's `receive`); the
//   archive is then read without unpacking it (gunzip as a stream, ustar headers) and its
//   `plugin.toml` checked against the submission before anything is kept.
// - Changes run one at a time (one promise chain); each builds a new index and swaps it in once
//   written. Download counts live beside the listings and are written a moment later.
// - Files can live in lsuite Cloud's object store (`objectStoreConfig()` in cloud.js), keys
//   `[<prefix>/]<marketplace|production-marketplace>/files/<sha256>`; the index stays in the data dir.
import { createHash, randomBytes } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, open, readdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createGunzip } from 'node:zlib';
import { bytesFrom, CloudError, diskReserve, freeSpace, objectStore, receive, storageLabel, temporaryDir } from './cloud.js';

/** The platforms a version can be uploaded for, and the `[library]` key each one reads. */
export const PLATFORMS = { 'macos-arm64': 'macos', 'macos-x86_64': 'macos', 'linux-x86_64': 'linux', 'windows-x86_64': 'windows' };
/** The apps plugins are made for. */
export const MARKET_APPS = ['ryolune', 'kimchi', 'zenith', 'nori', 'folio'];
/** Ids that are lsuite's own: only admins publish under them. */
export const RESERVED_PREFIX = 'xyz.lsuite.';

/** Caps (MARKETPLACE.md) from `LSUITE_MARKET_MAX_FILE` and `LSUITE_MARKET_TOTAL` (bytes): 15 MB a file, 80 MB in all. */
export function marketCaps(env = process.env) {
  return { maxFile: bytesFrom(env.LSUITE_MARKET_MAX_FILE, 15e6), total: bytesFrom(env.LSUITE_MARKET_TOTAL, 80e6) };
}

/** The admins' emails, from `LSUITE_ADMIN_EMAILS` (comma-separated), lowercased. */
export function adminEmails(env = process.env) {
  return String(env.LSUITE_ADMIN_EMAILS ?? '').split(',').map((e) => e.trim().toLowerCase()).filter(Boolean);
}

const ID = /^[a-z0-9][a-z0-9_-]*(\.[a-z0-9][a-z0-9_-]*)+$/;
const VERSION = /^\d{1,9}\.\d{1,9}\.\d{1,9}(-[0-9A-Za-z.-]{1,40})?(\+[0-9A-Za-z.-]{1,40})?$/;
const KIND = /^[a-z][a-z0-9-]{0,31}$/;
const SHA = /^[0-9a-f]{64}$/;
const CONTROL = /[\u0000-\u001f\u007f]/;
const MANIFEST_MAX = 64 * 1024;

const bad = (message, extra) => new CloudError(400, 'invalid_request_error', message, extra);
const invalid = (message) => new CloudError(400, 'invalid_plugin', message);
const notFound = (message) => new CloudError(404, 'not_found_error', message);
const conflict = (message) => new CloudError(409, 'conflict_error', message);
const notOwner = (message) => new CloudError(403, 'not_owner', message);

/** Semantic-version order: numbers compared as numbers, a pre-release before its release. */
export function compareVersions(a, b) {
  const parse = (v) => {
    const [core, pre = null] = v.split('+')[0].split(/-(.*)/s);
    return { nums: core.split('.').map(Number), pre };
  };
  const x = parse(a);
  const y = parse(b);
  for (let i = 0; i < 3; i++) if (x.nums[i] !== y.nums[i]) return x.nums[i] - y.nums[i];
  if (x.pre === y.pre) return 0;
  if (x.pre === null) return 1;
  if (y.pre === null) return -1;
  const p = x.pre.split('.');
  const q = y.pre.split('.');
  for (let i = 0; i < Math.max(p.length, q.length); i++) {
    if (p[i] === undefined) return -1;
    if (q[i] === undefined) return 1;
    const m = /^\d+$/.test(p[i]);
    const n = /^\d+$/.test(q[i]);
    if (m && n && Number(p[i]) !== Number(q[i])) return Number(p[i]) - Number(q[i]);
    if (m !== n) return m ? -1 : 1;
    if (p[i] !== q[i]) return p[i] < q[i] ? -1 : 1;
  }
  return 0;
}

// ---- plugin.toml: enough TOML for a manifest (tables, strings, numbers, booleans, arrays) ----

const ESCAPES = { b: '\b', t: '\t', n: '\n', f: '\f', r: '\r', '"': '"', '\\': '\\' };

/** Parses TOML text into plain objects; throws an Error naming the line on anything it can't read. */
export function parseToml(text) {
  const root = {};
  let table = root;
  let i = 0;
  let line = 1;
  const n = text.length;
  const fail = (why) => {
    throw new Error(`${why} (line ${line})`);
  };
  const ws = () => {
    while (text[i] === ' ' || text[i] === '\t') i++;
  };
  const comment = () => {
    if (text[i] === '#') while (i < n && text[i] !== '\n') i++;
  };
  /** Blank lines, comments and newlines. */
  const blank = () => {
    for (;;) {
      ws();
      comment();
      if (text[i] === '\r' && text[i + 1] === '\n') i++;
      if (text[i] !== '\n') return;
      i++;
      line++;
    }
  };
  const eol = () => {
    ws();
    comment();
    if (i < n && text[i] !== '\n' && !(text[i] === '\r' && text[i + 1] === '\n')) fail('Expected the end of the line');
  };
  const unescape = (s) =>
    s.replace(/\\(u[0-9a-fA-F]{4}|U[0-9a-fA-F]{8}|.)/gs, (_, e) => ESCAPES[e] ?? (e.length > 1 ? String.fromCodePoint(parseInt(e.slice(1), 16)) : fail('Unknown escape')));
  const string = () => {
    const triple = text.slice(i, i + 3);
    if (triple === '"""' || triple === "'''") {
      i += 3;
      if (text[i] === '\r' && text[i + 1] === '\n') i++;
      if (text[i] === '\n') i++, line++;
      const end = text.indexOf(triple, i);
      if (end < 0) fail('A string is never closed');
      const s = text.slice(i, end);
      line += s.split('\n').length - 1;
      i = end + 3;
      return triple === '"""' ? unescape(s.replace(/\\[ \t]*\r?\n\s*/g, '')) : s;
    }
    const quote = text[i++];
    let s = '';
    while (text[i] !== quote) {
      if (i >= n || text[i] === '\n') fail('A string is never closed');
      if (quote === '"' && text[i] === '\\') {
        s += text.slice(i, i + 2);
        i += 2;
      } else s += text[i++];
    }
    i++;
    return quote === '"' ? unescape(s) : s;
  };
  const BARE = /[A-Za-z0-9_-]+/y;
  const key = () => {
    const parts = [];
    for (;;) {
      ws();
      if (text[i] === '"' || text[i] === "'") parts.push(string());
      else {
        BARE.lastIndex = i;
        const m = BARE.exec(text);
        if (!m) fail('Expected a key');
        parts.push(m[0]);
        i += m[0].length;
      }
      if (['__proto__', 'constructor', 'prototype'].includes(parts.at(-1))) fail('Reserved key');
      ws();
      if (text[i] !== '.') return parts;
      i++;
    }
  };
  /** Walks `keys` from `obj`, making tables on the way; returns the last table and key. */
  const walk = (obj, keys) => {
    let t = obj;
    for (const part of keys.slice(0, -1)) {
      if (t[part] === undefined) t[part] = {};
      t = Array.isArray(t[part]) ? t[part].at(-1) : t[part];
      if (!t || typeof t !== 'object') fail(`${part} is not a table`);
    }
    return [t, keys.at(-1)];
  };
  const assign = (obj, keys, value) => {
    const [t, last] = walk(obj, keys);
    if (Object.hasOwn(t, last)) fail(`${keys.join('.')} is defined twice`);
    t[last] = value;
  };
  const TOKEN = /[^\s,\]}#]+/y;
  const value = () => {
    ws();
    const c = text[i];
    if (c === '"' || c === "'") return string();
    if (c === '[') {
      i++;
      const out = [];
      for (;;) {
        blank();
        if (text[i] === ']') return i++, out;
        out.push(value());
        blank();
        if (text[i] === ',') i++;
        else if (text[i] !== ']') fail('Expected , or ] in an array');
      }
    }
    if (c === '{') {
      i++;
      const out = {};
      ws();
      if (text[i] === '}') return i++, out;
      for (;;) {
        const k = key();
        if (text[i] !== '=') fail('Expected =');
        i++;
        assign(out, k, value());
        ws();
        if (text[i] === '}') return i++, out;
        if (text[i] !== ',') fail('Expected , or } in an inline table');
        i++;
      }
    }
    TOKEN.lastIndex = i;
    const m = TOKEN.exec(text);
    if (!m) fail('Expected a value');
    i += m[0].length;
    if (m[0] === 'true' || m[0] === 'false') return m[0] === 'true';
    if (/^[+-]?\d[\d_]*(\.\d[\d_]*)?([eE][+-]?\d+)?$/.test(m[0])) return Number(m[0].replace(/_/g, ''));
    // Dates, hexadecimal and the like stay as written.
    return m[0];
  };

  for (;;) {
    blank();
    if (i >= n) return root;
    if (text[i] === '[') {
      const array = text[i + 1] === '[';
      i += array ? 2 : 1;
      const keys = key();
      if (text[i] !== ']' || (array && text[i + 1] !== ']')) fail('Expected ] after a table name');
      i += array ? 2 : 1;
      const [t, last] = walk(root, keys);
      if (array) {
        if (t[last] === undefined) t[last] = [];
        if (!Array.isArray(t[last])) fail(`${keys.join('.')} is not an array of tables`);
        t[last].push((table = {}));
      } else {
        if (t[last] === undefined) t[last] = {};
        if (!t[last] || typeof t[last] !== 'object' || Array.isArray(t[last])) fail(`${keys.join('.')} is not a table`);
        table = t[last];
      }
    } else {
      const keys = key();
      if (text[i] !== '=') fail('Expected = after a key');
      i++;
      assign(table, keys, value());
    }
    eol();
  }
}

// ---- The archive: a .tar.gz read as a stream, never unpacked ----

const cstr = (buf, start, length) => {
  const s = buf.subarray(start, start + length);
  const end = s.indexOf(0);
  return s.subarray(0, end < 0 ? s.length : end).toString('utf8');
};

/** A tar number field: octal text, or GNU's base-256 when the high bit is set. */
function tarNumber(buf, start, length) {
  if (buf[start] & 0x80) {
    let v = buf[start] & 0x7f;
    for (let k = start + 1; k < start + length; k++) v = v * 256 + buf[k];
    return v;
  }
  const s = cstr(buf, start, length).trim();
  return s ? parseInt(s, 8) : 0;
}

/** A path inside a bundle, without `./`; null for the archive's own root. Refuses anything that could land outside. */
function entryPath(raw) {
  let path = raw.replace(/^(\.\/)+/, '').replace(/\/+$/, '');
  if (path === '' || path === '.') return null;
  if (path.startsWith('/') || path.includes('\\') || CONTROL.test(path) || path.split('/').some((s) => s === '' || s === '.' || s === '..')) {
    throw invalid(`The archive holds an unsafe path (${JSON.stringify(raw.slice(0, 120))}).`);
  }
  return path;
}

/**
 * Reads `file` (a `.tar.gz`) through: `{entries: Map<path, 'file'|'dir'>, contents: Map<path,
 * Buffer>}`, `contents` holding the files `want(path)` asks for (64 KB at most each). Links,
 * devices and unsafe paths are refused; so is an archive unpacking to more than `maxUnpacked`.
 */
async function readArchive(file, { maxUnpacked, want }) {
  const entries = new Map();
  const contents = new Map();
  const input = createReadStream(file);
  const gunzip = createGunzip();
  input.on('error', (err) => gunzip.destroy(err));
  input.pipe(gunzip);
  let buf = Buffer.alloc(0);
  let body = null;
  let pax = null;
  let longName = null;
  let total = 0;
  let done = false;

  /** What follows a header: `left` bytes, kept when `sink`, then padding to 512. */
  const follow = (size, keep, then) => {
    if (keep && size > MANIFEST_MAX) throw invalid('A manifest or header in the archive is larger than 64 KB.');
    return { left: size, pad: (512 - (size % 512)) % 512, sink: keep ? [] : null, then };
  };

  const header = (h) => {
    if (h.every((b) => b === 0)) {
      done = true;
      return null;
    }
    let sum = 8 * 32;
    for (let k = 0; k < 512; k++) if (k < 148 || k >= 156) sum += h[k];
    if (sum !== tarNumber(h, 148, 8)) throw invalid('This isn’t a tar archive inside the gzip (a header’s checksum is wrong).');
    const type = h[156] === 0 ? '0' : String.fromCharCode(h[156]);
    let size = tarNumber(h, 124, 12);
    let name = cstr(h, 0, 100);
    // POSIX ustar keeps long paths' folders in `prefix`; GNU's "ustar  " uses those bytes otherwise.
    if (h.toString('latin1', 257, 263) === 'ustar\0') {
      const prefix = cstr(h, 345, 155);
      if (prefix) name = `${prefix}/${name}`;
    }
    if (type === 'x') return follow(size, true, (data) => (pax = parsePax(data)));
    if (type === 'g' || type === 'K') return follow(size, false);
    if (type === 'L') return follow(size, true, (data) => (longName = cstr(data, 0, data.length)));
    if (longName !== null) name = longName;
    if (pax?.path) name = pax.path;
    if (pax?.size !== undefined && /^\d+$/.test(pax.size)) size = Number(pax.size);
    longName = null;
    pax = null;
    const path = entryPath(name);
    if (type === '5') {
      if (path) entries.set(path, 'dir');
      return follow(size, false);
    }
    if (type === '1' || type === '2') throw invalid(`A bundle can't hold links (${path}).`);
    if (type !== '0' && type !== '7') throw invalid(`A bundle holds only files and folders (${path} is of tar type ${JSON.stringify(type)}).`);
    if (!path) throw invalid('The archive holds a file without a name.');
    entries.set(path, 'file');
    return want(path) ? follow(size, true, (data) => contents.set(path, data)) : follow(size, false);
  };

  try {
    for await (const chunk of gunzip) {
      total += chunk.length;
      if (total > maxUnpacked) throw invalid(`The archive unpacks to more than ${storageLabel(maxUnpacked)}.`);
      buf = buf.length ? Buffer.concat([buf, chunk]) : chunk;
      while (!done) {
        if (body) {
          const take = Math.min(body.left, buf.length);
          if (body.sink && take) body.sink.push(buf.subarray(0, take));
          body.left -= take;
          buf = buf.subarray(take);
          if (body.left) break;
          const skip = Math.min(body.pad, buf.length);
          body.pad -= skip;
          buf = buf.subarray(skip);
          if (body.pad) break;
          body.then?.(Buffer.concat(body.sink ?? []));
          body = null;
          continue;
        }
        if (buf.length < 512) break;
        const h = buf.subarray(0, 512);
        buf = buf.subarray(512);
        body = header(h);
      }
      if (done) break;
    }
  } catch (err) {
    if (err instanceof CloudError) throw err;
    throw invalid(`The bundle isn’t a readable .tar.gz (${String(err.message).split('\n')[0]}).`);
  } finally {
    input.destroy();
    gunzip.destroy();
  }
  if (!done && (body || buf.length)) throw invalid('The archive ends in the middle of a file.');
  return { entries, contents };
}

/** A pax extended header's records (`<length> <key>=<value>\n`). */
function parsePax(data) {
  const out = {};
  let at = 0;
  while (at < data.length) {
    const space = data.indexOf(0x20, at);
    const length = Number(data.toString('latin1', at, space));
    if (space < 0 || !Number.isInteger(length) || length <= 0) break;
    const record = data.toString('utf8', space + 1, at + length - 1);
    const eq = record.indexOf('=');
    if (eq > 0) out[record.slice(0, eq)] = record.slice(eq + 1);
    at += length;
  }
  return out;
}

/** The manifest's fields the marketplace keeps (shown to reviewers). */
function manifestOf(toml) {
  const pick = (v) => (typeof v === 'string' || typeof v === 'number' ? v : undefined);
  const library = Object.fromEntries(Object.entries(toml.library && typeof toml.library === 'object' ? toml.library : {}).filter(([, v]) => typeof v === 'string'));
  const authors = Array.isArray(toml.authors) ? toml.authors.filter((a) => typeof a === 'string').slice(0, 20) : [];
  return { id: pick(toml.id), name: pick(toml.name), version: pick(toml.version), app: pick(toml.app), kind: pick(toml.kind), abi: pick(toml.abi), description: pick(toml.description), authors, library };
}

/**
 * Checks a received bundle against its submission (MARKETPLACE.md, "Bundle files"): gzip, one top
 * folder, `plugin.toml` whose `id`, `version` and `app` (and `abi`, when it has one) match, and the
 * platform's `[library]` file in the folder. → `{manifest, files}`.
 */
export async function inspectBundle(file, { id, version, app, abi, platform, maxUnpacked }) {
  const handle = await open(file, 'r');
  try {
    const { buffer, bytesRead } = await handle.read(Buffer.alloc(2), 0, 2, 0);
    if (bytesRead < 2 || buffer[0] !== 0x1f || buffer[1] !== 0x8b) throw invalid('The bundle isn’t a gzip archive: upload the .tar.gz.');
  } finally {
    await handle.close();
  }
  const { entries, contents } = await readArchive(file, { maxUnpacked, want: (path) => /^[^/]+\/plugin\.toml$/.test(path) });
  if (!entries.size) throw invalid('The archive is empty.');
  const tops = [...new Set([...entries.keys()].map((p) => p.split('/')[0]))];
  if (tops.length !== 1) throw invalid(`A bundle is one top folder; this archive has ${tops.length} things at its top (${tops.slice(0, 3).join(', ')}${tops.length > 3 ? ', …' : ''}).`);
  const [top] = tops;
  if (entries.get(top) === 'file') throw invalid(`A bundle is one top folder; ${top} is a file.`);
  const text = contents.get(`${top}/plugin.toml`);
  if (!text) throw invalid(`There's no plugin.toml in ${top}/.`);
  let toml;
  try {
    toml = parseToml(text.toString('utf8'));
  } catch (err) {
    throw invalid(`plugin.toml isn't valid TOML: ${err.message}.`);
  }
  const manifest = manifestOf(toml);
  for (const [field, expected] of [['id', id], ['version', version], ['app', app]]) {
    if (manifest[field] !== expected) throw invalid(`plugin.toml says ${field} = ${JSON.stringify(manifest[field] ?? null)}; the submission says ${JSON.stringify(expected)}.`);
  }
  if (manifest.abi !== undefined && manifest.abi !== abi) throw invalid(`plugin.toml says abi = ${JSON.stringify(manifest.abi)}; the submission says ${abi}.`);
  const os = PLATFORMS[platform];
  const library = manifest.library[os];
  if (!library) throw invalid(`plugin.toml has no [library] ${os} entry, which ${platform} needs.`);
  const libraryPath = `${top}/${entryPath(library) ?? ''}`;
  if (entries.get(libraryPath) !== 'file') throw invalid(`The library plugin.toml names for ${os} (${library}) isn't in the archive.`);
  const files = [...entries].filter(([, kind]) => kind === 'file').map(([p]) => p.slice(top.length + 1)).sort();
  return { manifest, files: files.slice(0, 200), fileCount: files.length };
}

// ---- The store ----

const emptyIndex = () => ({ format: 1, listings: {} });
const clone = (value) => structuredClone(value);
/** Text from a JSON body: trimmed, at most `max` characters, one line unless `multiline`. */
function text(value, field, { max, min = 1, multiline = false, optional = false }) {
  if ((value === undefined || value === null || value === '') && optional) return '';
  if (typeof value !== 'string') throw bad(`${field}: a string is required.`);
  const s = value.trim();
  if (s.length < min || s.length > max) throw bad(`${field}: ${min} to ${max} characters.`);
  if (multiline ? /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(s) : CONTROL.test(s)) throw bad(`${field}: ${multiline ? 'no control characters' : 'one line, no control characters'}.`);
  return s;
}

/**
 * The marketplace. Options: `dataDir` (else a temporary folder), `production`, `caps` (`{maxFile,
 * total}`, `marketCaps()` by default), `diskReserve` and `statfs` (cloud.js's disk guard), `store`
 * (`objectStoreConfig()`: files go to the object store; needs `dataDir`), `passPlans` (the plan ids
 * that include the marketplace), `who(userId)` (the account behind a listing, for reviewers).
 */
export function createMarketplace(options = {}) {
  const caps = { ...marketCaps(), ...(options.caps ?? {}) };
  if (options.store && !options.dataDir) throw new Error('The marketplace\'s object store needs LSUITE_DATA_DIR: its index lives there.');
  const store = options.store ? objectStore(options.store, options.storeOptions) : null;
  const area = options.production ? 'production-marketplace' : 'marketplace';
  const fileKey = (sha) => [store.prefix, area, 'files', sha].filter(Boolean).join('/');
  const maxUnpacked = caps.maxFile * 10;
  let index = emptyIndex();
  let downloads = {};
  let used = 0;
  let reserved = 0;
  let chain = Promise.resolve();
  let base = null;
  let countsDirty = false;

  /** Every file a version names, by hash: its size. */
  const named = (idx = index) => {
    const out = new Map();
    for (const l of Object.values(idx.listings)) for (const v of Object.values(l.versions)) for (const f of Object.values(v.platforms)) out.set(f.sha256, f.size);
    return out;
  };
  const usedOf = (idx) => [...named(idx).values()].reduce((a, b) => a + b, 0);

  /** The root folder, made (and the index read, leftovers swept) on first use. */
  function root() {
    base ??= (async () => {
      const dir = options.dataDir ? join(options.dataDir, area) : await temporaryDir('lsuite-marketplace-');
      await mkdir(join(dir, 'files'), { recursive: true, mode: 0o700 });
      await rm(join(dir, '.tmp'), { recursive: true, force: true });
      await mkdir(join(dir, '.tmp'), { mode: 0o700 });
      try {
        const data = JSON.parse(await readFile(join(dir, 'index.json'), 'utf8'));
        if (data?.format === 1) {
          index = { format: 1, listings: data.listings ?? {} };
          downloads = data.downloads ?? {};
        }
      } catch (err) {
        if (err.code !== 'ENOENT') throw err;
      }
      used = usedOf(index);
      // Files no version names (a write cut short) are dropped; in the store, only if it answers.
      const keep = named();
      if (store) {
        locked(async () => {
          const keys = await store.list(`${[store.prefix, area, 'files'].filter(Boolean).join('/')}/`);
          await Promise.all(keys.filter((k) => !keep.has(k.split('/').at(-1))).map((k) => store.remove(k)));
        }).catch(() => {});
      } else {
        const files = await readdir(join(dir, 'files')).catch(() => []);
        await Promise.all(files.filter((f) => !keep.has(f)).map((f) => rm(join(dir, 'files', f), { force: true })));
      }
      return dir;
    })();
    return base;
  }

  /** Runs `work` after every change before it. */
  function locked(work) {
    const run = chain.catch(() => {}).then(work);
    chain = run.catch(() => {});
    return run;
  }

  async function write(next) {
    const file = join(await root(), 'index.json');
    const tmp = `${file}.${process.pid}.${randomBytes(4).toString('hex')}.tmp`;
    await writeFile(tmp, JSON.stringify({ ...next, downloads }), { mode: 0o600 });
    await rename(tmp, file);
  }

  /** Writes `next`, swaps it in and drops the files it no longer names (in the lock). */
  async function commit(next, dropped = []) {
    await write(next);
    index = next;
    used = usedOf(next);
    const keep = named();
    const gone = [...new Set(dropped)].filter((sha) => !keep.has(sha));
    const dir = await root();
    if (store) await Promise.all(gone.map((sha) => store.remove(fileKey(sha)).catch(() => {})));
    else await Promise.all(gone.map((sha) => rm(join(dir, 'files', sha), { force: true })));
  }

  /** Download counts: written soon, once for any number of downloads. */
  function saveCounts() {
    if (countsDirty) return;
    countsDirty = true;
    setTimeout(() => {
      locked(async () => {
        countsDirty = false;
        await write(index);
      }).catch(() => {});
    }, 200).unref?.();
  }

  /** The approved versions a listing can show (with at least one file), newest first. */
  const approved = (l) => Object.values(l.versions).filter((v) => v.status === 'approved' && Object.keys(v.platforms).length).sort((a, b) => compareVersions(b.version, a.version));
  const publicPlatforms = (v) => Object.fromEntries(Object.entries(v.platforms).sort(([a], [b]) => (a < b ? -1 : 1)).map(([p, f]) => [p, { size: f.size, sha256: f.sha256 }]));

  /** A listing as MARKETPLACE.md shows it, or null when nothing of it is approved. */
  function listingOf(l) {
    const [v] = approved(l);
    if (!v) return null;
    return {
      id: l.id,
      app: l.app,
      name: v.name,
      kind: v.kind,
      description: v.description,
      version: v.version,
      abi: v.abi,
      author: { name: l.author, verified: Boolean(l.verified) },
      platforms: publicPlatforms(v),
      downloads: downloads[l.id]?.total ?? 0,
      updatedAt: v.approvedAt,
      notes: v.notes,
    };
  }

  /** A version as its author sees it (`/mine`, `{submission}`). */
  function submissionOf(l, v) {
    return {
      id: l.id,
      version: v.version,
      app: l.app,
      name: v.name,
      kind: v.kind,
      abi: v.abi,
      description: v.description,
      notes: v.notes,
      status: v.status,
      note: v.note ?? null,
      platforms: Object.fromEntries(Object.entries(v.platforms).map(([p, f]) => [p, { size: f.size, sha256: f.sha256, uploadedAt: f.uploadedAt }])),
      submittedAt: v.submittedAt,
      reviewedAt: v.reviewedAt ?? null,
      verified: Boolean(l.verified),
      downloads: downloads[l.id]?.versions?.[v.version] ?? 0,
    };
  }

  /** The listing and version a route names; 404 when either is missing. */
  function find(idx, id, version) {
    const l = Object.hasOwn(idx.listings, id) ? idx.listings[id] : null;
    if (!l) throw notFound(`Nothing in the lsuite Marketplace is called ${id}.`);
    const v = Object.hasOwn(l.versions, version) ? l.versions[version] : null;
    if (!v) throw notFound(`${id} has no version ${version}.`);
    return { l, v };
  }

  /** Checks an upload's room: the total cap (without a store), then the disk the uploads stream to. */
  async function checkRoom(size, freed = 0) {
    if (!store && used + reserved - freed + size > caps.total) {
      throw new CloudError(507, 'storage_full', `The lsuite Marketplace is full for now (${storageLabel(caps.total)} for every plugin together). Try again later.`, { used, quota: caps.total });
    }
    const free = await freeSpace(await root(), options.statfs);
    if (free !== null && free - reserved - size < (options.diskReserve ?? diskReserve())) {
      throw new CloudError(507, 'storage_full', 'The lsuite Marketplace has no room left on this server for now. Try again later.', {});
    }
  }

  return {
    caps,
    backend: store ? 'object-store' : 'disk',
    ready: () => root(),

    /** `GET /api/marketplace`: approved listings, newest first; `app` filters. */
    async catalogue(app = null) {
      await root();
      if (app !== null && !MARKET_APPS.includes(app)) throw bad(`app: one of ${MARKET_APPS.join(', ')}.`);
      const plugins = Object.values(index.listings)
        .filter((l) => !app || l.app === app)
        .map(listingOf)
        .filter(Boolean)
        .sort((a, b) => (a.updatedAt < b.updatedAt ? 1 : a.updatedAt > b.updatedAt ? -1 : a.id < b.id ? -1 : 1));
      return { plugins, apps: [...MARKET_APPS], pass: { required: true, plans: [...(options.passPlans ?? ['plus', 'pro', 'studio'])] } };
    },

    /** `GET /api/marketplace/plugins/<id>`: the listing and its approved versions. */
    async plugin(id) {
      await root();
      const l = Object.hasOwn(index.listings, id) ? index.listings[id] : null;
      const listing = l && listingOf(l);
      if (!listing) throw notFound(`Nothing in the lsuite Marketplace is called ${id}.`);
      return { ...listing, versions: approved(l).map((v) => ({ version: v.version, notes: v.notes, approvedAt: v.approvedAt, platforms: publicPlatforms(v) })) };
    },

    /**
     * The file a download names: an approved version (the newest by default) and one of its
     * platforms. → `{id, version, platform, size, sha256}`.
     */
    async find(id, platform, version = null) {
      await root();
      const l = Object.hasOwn(index.listings, id) ? index.listings[id] : null;
      const versions = l ? approved(l) : [];
      if (!versions.length) throw notFound(`Nothing in the lsuite Marketplace is called ${id}.`);
      const v = version === null ? versions[0] : versions.find((x) => x.version === version);
      if (!v) throw notFound(`${id} has no approved version ${version}.`);
      const f = Object.hasOwn(v.platforms, platform) ? v.platforms[platform] : null;
      if (!f) throw notFound(`${id} ${v.version} isn't available for ${platform} (it is for ${Object.keys(v.platforms).join(', ')}).`);
      return { id, version: v.version, platform, size: f.size, sha256: f.sha256 };
    },

    /** Any version's file, whatever its status (reviewers). */
    async pendingFile(id, version, platform) {
      await root();
      const { v } = find(index, id, version);
      const f = Object.hasOwn(v.platforms, platform) ? v.platforms[platform] : null;
      if (!f) throw notFound(`${id} ${version} has no file for ${platform}.`);
      return { id, version, platform, size: f.size, sha256: f.sha256 };
    },

    /** A stream of a file's bytes, opened before it is returned. */
    async open(file) {
      if (store) {
        const res = await store.get(fileKey(file.sha256));
        if (!res) throw notFound(`The file of ${file.id} ${file.version} is missing.`);
        return res;
      }
      try {
        return (await open(join(await root(), 'files', file.sha256), 'r')).createReadStream();
      } catch (err) {
        if (err.code === 'ENOENT') throw notFound(`The file of ${file.id} ${file.version} is missing.`);
        throw err;
      }
    },

    /** Counts one download of a version. */
    count(id, version) {
      const d = (downloads[id] ??= { total: 0, versions: {} });
      d.total += 1;
      d.versions[version] = (d.versions[version] ?? 0) + 1;
      saveCounts();
    },

    /**
     * `POST /api/marketplace/submit`: a new version, pending (approved at once, and the listing
     * verified, for an admin). `user` is `{id, name}`.
     */
    async submit(user, admin, body) {
      const id = text(body.id, 'id', { max: 128 });
      if (!ID.test(id)) throw bad('id: reverse-DNS in lowercase, like com.example.tape-warmth (letters, digits, - and _ between dots).');
      if (!MARKET_APPS.includes(body.app)) throw bad(`app: one of ${MARKET_APPS.join(', ')}.`);
      const version = text(body.version, 'version', { max: 64 });
      if (!VERSION.test(version)) throw bad('version: a semantic version, like 1.2.0.');
      const sub = {
        name: text(body.name, 'name', { max: 60 }),
        kind: text(body.kind, 'kind', { max: 32 }),
        abi: body.abi,
        description: text(body.description, 'description', { max: 300 }),
        notes: text(body.notes, 'notes', { max: 2000, multiline: true, optional: true }),
      };
      if (!KIND.test(sub.kind)) throw bad('kind: the plugin’s kind from plugin.toml, like effect or filter.');
      if (!Number.isSafeInteger(sub.abi) || sub.abi < 1) throw bad('abi: the plugin ABI version, a positive integer.');
      if (id.startsWith(RESERVED_PREFIX) && !admin) throw notOwner(`Ids starting ${RESERVED_PREFIX} are lsuite’s own: pick an id of yours, like com.yourname.${id.split('.').at(-1)}.`);
      await root();
      return locked(async () => {
        const next = clone(index);
        const now = new Date().toISOString();
        let l = next.listings[id];
        if (l && l.owner !== user.id) throw notOwner(`${id} belongs to another account: only its owner publishes new versions. Pick another id.`);
        if (l && l.app !== body.app) throw conflict(`${id} is a ${l.app} plugin; it can’t become a ${body.app} one. Pick another id.`);
        l ??= next.listings[id] = { id, app: body.app, owner: user.id, createdAt: now, versions: {} };
        l.author = user.name;
        l.verified = Boolean(admin);
        const old = l.versions[version];
        if (old?.status === 'approved') throw conflict(`${id} ${version} is approved and can’t be replaced: publish a new version.`);
        const v = (l.versions[version] = { version, ...sub, status: 'pending', note: null, platforms: old?.platforms ?? {}, submittedAt: now, reviewedAt: null, approvedAt: null });
        if (admin) Object.assign(v, { status: 'approved', reviewedAt: now, approvedAt: now });
        await commit(next);
        return submissionOf(l, v);
      });
    },

    /**
     * `PUT /api/marketplace/submit/<id>/<version>/<platform>`: streams `req` in, checks the bundle
     * and attaches it to the pending version. An admin may also add a platform to an approved one of
     * theirs (never replace one). Headers: `content-length` (required), `x-lsuite-sha256`.
     */
    async upload(user, admin, id, version, platform, req) {
      if (!Object.hasOwn(PLATFORMS, platform)) throw bad(`platform: one of ${Object.keys(PLATFORMS).join(', ')}.`);
      await root();
      /** Refuses an upload the version can't take now (checked again in the lock). */
      const check = (idx) => {
        const { l, v } = find(idx, id, version);
        if (l.owner !== user.id) throw notOwner(`${id} belongs to another account.`);
        if (v.status === 'rejected') throw conflict(`${id} ${version} was rejected: submit it again (POST /api/marketplace/submit), then upload.`);
        if (v.status === 'approved' && (!admin || v.platforms[platform])) throw conflict(`${id} ${version} is approved and can’t be replaced: publish a new version.`);
        return { l, v };
      };
      const { l, v } = check(index);
      const declared = req.headers['content-length'];
      if (declared === undefined || !/^\d+$/.test(declared)) throw new CloudError(411, 'length_required', 'Send the bundle’s size in Content-Length: the marketplace doesn’t take chunked uploads.');
      const length = Number(declared);
      const expected = req.headers['x-lsuite-sha256'] === undefined ? null : String(req.headers['x-lsuite-sha256']).trim().toLowerCase();
      if (expected !== null && !SHA.test(expected)) throw bad('X-Lsuite-Sha256 must be the SHA-256 of the body, 64 hex digits.');
      if (length > caps.maxFile) throw new CloudError(413, 'request_too_large', `This bundle is ${storageLabel(length)}; the marketplace takes ${storageLabel(caps.maxFile)} at most per file.`, { max_file: caps.maxFile });
      const old = v.platforms[platform];
      const freed = old && [...Object.values(index.listings)].flatMap((x) => Object.values(x.versions)).flatMap((x) => Object.values(x.platforms)).filter((f) => f.sha256 === old.sha256).length === 1 ? old.size : 0;
      await checkRoom(length, freed);
      reserved += length;
      const tmp = join(await root(), '.tmp', randomBytes(12).toString('hex'));
      let got;
      let inspected;
      try {
        try {
          got = await receive(req, tmp, length);
        } finally {
          reserved -= length;
        }
        if (expected && expected !== got.sha256) throw new CloudError(400, 'checksum_mismatch', `The upload doesn't match X-Lsuite-Sha256 (received ${got.sha256}). Nothing was saved; send it again.`);
        inspected = await inspectBundle(tmp, { id, version, app: l.app, abi: v.abi, platform, maxUnpacked });
      } catch (err) {
        await rm(tmp, { force: true });
        throw err;
      }
      return locked(async () => {
        try {
          const next = clone(index);
          const { l: nl, v: nv } = check(next);
          const previous = nv.platforms[platform];
          if (!store && used - (previous && freed ? previous.size : 0) + got.size > caps.total) {
            throw new CloudError(507, 'storage_full', `The lsuite Marketplace is full for now (${storageLabel(caps.total)} for every plugin together). Try again later.`, { used, quota: caps.total });
          }
          let stored = false;
          if (!named().has(got.sha256)) {
            if (store) {
              await store.put(fileKey(got.sha256), tmp, got.size, got.sha256);
              stored = true;
            } else if (!(await stat(join(await root(), 'files', got.sha256)).catch(() => null))) {
              await rename(tmp, join(await root(), 'files', got.sha256));
            }
          }
          nv.platforms[platform] = { size: got.size, sha256: got.sha256, uploadedAt: new Date().toISOString(), manifest: inspected.manifest, files: inspected.files, fileCount: inspected.fileCount };
          try {
            await commit(next, previous ? [previous.sha256] : []);
          } catch (err) {
            if (stored && index !== next) await store.remove(fileKey(got.sha256)).catch(() => {});
            throw err;
          }
          return submissionOf(nl, nv);
        } finally {
          await rm(tmp, { force: true });
        }
      });
    },

    /** `GET /api/marketplace/mine`: every version the account submitted, newest first. */
    async mine(userId) {
      await root();
      return Object.values(index.listings)
        .filter((l) => l.owner === userId)
        .flatMap((l) => Object.values(l.versions).map((v) => submissionOf(l, v)))
        .sort((a, b) => (a.submittedAt < b.submittedAt ? 1 : a.submittedAt > b.submittedAt ? -1 : 0));
    },

    /** `GET /api/marketplace/review`: pending versions, oldest first, with their manifests and files. */
    async pending() {
      await root();
      return Object.values(index.listings)
        .flatMap((l) =>
          Object.values(l.versions)
            .filter((v) => v.status === 'pending')
            .map((v) => {
              const owner = options.who?.(l.owner);
              return {
                ...submissionOf(l, v),
                author: { name: l.author, email: owner?.email ?? null, verified: Boolean(l.verified) },
                platforms: Object.fromEntries(Object.entries(v.platforms).map(([p, f]) => [p, { size: f.size, sha256: f.sha256, uploadedAt: f.uploadedAt, manifest: f.manifest, files: f.files, fileCount: f.fileCount ?? f.files?.length ?? 0 }])),
              };
            }),
        )
        .sort((a, b) => (a.submittedAt < b.submittedAt ? -1 : a.submittedAt > b.submittedAt ? 1 : 0));
    },

    /**
     * `POST /api/marketplace/review`: approve a pending version (it is listed), or reject it (the
     * note is kept for the author, its files dropped; an approved version rejected is taken down).
     */
    async review(body) {
      const id = text(body.id, 'id', { max: 128 });
      const version = text(body.version, 'version', { max: 64 });
      if (body.decision !== 'approve' && body.decision !== 'reject') throw bad('decision: approve or reject.');
      const note = text(body.note, 'note', { max: 2000, multiline: true, optional: true }) || null;
      await root();
      return locked(async () => {
        const next = clone(index);
        const { l, v } = find(next, id, version);
        const now = new Date().toISOString();
        const dropped = [];
        if (body.decision === 'approve') {
          if (v.status !== 'pending') throw conflict(`${id} ${version} is ${v.status}, not pending.`);
          if (!Object.keys(v.platforms).length) throw bad(`${id} ${version} has no bundle yet: nothing to approve.`);
          Object.assign(v, { status: 'approved', note, reviewedAt: now, approvedAt: now });
        } else {
          if (v.status === 'rejected') throw conflict(`${id} ${version} is already rejected.`);
          dropped.push(...Object.values(v.platforms).map((f) => f.sha256));
          Object.assign(v, { status: 'rejected', note, reviewedAt: now, approvedAt: null, platforms: {} });
        }
        await commit(next, dropped);
        return submissionOf(l, v);
      });
    },

    /** `{used, total, maxFile}` of the marketplace's files. */
    usage: () => ({ used, total: store ? null : caps.total, maxFile: caps.maxFile }),
  };
}

/** `Content-Disposition` for a bundle download. */
export const bundleName = (file) => `${file.id}-${file.version}-${file.platform}.tar.gz`.replace(/[^\w.+-]/g, '_');

/** SHA-256 of a buffer, hex (tests and tools). */
export const sha256 = (data) => createHash('sha256').update(data).digest('hex');
