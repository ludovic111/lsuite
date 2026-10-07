// lsuite — the account pages (/account, /account/connect, /account/checkout). The session is an
// HttpOnly cookie the server sets; this script only calls the same-origin JSON API (AI.md) and
// shows one `[data-state]` block at a time. User data is only ever set as text.

const page = document.body.dataset.page;
const params = new URLSearchParams(location.search);
const APP_NAMES = { ryolune: 'ryolune', kimchi: 'kimchi', zenith: 'zenith', nori: 'nori', folio: 'folio', lsuite: 'lsuite launcher' };
/** How /account/connect names the app in its sentences ("Connect the lsuite launcher"). */
const CONNECT_NAMES = { ...APP_NAMES, lsuite: 'the lsuite launcher' };
let plans = [];
let demo = true;

async function api(path, body) {
  const res = await fetch(path, {
    method: body === undefined ? 'GET' : 'POST',
    headers: body === undefined ? {} : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: 'same-origin',
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    const err = new Error(data?.error?.message || `Something went wrong (${res.status}). Try again.`);
    err.status = res.status;
    throw err;
  }
  return data;
}

const me = () => api('/api/account/me').catch((err) => (err.status === 401 ? null : Promise.reject(err)));

function show(state) {
  document.querySelectorAll('[data-state]').forEach((el) => (el.hidden = el.dataset.state !== state));
  const shown = document.querySelector(`[data-state="${state}"]`);
  shown?.querySelector('input')?.focus({ preventScroll: true });
}

function bind(name, text) {
  document.querySelectorAll(`[data-bind="${name}"]`).forEach((el) => (el.textContent = text));
}

/** Bytes in decimal units, as lsuite Cloud counts them (1 GB = 1e9 bytes). */
const bytes = (n) => {
  const [size, unit] = [[1e12, 'TB'], [1e9, 'GB'], [1e6, 'MB'], [1e3, 'KB']].find(([u]) => n >= u) ?? [1, 'bytes'];
  return `${Math.round((n / size) * 10) / 10} ${unit}`;
};
const credits = (n) => `${Math.round(n).toLocaleString('en-US')} credits`;
const day = (iso) => new Date(iso).toLocaleDateString('en-GB', { day: 'numeric', month: 'short', timeZone: 'UTC' });
const price = (p) => (p.price ? `$${p.price} / month` : 'Free');

/** Sign-in forms: email and name, then `after()`. */
function wireSignIn(after) {
  document.querySelectorAll('[data-signin]').forEach((form) => {
    form.addEventListener('submit', async (e) => {
      e.preventDefault();
      const error = form.querySelector('.form__error');
      const button = form.querySelector('[type="submit"]');
      error.textContent = '';
      const email = form.elements.email.value.trim();
      if (!email) {
        error.textContent = 'Enter your email.';
        return;
      }
      button.disabled = true;
      try {
        if (form.dataset.challenge) {
          await api('/api/account/verify', { challenge: form.dataset.challenge, code: form.elements.code.value.trim() });
          delete form.dataset.challenge;
          await after();
        } else {
          const result = await api('/api/account/session', { email, name: form.elements.name.value.trim() });
          if (result.verificationRequired) {
            form.dataset.challenge = result.challenge;
            form.elements.email.readOnly = true;
            const field = document.createElement('div');
            field.className = 'field';
            const label = document.createElement('label');
            label.htmlFor = 'signin-code';
            label.textContent = 'Code from your email';
            const code = document.createElement('input');
            Object.assign(code, { id: 'signin-code', name: 'code', type: 'text', inputMode: 'numeric', autocomplete: 'one-time-code', maxLength: 8 });
            field.append(label, code);
            form.insertBefore(field, error);
            button.textContent = 'Verify and sign in';
            error.textContent = 'Check your email. The code expires in 10 minutes.';
            const again = document.createElement('button');
            Object.assign(again, { type: 'button', className: 'btn btn--sm', textContent: 'Use another email or resend' });
            again.onclick = () => { delete form.dataset.challenge; field.remove(); again.remove(); form.elements.email.readOnly = false; error.textContent = ''; button.textContent = 'Send sign-in code'; };
            form.append(again);
            code.focus();
          } else await after();
        }
      } catch (err) {
        error.textContent = err.message;
      } finally {
        button.disabled = false;
      }
    });
  });
}

async function signOut() {
  await api('/api/account/signout', {}).catch(() => {});
  location.reload();
}

function wireActions(handlers) {
  document.querySelectorAll('[data-action]').forEach((el) => {
    const run = handlers[el.dataset.action];
    if (run) el.addEventListener('click', (e) => run(e, el));
  });
}

/** Marks the current plan in a plans grid; `href(plan)` is where each button goes. */
function markPlans(root, account, href) {
  root.querySelectorAll('.plan').forEach((card) => {
    const id = card.dataset.plan;
    const btn = card.querySelector('.btn');
    const current = account && account.plan === id;
    card.classList.toggle('is-current', current);
    if (!btn) return;
    btn.setAttribute('href', href(id));
    if (current) btn.textContent = 'Your plan';
    else if (id === 'free' && account) btn.textContent = 'Switch to Free';
  });
}

// ---------- /account ----------
async function accountPage() {
  const account = await me();
  if (!account) {
    show('out');
    return;
  }
  bind('name', account.name);
  bind('email', account.email);
  bind('origin', location.origin);
  bind('planName', account.planName);
  const plan = plans.find((p) => p.id === account.plan);
  bind('planPrice', plan && plan.price ? `· ${price(plan)} USD${demo ? ' · demo' : ''}` : '');
  const paid = account.status === 'active';
  document.querySelector('[data-when="paid"]').hidden = !paid;
  document.querySelector('[data-when="free"]').hidden = paid;
  bind('period', paid ? `resets ${day(account.usage.resetsAt)}` : '');
  if (paid) {
    const meter = document.querySelector('[data-when="paid"] .meter');
    meter.style.setProperty('--used', `${account.usage.percent}%`);
    meter.setAttribute('aria-valuenow', String(account.usage.percent));
    const left = Math.max(0, account.usage.limit - account.usage.used);
    bind('usage', `${account.planName} · ${account.usage.percent} % used · ${credits(left)} left of ${credits(account.usage.limit)} · resets ${day(account.usage.resetsAt)}`);
  }
  bind('models', account.models.length ? `Models: ${account.models.join(', ')}` : '');
  showCloud(account.cloud);
  markPlans(document.querySelector('[data-plans]'), account, (id) => `/account/checkout?plan=${id}`);

  const table = document.querySelector('[data-conns]');
  const body = table.querySelector('tbody');
  const renderConns = (list) => {
    body.replaceChildren(
      ...list.map((c) => {
        const tr = document.createElement('tr');
        const cells = [c.kind === 'key' ? 'Pasted key' : APP_NAMES[c.app] || c.app, day(c.createdAt), c.lastUsedAt ? day(c.lastUsedAt) : '—'];
        for (const text of cells) {
          const td = document.createElement('td');
          td.textContent = text;
          tr.append(td);
        }
        const td = document.createElement('td');
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'btn btn--sm';
        btn.textContent = 'Disconnect';
        btn.addEventListener('click', async () => {
          btn.disabled = true;
          const { connections } = await api('/api/account/disconnect', { id: c.id });
          renderConns(connections);
        });
        td.append(btn);
        tr.append(td);
        return tr;
      }),
    );
    table.hidden = !list.length;
    document.querySelector('[data-when="noconns"]').hidden = list.length > 0;
  };
  renderConns(account.connections ?? []);
  show('in');
  const billing = document.querySelector('[data-action=billing]');
  if (billing) billing.hidden = demo || account.status !== 'active';
  if (!demo && params.get('payment') === 'processing' && !paid) {
    bind('models', 'Your payment is being confirmed. This page updates automatically.');
    setTimeout(accountPage, 4000);
  }
  if (location.hash === '#plans') document.querySelector('[data-plans]').hidden = false;
}

/** The lsuite Cloud panel: used / quota and the files, or which plans include it. */
function showCloud(cloud) {
  if (!cloud) return;
  const hasRoom = cloud.quota > 0;
  document.querySelector('[data-when="cloud"]').hidden = !hasRoom;
  document.querySelector('[data-when="nocloud"]').hidden = hasRoom;
  const files = `${cloud.files.toLocaleString('en-US')} file${cloud.files === 1 ? '' : 's'}`;
  bind('cloudFiles', cloud.files || hasRoom ? files : '');
  if (!hasRoom) {
    const offer = plans.filter((p) => p.storage).map((p) => `${p.storageLabel} with ${p.name}`).join(', ');
    bind('cloudPlans', cloud.files ? `${offer}. Your ${files} (${bytes(cloud.used)}) can still be downloaded and deleted` : offer);
    return;
  }
  const percent = Math.min(100, Math.round((cloud.used / cloud.quota) * 100));
  const meter = document.querySelector('[data-when="cloud"] .meter');
  meter.style.setProperty('--used', `${percent}%`);
  meter.setAttribute('aria-valuenow', String(percent));
  bind('cloudUsage', `${bytes(cloud.used)} of ${bytes(cloud.quota)} used · ${files}${demo ? ' · demo storage is capped' : ''}`);
}

// ---------- /account/connect ----------
const connectArgs = () => {
  const app = params.get('app');
  const port = Number(params.get('port'));
  const state = params.get('state') ?? '';
  const ok = APP_NAMES[app] && Number.isInteger(port) && port >= 1024 && port <= 65535 && /^[\w.~-]{8,256}$/.test(state);
  return ok ? { app, port, state } : null;
};

async function connectPage() {
  const args = connectArgs();
  if (!args) {
    show('bad');
    return;
  }
  bind('app', CONNECT_NAMES[args.app]);
  document.title = `Connect ${CONNECT_NAMES[args.app]} · lsuite`;
  const account = await me();
  if (!account) {
    show('out');
    return;
  }
  bind('name', account.name);
  bind('email', account.email);
  if (account.status !== 'active') {
    const back = `/account/connect?${params.toString()}`;
    markPlans(document.querySelector('[data-plan-links]'), account, (id) => `/account/checkout?plan=${id}&next=${encodeURIComponent(back)}`);
    // Free is the "connect on Free" link below the plans.
    document.querySelector('[data-plan-links] [data-plan="free"]')?.remove();
    document.querySelector('[data-plan-links] .plans')?.style.setProperty('grid-template-columns', 'repeat(auto-fit, minmax(220px, 1fr))');
    show('plan');
    return;
  }
  const left = Math.max(0, account.usage.limit - account.usage.used);
  bind('planLine', `${account.planName} · ${credits(left)} left this month${demo ? ' (demo)' : ''}`);
  show('ready');
}

async function connect(e, el) {
  const args = connectArgs();
  if (!args) return;
  el.disabled = true;
  const error = document.querySelector('[data-connect-error]');
  if (error) error.textContent = '';
  try {
    const { code } = await api('/api/account/connect', { app: args.app });
    show('done');
    const back = new URL(`http://127.0.0.1:${args.port}/callback`);
    back.searchParams.set('code', code);
    back.searchParams.set('state', args.state);
    location.replace(back.href);
  } catch (err) {
    if (error && !error.closest('[hidden]')) error.textContent = err.message;
    else alert(err.message);
    el.disabled = false;
  }
}

// ---------- /account/checkout ----------
const nextPath = () => {
  const next = params.get('next') ?? '';
  return /^\/account(\/[\w-]*)?(\?[^#]*)?$/.test(next) ? next : '/account';
};

async function checkoutPage() {
  const plan = plans.find((p) => p.id === params.get('plan'));
  if (!plan) {
    show('bad');
    return;
  }
  bind('planName', plan.name);
  bind('planPrice', plan.price ? `$${plan.price} USD / month${demo ? ' · demo' : ''}` : 'Free');
  bind('summary', plan.summary);
  bind('credits', plan.credits ? `${credits(plan.credits)} a month` : 'None: bring your own provider');
  bind('families', plan.families.length ? plan.families.join(', ') : 'The ones you bring');
  const account = await me();
  if (!account) {
    show('out');
    return;
  }
  bind('email', account.email);
  bind('current', account.plan === plan.id ? `${plan.name} is already your plan.` : `Today you're on ${account.planName}. ${demo ? 'Changing plan starts a fresh allowance.' : 'Manage changes and cancellation through Stripe.'}`);
  show('ready');
}

async function checkout(e, el) {
  el.disabled = true;
  const error = document.querySelector('[data-checkout-error]');
  error.textContent = '';
  try {
    const result = await api('/api/account/checkout', { plan: params.get('plan'), next: nextPath() });
    location.assign(result.url || nextPath());
  } catch (err) {
    error.textContent = err.message;
    el.disabled = false;
  }
}

// ---------- start ----------
async function start() {
  try {
    const catalogue = await api('/api/ai/plans');
    plans = catalogue.plans;
    demo = catalogue.demo;
    if (!demo) {
      document.querySelectorAll('[data-demo]').forEach((el) => el.hidden = true);
      document.querySelectorAll('[data-live]').forEach((el) => el.hidden = false);
      document.querySelectorAll('[data-signin] [type=submit]').forEach((el) => el.textContent = 'Send sign-in code');
      document.querySelector('[data-action=checkout]')?.replaceChildren(document.createTextNode('Continue to secure billing'));
    }
    const run = { account: accountPage, connect: connectPage, checkout: checkoutPage }[page];
    wireSignIn(run);
    wireActions({
      signout: signOut,
      billing: async (e, el) => { el.disabled = true; try { const result = await api('/api/account/portal', {}); location.assign(result.url); } catch (err) { alert(err.message); el.disabled = false; } },
      connect,
      checkout,
      plans: (e) => {
        e.preventDefault();
        const panel = document.querySelector('[data-plans]');
        panel.hidden = !panel.hidden;
        if (!panel.hidden) panel.scrollIntoView({ behavior: 'smooth', block: 'start' });
      },
      key: async (e, el) => {
        el.disabled = true;
        try {
          const { key } = await api('/api/account/key', {});
          bind('key', key);
          document.querySelector('[data-keyout]').hidden = false;
          el.textContent = 'Make a new key';
          await accountPage();
        } finally {
          el.disabled = false;
        }
      },
      copy: async (e, el) => {
        const key = document.querySelector('[data-bind="key"]').textContent;
        try {
          await navigator.clipboard.writeText(key);
          el.textContent = 'Copied';
          setTimeout(() => (el.textContent = 'Copy'), 1600);
        } catch {}
      },
    });
    await run();
  } catch (err) {
    const loading = document.querySelector('[data-state="loading"]');
    loading.hidden = false;
    loading.textContent = `The account service didn't answer (${err.message}). Reload to try again.`;
  }
}

start();
