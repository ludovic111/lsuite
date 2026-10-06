// lsuite accounts and lsuite AI (AI.md), dependency-free. `server.js` hands every `/api/` request
// to `accounts.handle()`.
//
// - Storage: one JSON file, `<LSUITE_DATA_DIR>/accounts.json`, written atomically (temporary file,
//   then rename) and only by this process; in memory when `LSUITE_DATA_DIR` is unset.
// - Secrets are never stored: tokens (`lsk_…` for apps and pasted keys, `lss_…` for the site's
//   session cookie) and one-time codes are kept as SHA-256 hashes.
// - `/api/ai/v1/messages` speaks the Anthropic Messages API. With `LSUITE_ANTHROPIC_API_KEY` it
//   forwards to Anthropic (streaming passed through as it arrives) and counts the usage the response
//   reports into credits; without it, it answers with a short demo message in the same format.
// - This is a demo: signing in takes an email and a name (no password, no email check) and checkout
//   takes no payment. Both say so wherever a person sees them.
import { createHash, randomBytes } from 'node:crypto';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

/** One credit is this much model usage, in US dollars at the provider's list price. */
export const CREDIT_USD = 0.005;

/**
 * Every model lsuite AI can serve, with list prices in dollars per million tokens. Cache writes cost
 * 1.25× input; cache reads have their own price. Dated ids (`claude-haiku-4-5-20251001`) resolve to
 * the id they start with.
 */
