// Production account services. Demo accounts use a separate file and never reach these providers.
import { createHash, createHmac, randomBytes, randomInt, timingSafeEqual } from 'node:crypto';

const hash = (s) => createHash('sha256').update(s).digest('hex');
const id = (s) => typeof s === 'string' ? s : s?.id;
const amounts = { plus: 1200, pro: 2900, studio: 7900 };
export class ServiceError extends Error {
  constructor(status, message) { super(message); this.status = status; this.type = 'service_error'; }
}

export function liveConfig(env = process.env) {
  return {
    origin: env.LSUITE_PUBLIC_ORIGIN || 'https://lsuite.xyz',
    stripeKey: env.LSUITE_STRIPE_SECRET_KEY,
    webhookSecret: env.LSUITE_STRIPE_WEBHOOK_SECRET,
    emailKey: env.LSUITE_RESEND_API_KEY,
    emailFrom: env.LSUITE_EMAIL_FROM,
    prices: Object.fromEntries(Object.keys(amounts).map((p) => [p, env[`LSUITE_STRIPE_PRICE_${p.toUpperCase()}`]])),
  };
}

export function verifyWebhook(raw, header, secret, time = Date.now()) {
  const parts = String(header || '').split(',').map((v) => v.trim().split('='));
  const ts = parts.find(([k]) => k === 't')?.[1];
  if (!/^\d+$/.test(ts || '') || Math.abs(time / 1000 - Number(ts)) > 300) throw new ServiceError(400, 'Invalid webhook timestamp.');
  const expected = createHmac('sha256', secret).update(`${ts}.`).update(raw).digest();
  const valid = parts.some(([k, v]) => k === 'v1' && /^[a-f0-9]{64}$/.test(v || '') && timingSafeEqual(expected, Buffer.from(v, 'hex')));
  if (!valid) throw new ServiceError(400, 'Invalid webhook signature.');
  try { return JSON.parse(raw.toString('utf8')); } catch { throw new ServiceError(400, 'Invalid webhook JSON.'); }
}

