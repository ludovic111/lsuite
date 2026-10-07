// lsuite accounts and lsuite AI (AI.md): every route, the SSE format, auth, allowance and codes.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, readFile, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createAccounts, creditsFor, resolveModel, MODELS, PLANS } from '../ai.js';
import { handle } from '../server.js';

/** A running accounts service on a free port, with a cookie jar for the site's calls. */
async function start(options = {}) {
  const accounts = createAccounts({ demoDelayMs: 0, allowDemoUpstream: !!options.anthropicKey, ...options });
  const server = createServer(async (req, res) => {
    if (!(await accounts.handle(req, res, new URL(req.url, 'http://localhost')))) {
      res.writeHead(404);
      res.end();
    }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
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
  const api = (path, token, body, headers = {}) =>
    fetch(base + path, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...(token ? { 'x-api-key': token } : {}), ...headers },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  return { accounts, base, site, api, close: () => new Promise((r) => server.close(r)), get jar() { return jar; } };
}

/** Signs in on the site, optionally picks a plan, and connects an app: returns its token. */
async function connected(t, { plan = 'plus', email = 'ada@example.com', name = 'Ada' } = {}) {
  assert.equal((await t.site('/api/account/session', { email, name })).status, 200);
  if (plan) assert.equal((await t.site('/api/account/checkout', { plan })).status, 200);
  const { code } = await (await t.site('/api/account/connect', { app: 'kimchi' })).json();
  const res = await t.api('/api/account/token', null, { code });
  assert.equal(res.status, 200);
  return (await res.json()).token;
}

/** Parses an SSE body into `[{event, data}]`. */
function events(text) {
  return text
    .split('\n\n')
    .filter((b) => b.trim())
    .map((block) => {
      const event = /^event: (.+)$/m.exec(block)[1];
      const data = JSON.parse(/^data: (.+)$/m.exec(block)[1]);
      return { event, data };
    });
}

const ask = (model, extra = {}) => ({ model, max_tokens: 256, messages: [{ role: 'user', content: 'Cut the intro to four seconds.' }], ...extra });

test('configuring a key in demo mode never grants access to a paid provider', async () => {
  const t = await start({ anthropicKey: 'unused-key', allowDemoUpstream: false, fetch: () => { throw new Error('Demo contacted the real provider'); } });
  try {
    const token = await connected(t);
    const response = await t.api('/api/ai/v1/messages', token, ask('claude-haiku-4-5'));
    assert.equal(response.status, 200);
    assert.match((await response.json()).content[0].text, /lsuite AI demo/);
  } finally { await t.close(); }
});

test('plans: the four of AI.md, demo, models per plan', async () => {
  const t = await start();
  try {
    const res = await t.api('/api/ai/plans');
    assert.equal(res.status, 200);
    const doc = await res.json();
    assert.equal(doc.demo, true);
    assert.deepEqual(doc.plans.map((p) => [p.id, p.price, p.credits]), [['free', 0, 0], ['plus', 12, 1000], ['pro', 29, 4000], ['studio', 79, 12000]]);
    const plus = doc.plans.find((p) => p.id === 'plus');
    assert.ok(plus.models.every((id) => /sonnet|haiku/.test(id)));
    assert.ok(doc.plans.find((p) => p.id === 'pro').models.some((id) => /opus/.test(id)));
    assert.equal(doc.plans.find((p) => p.id === 'studio').models.length, MODELS.length);
    assert.deepEqual(doc.plans[0].models, []);
  } finally {
    await t.close();
  }
});

test('the site: sign in with a session cookie, same origin and JSON only', async () => {
  const t = await start();
  try {
    assert.equal((await t.site('/api/account/me')).status, 401);
    assert.equal((await t.site('/api/account/session', { email: 'not-an-email', name: 'Ada' })).status, 400);
    assert.equal((await t.site('/api/account/session', { email: 'ada@example.com' })).status, 400, 'a new account needs a name');
    const res = await t.site('/api/account/session', { email: 'Ada@Example.com', name: 'Ada' });
    assert.equal(res.status, 200);
    const cookie = res.headers.get('set-cookie');
    assert.match(cookie, /^lsuite_session=lss_[\w-]+;/);
    assert.match(cookie, /HttpOnly/);
    assert.match(cookie, /SameSite=Lax/);
    const body = await res.json();
    assert.equal(body.created, true);
    assert.equal(body.account.email, 'ada@example.com');
    assert.equal(body.account.plan, 'free');
    const me = await (await t.site('/api/account/me')).json();
    assert.equal(me.name, 'Ada');
    assert.deepEqual(me.connections, []);
    // Signing in again finds the same account.
    assert.equal((await (await t.site('/api/account/session', { email: 'ada@example.com' })).json()).created, false);
    // Another site can't use the cookie, and forms (not JSON) are refused.
    assert.equal((await t.site('/api/account/checkout', { plan: 'pro' }, { origin: 'https://evil.example' })).status, 403);
    const form = await fetch(`${t.base}/api/account/checkout`, { method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded', cookie: t.jar }, body: 'plan=pro' });
    assert.equal(form.status, 415);
    // Demo checkout: no payment, the plan starts at once.
    const checkout = await (await t.site('/api/account/checkout', { plan: 'pro' })).json();
    assert.equal(checkout.demo, true);
    assert.equal(checkout.charged, 0);
    assert.equal(checkout.account.plan, 'pro');
    assert.equal(checkout.account.usage.limit, 4000);
    // Signing out clears the cookie.
    const out = await t.site('/api/account/signout', {});
    assert.match(out.headers.get('set-cookie'), /Max-Age=0/);
    assert.equal((await t.site('/api/account/me')).status, 401);
  } finally {
    await t.close();
  }
});

test('loopback sign-in: a code is good once, for five minutes, then a token works by x-api-key and Bearer', async () => {
  const t = await start();
  try {
    assert.equal((await t.site('/api/account/connect', { app: 'kimchi' })).status, 401, 'needs a session');
    await t.site('/api/account/session', { email: 'ada@example.com', name: 'Ada' });
    assert.equal((await t.site('/api/account/connect', { app: 'photoshop' })).status, 400);
    const { code, expiresIn } = await (await t.site('/api/account/connect', { app: 'nori' })).json();
    assert.match(code, /^lsc_/);
    assert.equal(expiresIn, 300);
    const first = await t.api('/api/account/token', null, { code });
    assert.equal(first.status, 200);
    const { token, account } = await first.json();
    assert.match(token, /^lsk_[\w-]{40,}$/);
    assert.equal(account.email, 'ada@example.com');
    // Used once: the same code is refused.
    const again = await t.api('/api/account/token', null, { code });
    assert.equal(again.status, 400);
    assert.equal((await again.json()).error.type, 'invalid_grant');
    // An expired code is refused.
    const { code: late } = await (await t.site('/api/account/connect', { app: 'kimchi' })).json();
    for (const c of Object.values(t.accounts.store.data.codes)) c.expiresAt = Date.now() - 1;
    assert.equal((await t.api('/api/account/token', null, { code: late })).status, 400);
    assert.equal((await t.api('/api/account/token', null, { code: 'lsc_made_up_code_000000000000' })).status, 400);
    // The token, both ways.
    const byKey = await t.api('/api/account/me', token);
    assert.equal(byKey.status, 200);
    assert.equal((await byKey.json()).plan, 'free');
    const byBearer = await fetch(`${t.base}/api/account/me`, { headers: { authorization: `Bearer ${token}` } });
    assert.equal(byBearer.status, 200);
    const me = await byBearer.json();
    assert.deepEqual(Object.keys(me.usage).sort(), ['limit', 'percent', 'resetsAt', 'used']);
    assert.equal(me.connections, undefined, 'apps don\'t list the other connections');
    // The site lists the connection; signing the app out revokes its token.
    assert.equal((await (await t.site('/api/account/me')).json()).connections[0].app, 'nori');
    assert.equal((await fetch(`${t.base}/api/account/signout`, { method: 'POST', headers: { authorization: `Bearer ${token}` } })).status, 200);
    assert.equal((await t.api('/api/account/me', token)).status, 401);
    assert.equal((await t.api('/api/account/me', 'lsk_nope_nope_nope_nope_nope_nope')).status, 401);
  } finally {
    await t.close();
  }
});

test('the paste key replaces the last one and works like an app token', async () => {
  const t = await start();
  try {
    await t.site('/api/account/session', { email: 'ada@example.com', name: 'Ada' });
    const { key: one } = await (await t.site('/api/account/key', {})).json();
    const { key: two } = await (await t.site('/api/account/key', {})).json();
    assert.match(two, /^lsk_/);
    assert.equal((await t.api('/api/account/me', one)).status, 401);
    assert.equal((await t.api('/api/account/me', two)).status, 200);
    const { connections } = await (await t.site('/api/account/me')).json();
    assert.equal(connections.length, 1);
    const after = await (await t.site('/api/account/disconnect', { id: connections[0].id })).json();
    assert.deepEqual(after.connections, []);
    assert.equal((await t.api('/api/account/me', two)).status, 401);
  } finally {
    await t.close();
  }
});

test('messages: no plan, a model outside the plan, unknown models and bad keys get clear errors', async () => {
  const t = await start();
  try {
    assert.equal((await (await t.api('/api/ai/v1/messages', null, ask('claude-sonnet-5-5'))).json()).error.type, 'authentication_error');
    const free = await connected(t, { plan: null });
    const none = await t.api('/api/ai/v1/messages', free, ask('claude-sonnet-5-5'));
    assert.equal(none.status, 403);
    const noneBody = await none.json();
    assert.equal(noneBody.type, 'error');
    assert.equal(noneBody.error.type, 'plan_required');
    assert.match(noneBody.error.manage_url, /\/account$/);

    await t.site('/api/account/checkout', { plan: 'plus' });
    const opus = await t.api('/api/ai/v1/messages', free, ask('claude-opus-5-5'));
    assert.equal(opus.status, 403);
    const opusBody = await opus.json();
    assert.equal(opusBody.error.type, 'model_not_in_plan');
    assert.match(opusBody.error.message, /Claude Opus 5\.5 isn't in the Plus plan/);
    assert.equal((await t.api('/api/ai/v1/messages', free, ask('gpt-5'))).status, 404);
    assert.equal((await t.api('/api/ai/v1/messages', free, { model: 'claude-haiku-4-5', messages: [] })).status, 400);
  } finally {
    await t.close();
  }
});

test('demo: streamed in the exact Anthropic SSE format, counted into credits', async () => {
  const t = await start();
  try {
    const token = await connected(t);
    const res = await t.api('/api/ai/v1/messages', token, ask('claude-sonnet-5-5', { stream: true }));
    assert.equal(res.status, 200);
    assert.match(res.headers.get('content-type'), /^text\/event-stream/);
    const list = events(await res.text());
    assert.deepEqual(
      [...new Set(list.map((e) => e.event))],
      ['message_start', 'content_block_start', 'ping', 'content_block_delta', 'content_block_stop', 'message_delta', 'message_stop'],
    );
    for (const { event, data } of list) assert.equal(data.type, event, 'each data names its event');
    const start = list[0].data.message;
    assert.equal(start.type, 'message');
    assert.equal(start.role, 'assistant');
    assert.equal(start.model, 'claude-sonnet-5-5');
    assert.deepEqual(start.content, []);
    assert.equal(start.stop_reason, null);
    assert.ok(start.usage.input_tokens > 0);
    assert.deepEqual(list[1].data.content_block, { type: 'text', text: '' });
    const text = list.filter((e) => e.event === 'content_block_delta').map((e) => e.data.delta.text).join('');
    assert.match(text, /lsuite AI demo/);
    assert.ok(list.filter((e) => e.event === 'content_block_delta').every((e) => e.data.index === 0 && e.data.delta.type === 'text_delta'));
    const delta = list.find((e) => e.event === 'message_delta').data;
    assert.equal(delta.delta.stop_reason, 'end_turn');
    assert.ok(delta.usage.output_tokens > 0);
    assert.equal(list.at(-1).event, 'message_stop');
    const me = await (await t.api('/api/account/me', token)).json();
    assert.ok(me.usage.used > 0, 'the demo counts its usage');

    // Not streamed: one message object. Haiku by a dated id works too.
    const plain = await (await t.api('/api/ai/v1/messages', token, ask('claude-haiku-4-5-20251001'))).json();
    assert.equal(plain.type, 'message');
    assert.equal(plain.stop_reason, 'end_turn');
    assert.equal(plain.content[0].type, 'text');
    assert.ok(plain.usage.output_tokens > 0);

    const counted = await (await t.api('/api/ai/v1/messages/count_tokens', token, { model: 'claude-sonnet-5-5', messages: ask('x').messages })).json();
    assert.ok(counted.input_tokens > 0);
  } finally {
    await t.close();
  }
});

test('allowance used up: 402 allowance_exhausted with when it resets and where to manage it', async () => {
  const t = await start();
  try {
    const token = await connected(t);
    const user = Object.values(t.accounts.store.data.users)[0];
    user.usage.used = 1000;
    const res = await t.api('/api/ai/v1/messages', token, ask('claude-sonnet-5-5', { stream: true }));
    assert.equal(res.status, 402);
    const body = await res.json();
    assert.equal(body.type, 'error');
    assert.equal(body.error.type, 'allowance_exhausted');
    assert.match(body.error.message, /allowance for this month is used up \(Plus, 1,000 credits\)\. It resets on 1 \w{3}/);
    assert.match(body.error.manage_url, /\/account$/);
    assert.ok(Date.parse(body.error.resets_at) > Date.now());
    // A new month starts over.
    user.usage.period = '2000-01';
    assert.equal((await t.api('/api/ai/v1/messages', token, ask('claude-sonnet-5-5'))).status, 200);
  } finally {
    await t.close();
  }
});

test('with a server key: streams pass through untouched, the reported usage is charged', async () => {
  const seen = [];
  const sseBody = [
    'event: message_start\ndata: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-5-5","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1000,"cache_creation_input_tokens":0,"cache_read_input_tokens":2000,"output_tokens":1}}}\n\n',
    'event: content_block_start\ndata: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n',
    'event: content_block_delta\ndata: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Done."}}\n\n',
    'event: content_block_stop\ndata: {"type":"content_block_stop","index":0}\n\n',
    'event: message_delta\ndata: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":500}}\n\n',
    'event: message_stop\ndata: {"type":"message_stop"}\n\n',
  ];
  const upstream = createServer(async (req, res) => {
    let raw = '';
    for await (const c of req) raw += c;
    seen.push({ url: req.url, key: req.headers['x-api-key'], version: req.headers['anthropic-version'], beta: req.headers['anthropic-beta'], body: JSON.parse(raw) });
    if (JSON.parse(raw).stream) {
      res.writeHead(200, { 'content-type': 'text/event-stream', 'request-id': 'req_test' });
      // Split mid-event, as networks do.
      const all = sseBody.join('');
      res.write(all.slice(0, 37));
      setTimeout(() => res.end(all.slice(37)), 5);
    } else {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ id: 'msg_2', type: 'message', content: [{ type: 'text', text: 'ok' }], usage: { input_tokens: 200, output_tokens: 100 } }));
    }
  });
  await new Promise((r) => upstream.listen(0, '127.0.0.1', r));
  const t = await start({ anthropicKey: 'sk-ant-server', upstream: `http://127.0.0.1:${upstream.address().port}` });
  try {
    const token = await connected(t, { plan: 'pro' });
    const res = await t.api('/api/ai/v1/messages', token, ask('claude-opus-5-5', { stream: true }), { 'anthropic-beta': 'some-beta-2026-01-01' });
    assert.equal(res.status, 200);
    assert.equal(res.headers.get('request-id'), 'req_test');
    assert.equal(await res.text(), sseBody.join(''), 'byte for byte');
    assert.equal(seen[0].url, '/v1/messages');
    assert.equal(seen[0].key, 'sk-ant-server', 'the server key, never the app token');
    assert.equal(seen[0].version, '2023-06-01');
    assert.equal(seen[0].beta, 'some-beta-2026-01-01');
    assert.equal(seen[0].body.model, 'claude-opus-5-5');
    const opus = resolveModel('claude-opus-5-5');
    const expected = creditsFor(opus, { input_tokens: 1000, cache_read_input_tokens: 2000, output_tokens: 500 });
    await new Promise((r) => setTimeout(r, 20));
    const used = Object.values(t.accounts.store.data.users)[0].usage.used;
    assert.ok(Math.abs(used - expected) < 1e-9, `${used} credits, expected ${expected}`);
    // (1000 × $4 + 2000 × $0.20 + 500 × $20) / 1e6 = $0.0144 = 2.88 credits
    assert.ok(Math.abs(expected - 2.88) < 1e-9);

    const plain = await t.api('/api/ai/v1/messages', token, ask('claude-sonnet-5-5'));
    assert.equal((await plain.json()).id, 'msg_2');
    const after = Object.values(t.accounts.store.data.users)[0].usage.used;
    assert.ok(Math.abs(after - expected - creditsFor(resolveModel('claude-sonnet-5-5'), { input_tokens: 200, output_tokens: 100 })) < 1e-9);
  } finally {
    await t.close();
    upstream.close();
  }
});

test('models: the plan\'s models in the Anthropic list format', async () => {
  const t = await start();
  try {
    assert.equal((await t.api('/api/ai/v1/models')).status, 401);
    const token = await connected(t, { plan: 'plus' });
    const list = await (await t.api('/api/ai/v1/models', token)).json();
    assert.equal(list.has_more, false);
    assert.ok(list.data.length > 0);
    for (const m of list.data) {
      assert.equal(m.type, 'model');
      assert.match(m.id, /sonnet|haiku/);
      assert.ok(m.display_name);
    }
    assert.equal((await (await t.api('/api/ai/v1/models/claude-haiku-4-5', token)).json()).id, 'claude-haiku-4-5');
    assert.equal((await t.api('/api/ai/v1/models/claude-opus-5-5', token)).status, 404);
  } finally {
    await t.close();
  }
});

test('sign-in and code exchange are rate limited per address', async () => {
  const t = await start({ limits: { signin: [2, 60000], token: [2, 60000] } });
  try {
    for (let i = 0; i < 2; i++) assert.equal((await t.site('/api/account/session', { email: `a${i}@example.com`, name: 'A' })).status, 200);
    const third = await t.site('/api/account/session', { email: 'a3@example.com', name: 'A' });
    assert.equal(third.status, 429);
    assert.ok(Number(third.headers.get('retry-after')) > 0);
    assert.equal((await third.json()).error.type, 'rate_limit_error');
    for (let i = 0; i < 2; i++) assert.equal((await t.api('/api/account/token', null, { code: 'lsc_x' })).status, 400);
    assert.equal((await t.api('/api/account/token', null, { code: 'lsc_x' })).status, 429);
  } finally {
    await t.close();
  }
});

test('storage: one 0600 JSON file, secrets only as hashes, survives a restart', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'lsuite-accounts-'));
  const t = await start({ dataDir: dir });
  let token;
  try {
    token = await connected(t);
  } finally {
    await t.close();
  }
  const file = join(dir, 'accounts.json');
  const text = await readFile(file, 'utf8');
  assert.ok(!text.includes(token), 'the token is not stored');
  assert.ok(!/lss_|lsc_|lsk_/.test(text), 'no secret is stored');
  assert.equal((await stat(file)).mode & 0o777, 0o600);
  const again = await start({ dataDir: dir });
  try {
    const me = await (await again.api('/api/account/me', token)).json();
    assert.equal(me.plan, 'plus');
  } finally {
    await again.close();
  }
});

test('the site server answers the API before its pages', async () => {
  const server = createServer((req, res) => handle(req, res));
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  try {
    const base = `http://127.0.0.1:${server.address().port}`;
    const plans = await fetch(`${base}/api/ai/plans`);
    assert.equal(plans.status, 200);
    assert.equal((await plans.json()).plans.length, PLANS.length);
    assert.equal((await fetch(`${base}/api/nothing`)).status, 404);
  } finally {
    server.close();
  }
});