export const MODELS = [
  { id: 'claude-haiku-4-5', name: 'Claude Haiku 4.5', family: 'haiku', input: 1, output: 5, cacheRead: 0.1, created: '2025-10-15' },
  { id: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6', family: 'sonnet', input: 3, output: 15, cacheRead: 0.3, created: '2026-02-17' },
  { id: 'claude-sonnet-5', name: 'Claude Sonnet 5', family: 'sonnet', input: 2, output: 10, cacheRead: 0.2, created: '2026-06-01' },
  { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', family: 'sonnet', input: 2, output: 10, cacheRead: 0.2, created: '2026-09-01' },
  { id: 'claude-opus-4-8', name: 'Claude Opus 4.8', family: 'opus', input: 5, output: 25, cacheRead: 0.5, created: '2026-05-01' },
  { id: 'claude-opus-5', name: 'Claude Opus 5', family: 'opus', input: 5, output: 25, cacheRead: 0.5, created: '2026-07-01' },
  { id: 'claude-opus-5-5', name: 'Claude Opus 5.5', family: 'opus', input: 4, output: 20, cacheRead: 0.2, created: '2026-09-01' },
  { id: 'claude-fable-5', name: 'Claude Fable 5', family: 'fable', input: 10, output: 50, cacheRead: 1, created: '2026-06-01' },
  { id: 'claude-fable-5-1', name: 'Claude Fable 5.1', family: 'fable', input: 10, output: 50, cacheRead: 0.25, created: '2026-09-01' },
];

/** The plans of AI.md. Prices are placeholders the owner will set; nothing is charged (demo). */
export const PLANS = [
  { id: 'free', name: 'Free', price: 0, credits: 0, families: [], defaultModel: null, priority: false, summary: 'Bring your own provider: Claude Code, Codex, API keys or a local model.' },
  { id: 'plus', name: 'Plus', price: 12, credits: 1000, families: ['sonnet', 'haiku'], defaultModel: 'claude-sonnet-5-5', priority: false, summary: 'Claude Sonnet and Claude Haiku in every app, no setup.' },
  { id: 'pro', name: 'Pro', price: 29, credits: 4000, families: ['sonnet', 'haiku', 'opus'], defaultModel: 'claude-opus-5-5', priority: false, summary: 'Adds Claude Opus, for long agent runs and hard edits.' },
  { id: 'studio', name: 'Studio', price: 79, credits: 12000, families: ['sonnet', 'haiku', 'opus', 'fable'], defaultModel: 'claude-opus-5-5', priority: true, summary: 'Every model, the most allowance, priority.' },
];
const PLAN = Object.fromEntries(PLANS.map((p) => [p.id, p]));
const FAMILY_NAMES = { haiku: 'Claude Haiku', sonnet: 'Claude Sonnet', opus: 'Claude Opus', fable: 'Claude Fable' };

/** The apps that may ask for a sign-in (`/account/connect?app=`). */
export const APPS = { ryolune: 'ryolune', kimchi: 'kimchi', zenith: 'zenith', nori: 'nori', folio: 'folio' };

export const modelsOf = (plan) => MODELS.filter((m) => PLAN[plan]?.families.includes(m.family));

/** The catalogue entry for a model id, dated ids included, or null. */
export function resolveModel(id) {
  if (typeof id !== 'string' || !id) return null;
  const exact = MODELS.find((m) => m.id === id);
  if (exact) return exact;
  // `claude-opus-5-5-20260901` → claude-opus-5-5 (the longest id followed by a date).
  return MODELS.filter((m) => new RegExp(`^${m.id}-\\d{8}$`).test(id)).sort((a, b) => b.id.length - a.id.length)[0] ?? null;
}

/** Credits for one response's `usage` on a model. */
export function creditsFor(model, usage = {}) {
  const n = (v) => (Number.isFinite(v) && v > 0 ? v : 0);
  const usd =
    (n(usage.input_tokens) * model.input +
      n(usage.cache_creation_input_tokens) * model.input * 1.25 +
      n(usage.cache_read_input_tokens) * model.cacheRead +
      n(usage.output_tokens) * model.output) /
    1e6;
  return usd / CREDIT_USD;
}

/** `GET /api/ai/plans`: what the apps and the site show. */
export function plansDocument() {
  return {
    demo: true,
    currency: 'USD',
    credit: { usd: CREDIT_USD, description: 'A credit is half a US cent of model usage at the provider’s list price: input, output and cached tokens weighted by the model’s price.' },
    plans: PLANS.map((p) => ({
      id: p.id,
      name: p.name,
      price: p.price,
      priceLabel: p.price ? `$${p.price} / month` : 'Free',
      interval: 'month',
      credits: p.credits,
      models: modelsOf(p.id).map((m) => m.id),
      families: p.families.map((f) => FAMILY_NAMES[f]),
      defaultModel: p.defaultModel,
      priority: p.priority,
      summary: p.summary,
    })),
    models: MODELS.map(({ id, name, family, input, output }) => ({ id, name, family, price: { input, output } })),
  };
}

const sha = (s) => createHash('sha256').update(String(s)).digest('hex');
const secret = (prefix) => prefix + randomBytes(32).toString('base64url');
const now = () => Date.now();
const periodOf = (t) => new Date(t).toISOString().slice(0, 7);
/** The first instant of the next calendar month (UTC): when the allowance resets. */
const resetOf = (t) => {
  const d = new Date(t);
  return new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 1)).toISOString();
};

const CODE_TTL = 5 * 60 * 1000;
const SESSION_TTL = 30 * 24 * 60 * 60 * 1000;
const EMAIL = /^[^\s@<>()",;:]{1,64}@[^\s@<>()",;:]{1,190}\.[a-z0-9-]{2,24}$/i;

/** The account file: users, hashed tokens and codes. */
class Store {
  constructor(dir) {
    this.file = dir ? join(dir, 'accounts.json') : null;
    this.data = { format: 1, users: {}, tokens: {}, codes: {} };
    this.writing = Promise.resolve();
    this.dirty = false;
  }

  async load() {
    if (!this.file) return;
    await mkdir(join(this.file, '..'), { recursive: true });
    try {
      const data = JSON.parse(await readFile(this.file, 'utf8'));
      if (data?.format === 1) this.data = { format: 1, users: data.users ?? {}, tokens: data.tokens ?? {}, codes: data.codes ?? {} };
    } catch (err) {
      if (err.code !== 'ENOENT') throw err;
    }
  }

