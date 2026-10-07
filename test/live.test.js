import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHmac } from 'node:crypto';
import { createServer } from 'node:http';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createAccounts } from '../ai.js';
import { verifyWebhook } from '../live.js';

const config = { origin: 'https://lsuite.xyz', stripeKey: 'test-key', webhookSecret: 'test-webhook-secret', emailKey: 'test-email-key', emailFrom: 'lsuite <test@example.com>', prices: { plus: 'price_plus', pro: 'price_pro', studio: 'price_studio' } };
const sign = (raw, ts = Math.floor(Date.now() / 1000)) => `t=${ts},v1=${createHmac('sha256', config.webhookSecret).update(`${ts}.`).update(raw).digest('hex')}`;

async function fixture(t) {
  const dir = await mkdtemp(join(tmpdir(), 'lsuite-live-'));
  const sent = [], calls = [];
  let subscription, priceOverride;
  const fetchProvider = async (url, init) => {
    calls.push({ url, init });
    if (url === 'https://api.resend.com/emails') { sent.push(JSON.parse(init.body)); return Response.json({ id: 'mail_1' }); }
    if (url.includes('/prices/')) return Response.json(priceOverride || { id: 'price_plus', currency: 'usd', unit_amount: 1200, recurring: { interval: 'month', interval_count: 1 }, active: true });
    if (url.endsWith('/customers')) return Response.json({ id: 'cus_1' });
    if (url.endsWith('/checkout/sessions')) return Response.json({ id: 'cs_1', url: 'https://checkout.stripe.com/test' });
    if (url.includes('/checkout/sessions/cs_1')) return Response.json({ id: 'cs_1', status: 'open', url: 'https://checkout.stripe.com/test' });
    if (url.includes('/subscriptions/')) return Response.json(subscription);
    if (url.includes('/billing_portal/')) return Response.json({ url: 'https://billing.stripe.com/test' });
    if (url.includes('api.anthropic.com')) return Response.json({ type: 'message', content: [{ type: 'text', text: 'A real response' }], usage: { input_tokens: 40, output_tokens: 20 } });
    throw new Error(`Unexpected provider request ${url}`);
  };
  const accounts = createAccounts({ mode: 'production', dataDir: dir, anthropicKey: 'test-anthropic', live: config, fetch: fetchProvider });
  await accounts.ready;
  const server = createServer(async (req, res) => {
    try { await accounts.handle(req, res, new URL(req.url, 'http://localhost')); }
    catch { res.writeHead(500); res.end('unexpected server error'); }
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  let cookie;
  const request = async (path, body, headers = {}) => {
    const res = await fetch(base + path, { method: body === undefined ? 'GET' : 'POST', headers: { 'Content-Type': 'application/json', ...(cookie ? { cookie } : {}), ...headers }, body: body === undefined ? undefined : JSON.stringify(body) });
    if (res.headers.get('set-cookie')) cookie = res.headers.get('set-cookie').split(';')[0];
    return res;
  };
  const login = async (email = 'ada@example.com') => {
    const res = await request('/api/account/session', { email, name: 'Ada' });
    assert.equal(res.status, 202);
    const { challenge } = await res.json();
    const code = sent.at(-1).text.match(/\b\d{8}\b/)[0];
    const verified = await request('/api/account/verify', { challenge, code });
    assert.equal(verified.status, 200);
    assert.match(verified.headers.get('set-cookie'), /Secure/);
    return { challenge, code };
  };
  const webhook = async (event, signature) => {
    const raw = JSON.stringify(event);
    return fetch(base + '/api/billing/webhook', { method: 'POST', headers: { 'stripe-signature': signature || sign(raw) }, body: raw });
  };
  t.after(async () => { await new Promise((r) => server.close(r)); await accounts.store.writing; await rm(dir, { recursive: true, force: true }); });
  return { accounts, request, login, webhook, sent, calls, dir, set subscription(v) { subscription = v; }, set price(v) { priceOverride = v; } };
}

test('production requires providers and persistent storage; a demo key cannot enable live forwarding', async (t) => {
  assert.throws(() => createAccounts({ mode: 'production' }), /persistent storage/);
  const d = await mkdtemp(join(tmpdir(), 'lsuite-mode-'));
  t.after(() => rm(d, { recursive: true, force: true }));
  assert.throws(() => createAccounts({ mode: 'production', dataDir: d, anthropicKey: 'test', live: {} }), /requires origin/);
  const a = createAccounts({ anthropicKey: 'test', fetch: () => { throw new Error('Demo must never contact a provider'); } });
  await a.ready;
  assert.equal(a.store.file, null);
});

test('verified sign-in: no session before verification, codes hashed, single use, and five-guess limit', async (t) => {
  const f = await fixture(t);
  const res = await f.request('/api/account/session', { email: 'ada@example.com', name: 'Ada' });
  const { challenge } = await res.json();
  assert.equal(res.headers.get('set-cookie'), null);
  assert.equal((await f.request('/api/account/me')).status, 401);
  const code = f.sent[0].text.match(/\b\d{8}\b/)[0];
  const disk = await readFile(join(f.dir, 'production-accounts.json'), 'utf8');
  assert.ok(!disk.includes(`:${code}`));
  assert.equal((await f.request('/api/account/verify', { challenge, code })).status, 200);
  assert.equal((await f.request('/api/account/verify', { challenge, code })).status, 400);
  assert.equal((await (await f.request('/api/account/me')).json()).plan, 'free');
  const pending = await (await f.request('/api/account/session', { email: 'other@example.com', name: 'Other' })).json();
  const right = f.sent.at(-1).text.match(/\b\d{8}\b/)[0];
  for (let i = 0; i < 5; i++) assert.equal((await f.request('/api/account/verify', { challenge: pending.challenge, code: 'bad' })).status, 400);
  assert.equal((await f.request('/api/account/verify', { challenge: pending.challenge, code: right })).status, 400);
  assert.equal((await f.request('/api/account/session', { email: 'ada@example.com' }, { origin: 'https://evil.example' })).status, 403);
});

test('checkout uses approved USD amounts; paid webhooks grant access, replay and cancellation are safe', async (t) => {
  const f = await fixture(t);
  await f.login();
  const prices = await (await f.request('/api/ai/plans')).json();
  assert.equal(prices.demo, false);
  assert.equal(prices.currency, 'USD');
  assert.deepEqual(prices.plans.map((p) => p.price), [0, 12, 29, 79]);
  f.price = { id: 'price_plus', unit_amount: 1200, currency: 'eur', active: true, recurring: { interval: 'month', interval_count: 1 } };
  assert.equal((await f.request('/api/account/checkout', { plan: 'plus' })).status, 503);
  f.price = null;
  const checkout = await (await f.request('/api/account/checkout', { plan: 'plus', next: '//evil.example' })).json();
  assert.equal(checkout.url, 'https://checkout.stripe.com/test');
  assert.equal((await (await f.request('/api/account/me')).json()).plan, 'free', 'redirect is not proof of payment');
  await f.request('/api/account/checkout', { plan: 'plus' });
  assert.equal(f.calls.filter((c) => c.url.endsWith('/checkout/sessions')).length, 1, 'reuses unfinished checkout');
  const user = Object.values(f.accounts.store.data.users)[0];
  const now = Math.floor(Date.now() / 1000);
  const sub = { id: 'sub_1', customer: 'cus_1', metadata: { lsuite_user: user.id }, status: 'active', latest_invoice: { status: 'paid' }, items: { data: [{ quantity: 1, current_period_start: now - 10, current_period_end: now + 86400, price: { id: 'price_plus', currency: 'usd', unit_amount: 1200, active: true, recurring: { interval: 'month', interval_count: 1 } } }] } };
  f.subscription = sub;
  const event = { id: 'evt_paid', type: 'invoice.paid', data: { object: { parent: { subscription_details: { subscription: 'sub_1' } } } } };
  assert.equal((await f.webhook(event, sign('{}'))).status, 400);
  assert.equal((await f.webhook(event)).status, 200);
  assert.equal((await (await f.request('/api/account/me')).json()).plan, 'plus');
  const { key } = await (await f.request('/api/account/key', {})).json();
  const ask = () => f.request('/api/ai/v1/messages', { model: 'claude-haiku-4-5', max_tokens: 256, messages: [{ role: 'user', content: 'Hello' }] }, { 'x-api-key': key });
  assert.equal((await ask()).status, 200);
  const used = user.usage.used;
  assert.ok(used > 0);
  await f.webhook(event);
  assert.equal(user.usage.used, used);
  assert.equal((await (await f.request('/api/account/checkout', { plan: 'pro' })).json()).url, 'https://billing.stripe.com/test');
  f.subscription = { ...sub, status: 'canceled' };
  await f.webhook({ ...event, id: 'evt_cancel' });
  assert.equal((await ask()).status, 403);
  await f.webhook({ ...event, id: 'evt_old_paid' });
  assert.equal((await ask()).status, 403, 'a delayed paid event cannot restore cancelled access');
});

test('webhook signatures reject altered bodies, expired timestamps and unknown keys', () => {
  const raw = Buffer.from('{"id":"evt_1"}');
  assert.equal(verifyWebhook(raw, sign(raw), config.webhookSecret).id, 'evt_1');
  assert.throws(() => verifyWebhook(raw, sign(raw, 1), config.webhookSecret), /timestamp/);
  assert.throws(() => verifyWebhook(Buffer.from('{}'), sign(raw), config.webhookSecret), /signature/);
  assert.throws(() => verifyWebhook(raw, sign(raw), 'another-key'), /signature/);
});