export function productionServices(config, store, fetchImpl = fetch) {
  for (const key of ['origin', 'stripeKey', 'webhookSecret', 'emailKey', 'emailFrom']) {
    if (!config[key]) throw new Error(`Production AI requires ${key}; see AI.md.`);
  }
  if (new URL(config.origin).protocol !== 'https:') throw new Error('Production AI requires an HTTPS public origin.');
  for (const plan of Object.keys(amounts)) if (!/^price_/.test(config.prices?.[plan] || '')) throw new Error(`Production AI requires the ${plan} Stripe price.`);
  const origin = new URL(config.origin).origin;
  const locks = new Map();
  async function locked(key, work) {
    const previous = locks.get(key) || Promise.resolve();
    const next = previous.catch(() => {}).then(work);
    locks.set(key, next);
    try { return await next; } finally { if (locks.get(key) === next) locks.delete(key); }
  }
  async function stripe(path, values, key) {
    const res = await fetchImpl(`https://api.stripe.com/v1/${path}`, {
      method: values ? 'POST' : 'GET',
      headers: { Authorization: `Bearer ${config.stripeKey}`, 'Stripe-Version': '2025-06-30.basil', ...(values ? { 'Content-Type': 'application/x-www-form-urlencoded' } : {}), ...(key ? { 'Idempotency-Key': key } : {}) },
      body: values ? new URLSearchParams(values) : undefined,
      signal: AbortSignal.timeout(20000),
    });
    if (!res.ok) throw new ServiceError(502, 'The payment service is temporarily unavailable. Please try again.');
    return res.json();
  }
  function validatePrice(price, plan) {
    if (price.id !== config.prices[plan] || price.currency !== 'usd' || price.unit_amount !== amounts[plan] || price.recurring?.interval !== 'month' || price.recurring?.interval_count !== 1 || !price.active) {
      throw new ServiceError(503, 'This subscription price is not configured correctly. No payment was started.');
    }
  }
  const prune = () => {
    store.data.verifications ??= {};
    for (const [key, c] of Object.entries(store.data.verifications)) if (c.expiresAt < Date.now()) delete store.data.verifications[key];
  };
  async function sendCode(email, name) {
    prune();
    // Per-address cooldown also applies across server restarts. Do not invalidate earlier codes.
    if (Object.values(store.data.verifications).some((c) => c.email === email && c.createdAt > Date.now() - 60000)) throw new ServiceError(429, 'A code was sent recently. Wait one minute before requesting another.');
    const challenge = randomBytes(24).toString('base64url');
    const code = String(randomInt(0, 100000000)).padStart(8, '0');
    store.data.verifications[challenge] = { email, name, codeHash: hash(`${challenge}:${code}`), createdAt: Date.now(), expiresAt: Date.now() + 600000, attempts: 0 };
    await store.save();
    try {
      const res = await fetchImpl('https://api.resend.com/emails', {
        method: 'POST', headers: { Authorization: `Bearer ${config.emailKey}`, 'Content-Type': 'application/json', 'Idempotency-Key': `signin-${challenge}` },
        body: JSON.stringify({ from: config.emailFrom, to: [email], subject: 'Your lsuite sign-in code', text: `Your lsuite sign-in code is ${code}.\n\nIt expires in 10 minutes and works once. If you did not request it, ignore this email.` }),
        signal: AbortSignal.timeout(15000),
      });
      if (!res.ok) throw new Error('delivery failed');
    } catch {
      delete store.data.verifications[challenge];
      await store.save();
      throw new ServiceError(503, 'The sign-in email could not be sent. Please try again later.');
    }
    return { challenge, verificationRequired: true, expiresIn: 600 };
  }
  async function verifyCode(challenge, code) {
    prune();
    if (!/^[\w-]{32}$/.test(challenge)) throw new ServiceError(400, 'Invalid sign-in challenge.');
    const c = store.data.verifications[challenge];
    if (!c || c.attempts >= 5) throw new ServiceError(400, 'This code has expired or was already used. Request a new one.');
    c.attempts++;
    const actual = hash(`${challenge}:${String(code)}`);
    const valid = timingSafeEqual(Buffer.from(actual), Buffer.from(c.codeHash));
    if (!valid) {
      if (c.attempts >= 5) delete store.data.verifications[challenge];
      await store.save();
      throw new ServiceError(400, 'Incorrect sign-in code.');
    }
    delete store.data.verifications[challenge];
    let user = store.userByEmail(c.email);
    if (!user) {
      user = { id: `u_${randomBytes(12).toString('base64url')}`, email: c.email, name: c.name || c.email.split('@')[0], plan: 'free', createdAt: Date.now() };
      store.data.users[user.id] = user;
    }
    user.verifiedAt = Date.now();
    await store.save();
    return user;
  }
  async function portal(user) {
    if (!user.billing?.customer) throw new ServiceError(400, 'This account has no paid subscription to manage.');
    const result = await stripe('billing_portal/sessions', { customer: user.billing.customer, return_url: `${origin}/account` });
    return { url: result.url, demo: false };
  }
  async function checkout(user, plan, next) {
    return locked(`checkout:${user.id}`, async () => {
      if (user.billing?.subscription || plan === 'free') return portal(user);
      if (!amounts[plan]) throw new ServiceError(400, 'Unknown subscription plan.');
      validatePrice(await stripe(`prices/${encodeURIComponent(config.prices[plan])}`), plan);
      if (!user.billing?.customer) {
        const customer = await stripe('customers', { email: user.email, name: user.name, 'metadata[lsuite_user]': user.id }, `customer-${user.id}`);
        user.billing = { customer: customer.id };
        await store.save();
      }
      // Reuse an unfinished checkout so double clicks cannot create duplicate subscriptions.
      if (user.billing.checkout) {
        const old = await stripe(`checkout/sessions/${encodeURIComponent(user.billing.checkout)}`);
        if (old.status === 'open') {
          if (user.billing.checkoutPlan === plan) return { url: old.url, demo: false };
          await stripe(`checkout/sessions/${encodeURIComponent(old.id)}/expire`, {});
        } else if (old.status === 'complete') return portal(user);
      }
      const back = typeof next === 'string' && /^\/account(?:\/[\w-]*)?(?:\?[^#]*)?$/.test(next) ? next : '/account';
      const result = await stripe('checkout/sessions', {
        mode: 'subscription', customer: user.billing.customer, client_reference_id: user.id,
        'line_items[0][price]': config.prices[plan], 'line_items[0][quantity]': '1',
        'subscription_data[metadata][lsuite_user]': user.id,
        success_url: `${origin}${back}${back.includes('?') ? '&' : '?'}payment=processing`,
        cancel_url: `${origin}/account/checkout?plan=${plan}`,
      }, `checkout-${user.id}-${plan}-${user.billing.checkout || 'first'}`);
      user.billing.checkout = result.id;
      user.billing.checkoutPlan = plan;
      await store.save();
      return { url: result.url, demo: false };
    });
  }
  async function reconcile(subscriptionId) {
    const sub = await stripe(`subscriptions/${encodeURIComponent(subscriptionId)}?expand[]=latest_invoice`);
    const user = store.data.users[sub.metadata?.lsuite_user];
    if (!user?.verifiedAt || user.billing?.customer !== id(sub.customer)) return;
    // Do not let an old cancelled subscription displace a newer subscription.
    if (user.billing.subscription && user.billing.subscription !== sub.id) return;
    const items = sub.items?.data || [];
    const item = items[0];
    const plan = Object.keys(amounts).find((p) => config.prices[p] === item?.price?.id);
    const end = item?.current_period_end ?? sub.current_period_end;
    const start = item?.current_period_start ?? sub.current_period_start;
    const invoice = sub.latest_invoice;
    let eligible = sub.status === 'active' && invoice?.status === 'paid' && items.length === 1 && item.quantity === 1 && end * 1000 > Date.now() && !!plan;
    if (eligible) { try { validatePrice(item.price, plan); } catch { eligible = false; } }
    user.plan = eligible ? plan : 'free';
    Object.assign(user.billing, { subscription: sub.status === 'canceled' ? null : sub.id, status: sub.status, periodStart: start * 1000, periodEnd: end * 1000, cancelAtPeriodEnd: !!sub.cancel_at_period_end });
    await store.save();
  }
  async function webhook(raw, signature) {
    const event = verifyWebhook(raw, signature, config.webhookSecret);
    if (typeof event.id !== 'string' || !event.id.startsWith('evt_')) throw new ServiceError(400, 'Invalid event.');
    // Serialized reconciliation retrieves current Stripe state; duplicate and reordered events
    // cannot restore cancelled access or reset an allowance.
    return locked('webhook', async () => {
      store.data.stripeEvents ??= {};
      if (store.data.stripeEvents[event.id]) return;
      const object = event.data?.object;
      let subscription;
      if (['customer.subscription.created', 'customer.subscription.updated', 'customer.subscription.deleted'].includes(event.type)) subscription = object?.id;
      if (['invoice.paid', 'invoice.payment_failed'].includes(event.type)) subscription = id(object?.subscription ?? object?.parent?.subscription_details?.subscription);
      if (subscription && /^sub_/.test(subscription)) await reconcile(subscription);
      store.data.stripeEvents[event.id] = Date.now();
      for (const [key, ts] of Object.entries(store.data.stripeEvents)) if (ts < Date.now() - 30 * 86400000) delete store.data.stripeEvents[key];
      await store.save();
    });
  }
  return { sendCode, verifyCode, checkout, portal, webhook, origin };
}

/** Studio requests go first when the gateway is busy; active requests are never interrupted. */
export function requestQueue(capacity = 8) {
  let active = 0;
  const pending = [];
  function drain() {
    pending.sort((a, b) => Number(b.priority) - Number(a.priority));
    while (active < capacity && pending.length) {
      const item = pending.shift();
      active++;
      Promise.resolve().then(item.work).then(item.resolve, item.reject).finally(() => { active--; drain(); });
    }
  }
  return (priority, work) => new Promise((resolve, reject) => {
    if (pending.length >= 64) return reject(new ServiceError(503, 'lsuite AI is busy. Please try again shortly.'));
    pending.push({ priority, work, resolve, reject });
    drain();
  });
}