  /** Writes the file soon, once for any number of changes in between; resolves when written. */
  save() {
    if (!this.file) return Promise.resolve();
    this.dirty = true;
    this.writing = this.writing.then(async () => {
      if (!this.dirty) return;
      this.dirty = false;
      const tmp = `${this.file}.${process.pid}.tmp`;
      await writeFile(tmp, JSON.stringify(this.data), { mode: 0o600 });
      await rename(tmp, this.file);
    });
    return this.writing.catch((err) => console.error('lsuite accounts: could not save', err.message));
  }

  userByEmail(email) {
    return Object.values(this.data.users).find((u) => u.email === email) ?? null;
  }

  /** The user a token belongs to, if the token is live and of one of `kinds`. */
  tokenUser(token, kinds) {
    if (typeof token !== 'string' || token.length < 20 || token.length > 200) return null;
    const entry = this.data.tokens[sha(token)];
    if (!entry || !kinds.includes(entry.kind)) return null;
    if (entry.expiresAt && entry.expiresAt < now()) {
      delete this.data.tokens[sha(token)];
      return null;
    }
    const user = this.data.users[entry.userId];
    return user ? { user, entry, hash: sha(token) } : null;
  }

  issue(userId, kind, extra = {}) {
    const token = secret(kind === 'session' ? 'lss_' : 'lsk_');
    this.data.tokens[sha(token)] = { userId, kind, createdAt: now(), ...extra };
    return token;
  }

  /** Drops expired codes and sessions. */
  sweep() {
    const t = now();
    for (const [k, c] of Object.entries(this.data.codes)) if (c.expiresAt < t) delete this.data.codes[k];
    for (const [k, e] of Object.entries(this.data.tokens)) if (e.expiresAt && e.expiresAt < t) delete this.data.tokens[k];
  }
}

/** Per-IP fixed windows: `hit(key)` is false once `max` hits happened in the last `windowMs`. */
function limiter(max, windowMs) {
  const hits = new Map();
  return {
    hit(key) {
      const t = now();
      if (hits.size > 10000) hits.clear();
      const recent = (hits.get(key) ?? []).filter((at) => t - at < windowMs);
      if (recent.length >= max) {
        hits.set(key, recent);
        return Math.ceil((windowMs - (t - recent[0])) / 1000);
      }
      recent.push(t);
      hits.set(key, recent);
      return 0;
    },
  };
}

const API_HEADERS = { 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff', 'Referrer-Policy': 'no-referrer' };

function json(res, status, body, headers = {}) {
  const text = JSON.stringify(body);
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', 'Content-Length': Buffer.byteLength(text), ...API_HEADERS, ...headers });
  res.end(text);
}

/** An error in the Anthropic API's shape (`{type: "error", error: {type, message}}`), plus lsuite's fields. */
function fail(res, status, type, message, extra = {}, headers = {}) {
  json(res, status, { type: 'error', error: { type, message, ...extra } }, headers);
}

class HttpError extends Error {
  constructor(status, type, message) {
    super(message);
    this.status = status;
    this.type = type;
  }
}

async function readBody(req, limit) {
  const chunks = [];
  let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    if (size > limit) throw new HttpError(413, 'request_too_large', `The request is larger than ${Math.round(limit / 1e6)} MB.`);
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

async function readJson(req, limit = 16 * 1024) {
  if (!/^application\/json\b/i.test(String(req.headers['content-type'] ?? ''))) {
    throw new HttpError(415, 'invalid_request_error', 'Send JSON (Content-Type: application/json).');
  }
  const raw = await readBody(req, limit);
  try {
    const value = JSON.parse(raw.toString('utf8') || '{}');
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error();
    return { value, raw };
  } catch {
    throw new HttpError(400, 'invalid_request_error', 'The body is not a JSON object.');
  }
}

function cookie(req, name) {
  for (const part of String(req.headers.cookie ?? '').split(';')) {
    const [k, ...v] = part.trim().split('=');
    if (k === name) return decodeURIComponent(v.join('='));
  }
  return null;
}

/** The caller's address: the last hop the platform's proxy added, else the socket's. */
export function clientIp(req) {
  const forwarded = String(req.headers['x-forwarded-for'] ?? '').split(',').map((s) => s.trim()).filter(Boolean);
  return forwarded.at(-1) || req.socket?.remoteAddress || 'unknown';
}

/** The bearer token of a request: `x-api-key`, or `Authorization: Bearer`. */
function bearer(req) {
  const key = req.headers['x-api-key'];
  if (typeof key === 'string' && key) return key.trim();
  const m = /^Bearer\s+(\S+)$/i.exec(String(req.headers.authorization ?? ''));
  return m ? m[1] : null;
}

const originOf = (req) => {
  const proto = String(req.headers['x-forwarded-proto'] ?? 'http').split(',')[0].trim();
  return `${proto}://${String(req.headers.host ?? 'localhost').replace(/[^\w.:-]/g, '')}`;
};

/** A small streaming parser for server-sent events: calls `onEvent(name, data)` per event. */
function sseParser(onEvent) {
  const decoder = new TextDecoder();
  let buffer = '';
  const flush = (block) => {
    let event = 'message';
    const data = [];
    for (const line of block.split(/\r?\n/)) {
      if (line.startsWith('event:')) event = line.slice(6).trim();
      else if (line.startsWith('data:')) data.push(line.slice(5).replace(/^ /, ''));
    }
    if (data.length) onEvent(event, data.join('\n'));
  };
  return {
    push(chunk) {
      buffer += decoder.decode(chunk, { stream: true });
      let i;
      while ((i = buffer.search(/\r?\n\r?\n/)) >= 0) {
        const block = buffer.slice(0, i);
        buffer = buffer.slice(i).replace(/^\r?\n\r?\n/, '');
        flush(block);
      }
    },
    end() {
      if (buffer.trim()) flush(buffer);
      buffer = '';
    },
  };
}

/** Takes the largest value of each usage field seen across a stream's events. */
function mergeUsage(into, usage) {
  if (!usage || typeof usage !== 'object') return into;
  for (const k of ['input_tokens', 'output_tokens', 'cache_creation_input_tokens', 'cache_read_input_tokens']) {
    if (Number.isFinite(usage[k])) into[k] = Math.max(into[k] ?? 0, usage[k]);
  }
  return into;
}

const sse = (event, data) => `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const estimateTokens = (value) => Math.max(1, Math.ceil(JSON.stringify(value ?? '').length / 4));

/**
 * The accounts service. Options (all optional, for tests and the host):
 * `dataDir`, `anthropicKey`, `upstream` (Anthropic's base URL), `fetch`, `demoDelayMs`,
 * `limits: {signin: [max, windowMs], token: [max, windowMs]}`.
 */
export function createAccounts(options = {}) {
  const store = new Store(options.dataDir ?? null);
  const ready = store.load();
  const anthropicKey = options.anthropicKey ?? '';
  const upstream = (options.upstream ?? 'https://api.anthropic.com').replace(/\/+$/, '');
  const fetchImpl = options.fetch ?? fetch;
  const demoDelayMs = options.demoDelayMs ?? 18;
  const limits = {
    signin: limiter(...(options.limits?.signin ?? [20, 10 * 60 * 1000])),
    token: limiter(...(options.limits?.token ?? [30, 10 * 60 * 1000])),
  };

  /** Rolls a user's allowance into the current month. */
  function roll(user) {
    const period = periodOf(now());
    if (user.usage?.period !== period) user.usage = { period, used: 0 };
    return user.usage;
  }

  /** The account as AI.md describes it (`GET /api/account/me`). */
  function account(user, origin) {
    const plan = PLAN[user.plan] ?? PLAN.free;
    const usage = roll(user);
    return {
      email: user.email,
      name: user.name,
      plan: plan.id,
      planName: plan.name,
      status: plan.id === 'free' ? 'none' : 'active',
      demo: true,
      usage: {
        used: Math.round(usage.used * 10) / 10,
        limit: plan.credits,
        percent: plan.credits ? Math.min(100, Math.round((usage.used / plan.credits) * 100)) : 0,
        resetsAt: resetOf(now()),
      },
      models: modelsOf(plan.id).map((m) => m.id),
      defaultModel: plan.defaultModel,
      manageUrl: `${origin}/account`,
    };
  }

  function rateLimited(res, which, req) {
    const wait = limits[which].hit(clientIp(req));
    if (!wait) return false;
    fail(res, 429, 'rate_limit_error', `Too many attempts from this address. Try again in ${wait} s.`, {}, { 'Retry-After': String(wait) });
    return true;
  }

  /** The site's session (cookie), or null. */
  function session(req) {
    return store.tokenUser(cookie(req, 'lsuite_session'), ['session']);
  }

  /** An app's token (`x-api-key` or Bearer), or null. */
  function appAuth(req) {
    return store.tokenUser(bearer(req), ['app', 'key']);
  }

  /** Cookie-authenticated writes come from this site only (SameSite=Lax does most of it; JSON-only does the rest). */
  function sameOrigin(req) {
    const origin = req.headers.origin;
    if (!origin) return true;
    try {
      return new URL(origin).host === String(req.headers.host ?? '');
    } catch {
      return false;
    }
  }

  function sessionCookie(req, value, maxAge) {
    const secure = String(req.headers['x-forwarded-proto'] ?? '').startsWith('https') ? '; Secure' : '';
    return `lsuite_session=${value}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${maxAge}${secure}`;
  }

  function connections(user) {
    return Object.entries(store.data.tokens)
      .filter(([, e]) => e.userId === user.id && (e.kind === 'app' || e.kind === 'key'))
      .map(([hash, e]) => ({ id: hash.slice(0, 16), kind: e.kind, app: e.app ?? null, createdAt: new Date(e.createdAt).toISOString(), lastUsedAt: e.lastUsedAt ? new Date(e.lastUsedAt).toISOString() : null }))
      .sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  }

  /** Checks plan, model and allowance for a model request; answers and returns null when refused. */
  function admit(res, user, modelId, origin) {
    const plan = PLAN[user.plan] ?? PLAN.free;
    const manage = { manage_url: `${origin}/account`, plan: plan.id };
    if (plan.id === 'free') {
      fail(res, 403, 'plan_required', `lsuite AI needs a plan: your account is on Free (bring your own provider). Pick a plan at ${origin}/account (demo: no payment is taken).`, manage);
      return null;
    }
    const model = resolveModel(modelId);
    if (!model) {
      fail(res, 404, 'not_found_error', `lsuite AI has no model called ${JSON.stringify(String(modelId ?? ''))}. Models: ${modelsOf(plan.id).map((m) => m.id).join(', ')}.`);
      return null;
    }
    if (!plan.families.includes(model.family)) {
      fail(res, 403, 'model_not_in_plan', `${model.name} isn't in the ${plan.name} plan (${plan.families.map((f) => FAMILY_NAMES[f]).join(', ')}). Choose another model, or change plan at ${origin}/account.`, { ...manage, model: model.id });
      return null;
    }
    const usage = roll(user);
    if (usage.used >= plan.credits) {
      const resets = resetOf(now());
      const day = new Date(resets).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', timeZone: 'UTC' });
      fail(res, 402, 'allowance_exhausted', `Your lsuite AI allowance for this month is used up (${plan.name}, ${plan.credits.toLocaleString('en-US')} credits). It resets on ${day}. Manage plan: ${origin}/account`, { ...manage, resets_at: resets, used: Math.round(usage.used * 10) / 10, limit: plan.credits });
      return null;
    }
    return model;
  }

  function charge(user, model, usage) {
    const credits = creditsFor(model, usage);
    if (credits > 0) {
      roll(user).used += credits;
      store.save();
    }
    return credits;
  }

  /** The demo answer, in the Anthropic format (streamed or not), charged like a real one. */
  async function demoReply(req, res, user, model, body) {
    const text = `This is the lsuite AI demo. No model is connected to this server yet, so this answer is canned: your account (${PLAN[user.plan].name}), your allowance and the connection from the app all work. When lsuite AI goes live, ${model.name} answers here.`;
    const usage = { input_tokens: estimateTokens([body.system, body.messages, body.tools]), cache_creation_input_tokens: 0, cache_read_input_tokens: 0, output_tokens: estimateTokens(text) };
    const message = { id: `msg_demo_${randomBytes(12).toString('hex')}`, type: 'message', role: 'assistant', model: body.model, content: [], stop_reason: null, stop_sequence: null, usage: { ...usage, output_tokens: 1 } };
    if (!body.stream) {
      charge(user, model, usage);
      return json(res, 200, { ...message, content: [{ type: 'text', text }], stop_reason: 'end_turn', usage });
    }
    res.writeHead(200, { 'Content-Type': 'text/event-stream; charset=utf-8', Connection: 'keep-alive', ...API_HEADERS });
    let closed = false;
    res.on('close', () => (closed = true));
    res.write(sse('message_start', { type: 'message_start', message }));
    res.write(sse('content_block_start', { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }));
    res.write(sse('ping', { type: 'ping' }));
    for (const piece of text.match(/\S+\s*/g)) {
      if (closed) break;
      res.write(sse('content_block_delta', { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: piece } }));
      if (demoDelayMs) await sleep(demoDelayMs);
    }
    charge(user, model, usage);
    if (closed) return;
    res.write(sse('content_block_stop', { type: 'content_block_stop', index: 0 }));
    res.write(sse('message_delta', { type: 'message_delta', delta: { stop_reason: 'end_turn', stop_sequence: null }, usage: { output_tokens: usage.output_tokens } }));
    res.write(sse('message_stop', { type: 'message_stop' }));
    res.end();
  }

  /** Forwards to Anthropic with the server's key; streams through and counts the usage. */
  async function forward(req, res, user, model, raw, path) {
    const abort = new AbortController();
    res.on('close', () => abort.abort());
    const headers = { 'content-type': 'application/json', 'x-api-key': anthropicKey, 'anthropic-version': String(req.headers['anthropic-version'] || '2023-06-01') };
    if (req.headers['anthropic-beta']) headers['anthropic-beta'] = String(req.headers['anthropic-beta']);
    let up;
    try {
      up = await fetchImpl(`${upstream}${path}`, { method: 'POST', headers, body: raw, signal: abort.signal });
    } catch (err) {
      if (abort.signal.aborted) return;
      return fail(res, 502, 'api_error', `lsuite AI could not reach the model provider (${err.message}). Try again.`);
    }
    const type = up.headers.get('content-type') ?? 'application/json';
    const passed = { 'Content-Type': type, ...API_HEADERS };
    for (const h of ['request-id', 'retry-after']) if (up.headers.get(h)) passed[h] = up.headers.get(h);
    const counts = path === '/v1/messages';
    if (/text\/event-stream/i.test(type) && up.body) {
      res.writeHead(up.status, passed);
      const usage = {};
      const parser = sseParser((event, data) => {
        if (event !== 'message_start' && event !== 'message_delta') return;
        try {
          const value = JSON.parse(data);
          mergeUsage(usage, value.message?.usage ?? value.usage);
        } catch {}
      });
      try {
        for await (const chunk of up.body) {
          parser.push(chunk);
          res.write(chunk);
        }
        parser.end();
      } catch {
        // The app went away, or the provider dropped the stream: count what was reported.
      } finally {
        if (counts) charge(user, model, usage);
        if (!res.writableEnded) res.end();
      }
      return;
    }
    const text = Buffer.from(await up.arrayBuffer());
    if (counts && up.ok) {
      try {
        charge(user, model, JSON.parse(text.toString('utf8')).usage);
      } catch {}
    }
    res.writeHead(up.status, { ...passed, 'Content-Length': text.length });
    res.end(text);
  }

  async function messages(req, res, origin, path) {
    const auth = appAuth(req);
    if (!auth) return fail(res, 401, 'authentication_error', `Sign in to lsuite AI again: this key isn't valid (signed out or revoked). ${origin}/account`);
    auth.entry.lastUsedAt = now();
    const { value: body, raw } = await readJson(req, 32 * 1024 * 1024);
    const model = admit(res, auth.user, body.model, origin);
    if (!model) return;
    if (path === '/v1/messages/count_tokens') {
      if (anthropicKey) return forward(req, res, auth.user, model, raw, path);
      return json(res, 200, { input_tokens: estimateTokens([body.system, body.messages, body.tools]) });
    }
    if (!Array.isArray(body.messages) || !body.messages.length) return fail(res, 400, 'invalid_request_error', 'messages: at least one message is required.');
    if (!Number.isInteger(body.max_tokens) || body.max_tokens < 1) return fail(res, 400, 'invalid_request_error', 'max_tokens: a positive integer is required.');
    if (anthropicKey) return forward(req, res, auth.user, model, raw, path);
    return demoReply(req, res, auth.user, model, body);
  }

  function modelList(user) {
    return modelsOf(user.plan).map((m) => ({ type: 'model', id: m.id, display_name: m.name, created_at: `${m.created}T00:00:00Z` }));
  }

  /** Handles `/api/…`; returns false for any other path. */
  async function handle(req, res, url) {
    const path = url.pathname;
    if (!path.startsWith('/api/')) return false;
    await ready;
    const origin = originOf(req);
    const method = req.method;
    try {
      if (path === '/api/ai/plans' && method === 'GET') return json(res, 200, plansDocument(), { 'Cache-Control': 'public, max-age=300' }), true;

      // ---- The Anthropic-compatible endpoint (apps, Claude Code with ANTHROPIC_BASE_URL) ----
      if ((path === '/api/ai/v1/messages' || path === '/api/ai/v1/messages/count_tokens') && method === 'POST') {
        await messages(req, res, origin, path.slice('/api/ai'.length));
        return true;
      }
      if (path === '/api/ai/v1/models' || path.startsWith('/api/ai/v1/models/')) {
        if (method !== 'GET') return fail(res, 405, 'invalid_request_error', 'Use GET.'), true;
        const auth = appAuth(req);
        if (!auth) return fail(res, 401, 'authentication_error', `Sign in to lsuite AI again: this key isn't valid. ${origin}/account`), true;
        const list = modelList(auth.user);
        const id = decodeURIComponent(path.slice('/api/ai/v1/models/'.length));
        if (path !== '/api/ai/v1/models') {
          const found = list.find((m) => m.id === resolveModel(id)?.id);
          return (found ? json(res, 200, found) : fail(res, 404, 'not_found_error', `No model ${JSON.stringify(id)} in your plan.`)), true;
        }
        return json(res, 200, { data: list, has_more: false, first_id: list[0]?.id ?? null, last_id: list.at(-1)?.id ?? null }), true;
      }

      // ---- Accounts: apps (token) ----
      if (path === '/api/account/me' && method === 'GET') {
        const auth = appAuth(req) ?? session(req);
        if (!auth) return fail(res, 401, 'authentication_error', 'Not signed in.'), true;
        const body = account(auth.user, origin);
        if (auth.entry.kind === 'session') body.connections = connections(auth.user);
        return json(res, 200, body), true;
      }
      if (path === '/api/account/token' && method === 'POST') {
        if (rateLimited(res, 'token', req)) return true;
        const { value } = await readJson(req);
        store.sweep();
        const hash = typeof value.code === 'string' ? sha(value.code) : null;
        const code = hash && store.data.codes[hash];
        if (hash) delete store.data.codes[hash];
        const user = code && code.expiresAt >= now() ? store.data.users[code.userId] : null;
        if (!user) {
          store.save();
          return fail(res, 400, 'invalid_grant', 'This sign-in code is unknown, already used or expired (codes last 5 minutes). Sign in again from the app.'), true;
        }
        const token = store.issue(user.id, 'app', { app: code.app });
        await store.save();
        return json(res, 200, { token, account: account(user, origin) }), true;
      }
      if (path === '/api/account/signout' && method === 'POST') {
        const app = appAuth(req);
        if (app) {
          delete store.data.tokens[app.hash];
          await store.save();
          return json(res, 200, { ok: true }), true;
        }
        if (!sameOrigin(req)) return fail(res, 403, 'permission_error', 'Cross-site request refused.'), true;
        const site = session(req);
        if (site) {
          delete store.data.tokens[site.hash];
          await store.save();
        }
        return json(res, 200, { ok: true }, { 'Set-Cookie': sessionCookie(req, '', 0) }), true;
      }

      // ---- Accounts: the site (session cookie, same origin, JSON only) ----
      if (path.startsWith('/api/account/') && method === 'POST') {
        if (!sameOrigin(req)) return fail(res, 403, 'permission_error', 'Cross-site request refused.'), true;
        if (path === '/api/account/session') {
          if (rateLimited(res, 'signin', req)) return true;
          const { value } = await readJson(req);
          const email = String(value.email ?? '').trim().toLowerCase();
          const name = String(value.name ?? '').trim().replace(/\s+/g, ' ');
          if (!EMAIL.test(email) || email.length > 254) return fail(res, 400, 'invalid_request_error', 'Enter a valid email address.'), true;
          let user = store.userByEmail(email);
          const created = !user;
          if (!user) {
            if (!name || name.length > 80) return fail(res, 400, 'invalid_request_error', 'Enter your name (80 characters at most) to create the account.'), true;
            user = { id: `u_${randomBytes(9).toString('base64url')}`, email, name, plan: 'free', createdAt: now(), usage: { period: periodOf(now()), used: 0 } };
            store.data.users[user.id] = user;
          } else if (name && name.length <= 80) user.name = name;
          store.sweep();
          const token = store.issue(user.id, 'session', { expiresAt: now() + SESSION_TTL });
          await store.save();
          return json(res, 200, { account: account(user, origin), created }, { 'Set-Cookie': sessionCookie(req, token, SESSION_TTL / 1000) }), true;
        }
        const auth = session(req);
        if (!auth) return fail(res, 401, 'authentication_error', 'Not signed in.'), true;
        const { user } = auth;
        const { value } = await readJson(req);
        if (path === '/api/account/checkout') {
          if (!PLAN[value.plan]) return fail(res, 400, 'invalid_request_error', 'Unknown plan.'), true;
          // Demo: no payment is taken; the plan starts at once with a fresh allowance.
          if (user.plan !== value.plan) user.usage = { period: periodOf(now()), used: 0 };
          user.plan = value.plan;
          await store.save();
          return json(res, 200, { account: account(user, origin), demo: true, charged: 0 }), true;
        }
        if (path === '/api/account/connect') {
          const app = APPS[value.app];
          if (!app) return fail(res, 400, 'invalid_request_error', 'Unknown app.'), true;
          store.sweep();
          const code = secret('lsc_');
          store.data.codes[sha(code)] = { userId: user.id, app, expiresAt: now() + CODE_TTL };
          await store.save();
          return json(res, 200, { code, expiresIn: CODE_TTL / 1000 }), true;
        }
        if (path === '/api/account/key') {
          // One paste key at a time: a new one replaces the last.
          for (const [k, e] of Object.entries(store.data.tokens)) if (e.userId === user.id && e.kind === 'key') delete store.data.tokens[k];
          const key = store.issue(user.id, 'key', { app: null });
          await store.save();
          return json(res, 200, { key }), true;
        }
        if (path === '/api/account/disconnect') {
          const id = String(value.id ?? '');
          const hit = Object.entries(store.data.tokens).find(([hash, e]) => e.userId === user.id && hash.slice(0, 16) === id && e.kind !== 'session');
          if (!hit) return fail(res, 404, 'not_found_error', 'No such connection.'), true;
          delete store.data.tokens[hit[0]];
          await store.save();
          return json(res, 200, { connections: connections(user) }), true;
        }
      }
      fail(res, method === 'GET' || method === 'POST' ? 404 : 405, 'not_found_error', `No API at ${method} ${path}.`);
      return true;
    } catch (err) {
      if (res.headersSent) {
        res.destroy();
        return true;
      }
      if (err instanceof HttpError) return fail(res, err.status, err.type, err.message), true;
      throw err;
    }
  }

  return { handle, store, ready };
}
