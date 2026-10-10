import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { hostRedirect, osFor, supportTarget, downloadTarget, renderPage, handle, appVersions, versionOf, marketplaceHtml, plansHtml, DOWNLOADS, APP_NAMES, PAGES } from '../server.js';

test('ryolune.com lands on the ryolune page, path and query kept', () => {
  assert.equal(hostRedirect('ryolune.com', '/', 'lsuite.xyz'), 'https://lsuite.xyz/ryolune');
  assert.equal(hostRedirect('www.ryolune.com', '/', 'lsuite.xyz'), 'https://lsuite.xyz/ryolune');
  assert.equal(hostRedirect('ryolune.com', '/support', 'lsuite.xyz'), 'https://lsuite.xyz/ryolune/support');
  assert.equal(hostRedirect('ryolune.com', '/download/macos-arm64', 'lsuite.xyz'), 'https://lsuite.xyz/ryolune/download/macos-arm64');
  assert.equal(hostRedirect('ryolune.com', '/?ref=app', 'lsuite.xyz'), 'https://lsuite.xyz/ryolune?ref=app');
  assert.equal(hostRedirect('site-production-7751.up.railway.app', '/support', 'lsuite.xyz'), 'https://lsuite.xyz/ryolune/support');
  // Even before the canonical host is configured.
  assert.equal(hostRedirect('ryolune.com:443', '/', ''), 'https://lsuite.xyz/ryolune');
});

test('other hosts go to the canonical one; local hosts are served', () => {
  assert.equal(hostRedirect('www.lsuite.xyz', '/kimchi', 'lsuite.xyz'), 'https://lsuite.xyz/kimchi');
  assert.equal(hostRedirect('lsuite-production.up.railway.app', '/', 'lsuite.xyz'), 'https://lsuite.xyz/');
  assert.equal(hostRedirect('lsuite.xyz', '/nori', 'lsuite.xyz'), null);
  assert.equal(hostRedirect('localhost:4321', '/', 'lsuite.xyz'), null);
  assert.equal(hostRedirect('127.0.0.1:4321', '/', 'lsuite.xyz'), null);
  assert.equal(hostRedirect('anything.example', '/', ''), null);
});

test('platform from the user agent', () => {
  assert.equal(osFor('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)'), 'macos');
  assert.equal(osFor('Mozilla/5.0 (Windows NT 10.0; Win64; x64)'), 'windows');
  assert.equal(osFor('Mozilla/5.0 (X11; Linux x86_64)'), 'linux');
  assert.equal(osFor('Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)'), null);
});

test('the four apps come only through the lsuite app: their pages and download routes lead to it', async () => {
  for (const app of ['ryolune', 'kimchi', 'nori', 'folio']) {
    for (const wanted of [undefined, 'macos-arm64', 'nope']) assert.equal(await downloadTarget(app, wanted, 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)'), '/launcher', app);
    assert.equal(DOWNLOADS[app].repo, `ludovic111/${app}`);
    assert.notEqual(DOWNLOADS[app].published, false);
    const html = await renderPage(`${app}/index.html`, 'https://lsuite.xyz', { [app]: '9.8.7' });
    assert.ok(!html.includes(`/${app}/download`), `${app}: no download of its own`);
    assert.ok(!html.includes(`github.com/ludovic111/${app}/releases"`), `${app}: no public releases`);
    assert.ok(html.includes(`<h2 class="h2" id="downloads-title">Get ${app} in the lsuite app.</h2>`), `${app}: get it in the lsuite app`);
    assert.ok(html.includes('<a class="btn btn--primary" href="/launcher/download"'), `${app}: lsuite's download is the primary button`);
    assert.ok(html.includes('href="/launcher">About the lsuite app'), `${app}: links /launcher`);
    assert.ok(html.includes(`<a class="btn btn--primary" href="#downloads">Get ${app}</a>`), `${app}: the hero leads there`);
    assert.ok(html.includes('free lsuite account'), `${app}: the account is free`);
    assert.ok(html.includes(`Version 9.8.7`) && html.includes(`${app} 9.8.7 ·`), `${app}: version shown`);
    assert.ok(!html.includes('First public build coming'), app);
  }
  assert.equal(await downloadTarget('photoshop', 'macos-arm64'), null);
  const home = await renderPage('index.html', 'https://lsuite.xyz', { ryolune: '1.0.1', kimchi: '1.0.2', nori: '1.0.4', folio: '1.0.5' });
  assert.ok(home.includes('<a class="btn btn--primary" href="/launcher/download">Download lsuite'));
  assert.ok(home.includes('come through the lsuite app with a free account'));
  for (const v of ['1.0.1', '1.0.2', '1.0.4', '1.0.5']) assert.ok(home.includes(`Beta · v${v}`), v);

  const server = createServer((req, res) => handle(req, res));
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const base = `http://127.0.0.1:${server.address().port}`;
    const get = (path, headers = {}) => fetch(base + path, { redirect: 'manual', headers });
    for (const app of APP_NAMES) {
      for (const path of [`/${app}/download`, `/${app}/download/macos-arm64`, `/${app}/download/windows-x86_64`]) {
        const res = await get(path);
        assert.equal(res.status, 302, path);
        assert.equal(res.headers.get('location'), '/launcher', path);
      }
    }
    // An app that left the suite: its old addresses land on the home page.
    for (const path of ['/zenith', '/zenith/', '/zenith/download', '/zenith/download/linux-x86_64']) {
      const res = await get(path);
      assert.equal(res.status, 301, path);
      assert.equal(res.headers.get('location'), '/', path);
    }
    assert.ok(!(await (await get('/sitemap.xml')).text()).includes('/zenith'), 'not in the sitemap');
    // GET /api/apps stays public; the builds need an app token (and, here, LSUITE_BUILDS_TOKEN).
    const list = await get('/api/apps');
    assert.equal(list.status, 200);
    assert.deepEqual((await list.json()).apps.map((a) => a.id), APP_NAMES);
    const signedOut = await get('/api/apps/kimchi/latest');
    assert.equal(signedOut.status, 401);
    assert.equal((await signedOut.json()).error.message, 'Sign in to lsuite to get the apps: the account is free.');
    let jar = '';
    const post = async (path, body) => {
      const res = await fetch(base + path, { method: 'POST', headers: { 'content-type': 'application/json', ...(jar ? { cookie: jar } : {}) }, body: JSON.stringify(body) });
      jar = res.headers.get('set-cookie')?.split(';')[0] ?? jar;
      return res;
    };
    await post('/api/account/session', { email: 'grace@example.com', name: 'Grace' });
    const { code } = await (await post('/api/account/connect', { app: 'lsuite' })).json();
    const { token } = await (await post('/api/account/token', { code })).json();
    assert.equal((await get('/api/apps/kimchi/latest', { cookie: jar })).status, 401, 'the site cookie is not an app token');
    const unset = await get('/api/apps/kimchi/latest', { authorization: `Bearer ${token}` });
    assert.equal(unset.status, 503);
    assert.deepEqual((await unset.json()).error, { type: 'api_error', message: "App downloads aren't set up on this server yet." });
  } finally {
    server.close();
  }
});

test('beta apps: every page says Beta and Linux, the nav lists all four, only macOS and Windows are coming soon', async () => {
  for (const app of ['ryolune', 'kimchi', 'nori', 'folio']) {
    const html = await renderPage(`${app}/index.html`, 'https://lsuite.xyz', { ryolune: '9.9.9', kimchi: '8.8.8', nori: '7.7.7' });
    assert.ok(html.includes('<span class="badge">Beta</span>'), app);
    assert.ok(!/class="badge">Coming soon/i.test(html), `${app}: not a coming-soon app`);
    assert.ok(html.includes('Linux, macOS and Windows coming soon'), `${app}: Linux, macOS and Windows coming soon`);
    assert.ok(!/\/Applications\//.test(html), `${app}: no macOS paths`);
    for (const other of ['ryolune', 'kimchi', 'nori', 'folio']) assert.ok(html.includes(`href="/${other}" data-app="${other}"`), `${app}: nav has ${other}`);
    assert.ok(html.includes('id="changelog"'), `${app}: changelog`);
    assert.ok(html.includes('id="ai"'), `${app}: lsuite AI`);
    assert.ok(html.includes('href="/pass"') && !html.includes('href="/ai'), `${app}: lsuite Pass, never /ai`);
  }
  const home = await renderPage('index.html', 'https://lsuite.xyz', { ryolune: '9.9.9', kimchi: '8.8.8', nori: '7.7.7' });
  for (const app of ['ryolune', 'kimchi', 'nori', 'folio']) assert.ok(home.includes(`class="card app-${app}`), `home card ${app}`);
  assert.ok(!/class="badge">Coming soon/i.test(home));
  assert.ok(!/· macOS|· Windows/.test(home), 'home cards: Linux only');
});

test('donations only go to https', () => {
  assert.equal(supportTarget('https://example.org/give'), 'https://example.org/give');
  assert.equal(supportTarget('javascript:alert(1)'), 'https://github.com/sponsors/ludovic111');
  assert.equal(supportTarget(undefined), 'https://github.com/sponsors/ludovic111');
});

test('pages are rendered with their includes and the current app', async () => {
  for (const app of ['ryolune', 'kimchi', 'nori', 'folio']) {
    const html = await renderPage(`${app}/index.html`, 'https://lsuite.xyz', { ryolune: '9.9.9', kimchi: '8.8.8', nori: '7.7.7' });
    assert.ok(!/%VERSION:/.test(html), `${app}: versions filled`);
    assert.ok(!html.includes('<!-- include:'), `${app}: includes filled`);
    assert.ok(html.includes(`data-app="${app}" aria-current="page"`), `${app}: current in nav`);
    assert.ok(!html.includes('%ORIGIN%'), `${app}: origin filled`);
    assert.match(html, /\/assets\/styles\.css\?v=[0-9a-f]{10}/);
  }
  const home = await renderPage('index.html', 'https://lsuite.xyz', { ryolune: '9.9.9', kimchi: '8.8.8', nori: '7.7.7' });
  assert.ok(!home.includes('aria-current'));
});

test('assets are served wherever the site lives, dotfiles inside it never', async () => {
  // A checkout under a dot folder (~/.t3/worktrees/…) still serves its assets.
  const server = createServer((req, res) => handle(req, res));
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  try {
    assert.equal((await fetch(`${base}/assets/styles.css`)).status, 200);
    assert.equal((await fetch(`${base}/assets/.hidden`)).status, 404);
    assert.equal((await fetch(`${base}/assets/fonts/.LICENSE.txt`)).status, 404);
  } finally {
    server.close();
  }
});

test('lsuite Pass and the account pages: plans from the API, scripts as files, flow steps unlisted', async () => {
  const pass = await renderPage('pass/index.html', 'https://lsuite.xyz', {});
  for (const plan of ['free', 'plus', 'pro', 'studio']) assert.ok(pass.includes(`data-plan="${plan}"`), plan);
  assert.ok(pass.includes('href="/account/checkout?plan=pro"'));
  assert.ok(pass.includes('data-app="pass" aria-current="page"'), 'Pass is current in the nav');
  assert.ok(pass.includes('<link rel="canonical" href="https://lsuite.xyz/pass">'));
  for (const part of ['lsuite AI', 'lsuite Cloud', 'lsuite Marketplace']) assert.ok(pass.includes(part), part);
  for (const logo of ['claude', 'openai', 'ollama', 'gemini']) assert.ok(pass.includes(`/assets/img/logos/${logo}.svg`), logo);
  // Each paid plan: credits, models, storage and the marketplace.
  const cards = plansHtml().split(/<div class="plan(?:"| plan--lit")/).slice(1);
  assert.equal(cards.length, 4);
  for (const card of cards.slice(1)) for (const item of ['credits</b> a month of lsuite AI', 'Claude', 'lsuite Cloud', 'The lsuite Marketplace']) assert.ok(card.includes(item), item);
  assert.ok(!cards[0].includes('The lsuite Marketplace'));
  for (const page of ['index.html', 'partials/nav.html', 'partials/foot.html', 'pages/launcher.html', 'account/index.html', 'account/checkout.html', 'account/connect.html']) {
    const html = page.startsWith('partials/') ? await (await import('node:fs/promises')).readFile(new URL(`../${page}`, import.meta.url), 'utf8') : await renderPage(page, 'https://lsuite.xyz', {});
    assert.ok(!/href="\/ai[#"]/.test(html), `${page}: links /pass, not /ai`);
  }
  for (const page of ['account/index.html', 'account/connect.html', 'account/checkout.html']) {
    const html = await renderPage(page, 'https://lsuite.xyz', {});
    assert.match(html, /<script src="\/assets\/account\.js\?v=[0-9a-f]{10}" defer><\/script>/, page);
    assert.ok(!/<script>(?!\s*$)|on(click|submit)=/i.test(html), `${page}: no inline script`);
    assert.ok(html.includes('noindex'), page);
  }
  const checkout = await renderPage('account/checkout.html', 'https://lsuite.xyz', {});
  assert.ok(checkout.includes('Demo — no payment is taken'));

  const server = createServer((req, res) => handle(req, res));
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const base = `http://127.0.0.1:${server.address().port}`;
    for (const path of ['/pass', '/marketplace', '/account', '/account/connect', '/account/checkout']) assert.equal((await fetch(base + path)).status, 200, path);
    for (const path of ['/ai', '/ai/', '/ai/index.html']) {
      const moved = await fetch(base + path, { redirect: 'manual' });
      assert.equal(moved.status, 301, path);
      assert.equal(moved.headers.get('location'), '/pass', path);
    }
    assert.equal((await fetch(`${base}/ai?ref=app`, { redirect: 'manual' })).headers.get('location'), '/pass?ref=app');
    assert.equal((await fetch(`${base}/pass/`, { redirect: 'manual' })).headers.get('location'), '/pass');
    const sitemap = await (await fetch(`${base}/sitemap.xml`)).text();
    assert.ok(sitemap.includes('/pass</loc>') && sitemap.includes('/marketplace</loc>') && sitemap.includes('/account</loc>'));
    assert.ok(!sitemap.includes('/ai</loc>'));
    assert.ok(!sitemap.includes('/account/connect') && !sitemap.includes('/account/checkout'));
    assert.match(await (await fetch(`${base}/robots.txt`)).text(), /Disallow: \/account\//);
  } finally {
    server.close();
  }
});

test('the launcher: its page, its downloads from launcher-v tags, and never one of the four apps', async (t) => {
  assert.deepEqual(APP_NAMES, ['ryolune', 'kimchi', 'nori', 'folio']);
  assert.equal(PAGES['/launcher'], 'pages/launcher.html');
  assert.equal(versionOf('launcher-v0.1.1', 'launcher-v'), '0.1.1');
  assert.equal(versionOf('v0.15.3'), '0.15.3');
  assert.equal(versionOf('v0.2.0', 'launcher-v'), '0.2.0');

  // GitHub as the launcher sees it: the repository's newest release isn't the launcher's.
  const real = globalThis.fetch;
  const asked = [];
  globalThis.fetch = async (url, init) => {
    if (!String(url).startsWith('https://api.github.com/')) return real(url, init);
    asked.push(String(url));
    const asset = (name, tag) => ({ browser_download_url: `https://github.com/ludovic111/lsuite/releases/download/${tag}/${name}` });
    const names = ['lsuite-macos-arm64.dmg', 'lsuite-macos-x86_64.dmg', 'lsuite-macos-arm64.app.tar.gz', 'lsuite-windows-x86_64-setup.exe', 'lsuite-windows-x86_64.zip', 'lsuite-linux-x86_64.AppImage', 'lsuite-linux-x86_64.tar.gz', 'latest.json', 'SHA256SUMS'];
    return Response.json([
      { tag_name: 'site-v9', draft: false, prerelease: false, assets: [] },
      { tag_name: 'launcher-v0.2.0', draft: true, prerelease: false, assets: [] },
      { tag_name: 'launcher-v0.1.1', draft: false, prerelease: false, assets: names.map((n) => asset(n, 'launcher-v0.1.1')) },
    ]);
  };
  t.after(() => (globalThis.fetch = real));
  assert.deepEqual(await appVersions(['launcher']), { launcher: '0.1.1' });
  assert.ok(asked[0].startsWith('https://api.github.com/repos/ludovic111/lsuite/releases?'));
  const base = 'https://github.com/ludovic111/lsuite/releases/download/launcher-v0.1.1/';
  // Beta: Linux only. macOS and Windows land on the page's downloads, which say "coming soon".
  for (const wanted of ['macos-arm64', 'macos-x86_64', 'windows-x86_64', 'windows-zip']) assert.equal(await downloadTarget('launcher', wanted), '/launcher#downloads', wanted);
  assert.equal(await downloadTarget('launcher', 'linux-x86_64'), `${base}lsuite-linux-x86_64.AppImage`);
  assert.equal(await downloadTarget('launcher', 'linux-tar'), `${base}lsuite-linux-x86_64.tar.gz`);
  assert.equal(await downloadTarget('launcher', undefined, 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'), '/launcher#downloads');
  assert.equal(await downloadTarget('launcher', undefined, 'Mozilla/5.0 (X11; Linux x86_64)'), `${base}lsuite-linux-x86_64.AppImage`);
  assert.equal(await downloadTarget('launcher', 'nope'), 'https://github.com/ludovic111/lsuite/releases');
  assert.equal(asked.length, 1, 'cached');

  const html = await renderPage('pages/launcher.html', 'https://lsuite.xyz');
  assert.ok(!/%VERSION:|%ORIGIN%|<!-- include:/.test(html));
  assert.ok(html.includes('Version 0.1.1'));
  assert.ok(html.includes('<link rel="canonical" href="https://lsuite.xyz/launcher">'));
  for (const platform of Object.keys(DOWNLOADS.launcher.patterns)) assert.ok(html.includes(`href="/launcher/download/${platform}"`), platform);
  for (const shot of ['apps', 'cloud', 'account']) {
    assert.ok(html.includes(`src="/assets/img/launcher/${shot}.webp" width="2000" height="1250"`), shot);
    assert.ok(html.includes(`srcset="/assets/img/launcher/${shot}-light.webp"`), shot);
  }
  assert.ok(!/<script>(?!\s*$)|<script(?![^>]*\bsrc=)(?![^>]*application\/ld\+json)[^>]*>|on(click|load|submit)=/i.test(html), 'no inline script');
  assert.ok(!/\bL[Ss][Uu][Ii][Tt][Ee]\b/.test(html.replace(/<[^>]*>/g, '')), 'lsuite in lowercase');
  const home = await renderPage('index.html', 'https://lsuite.xyz', {});
  assert.ok(home.includes('href="/launcher"') && home.includes('Get the lsuite launcher'));

  const server = createServer((req, res) => handle(req, res));
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const base = `http://127.0.0.1:${server.address().port}`;
    const get = (path) => fetch(base + path, { redirect: 'manual' });
    assert.equal((await get('/launcher')).status, 200);
    assert.equal((await get('/launcher/')).headers.get('location'), '/launcher');
    assert.equal((await get('/launcher/download/linux-tar')).headers.get('location'), `https://github.com/ludovic111/lsuite/releases/download/launcher-v0.1.1/lsuite-linux-x86_64.tar.gz`);
    // The Rust workspace next to the page is never served, nor the page's own file.
    for (const path of ['/launcher/README.md', '/launcher/Cargo.toml', '/launcher/brand/icon.png', '/launcher/crates/lsuite-core/src/cloud.rs', '/pages/launcher.html', '/launcher/support']) {
      assert.equal((await get(path)).status, 404, path);
    }
    assert.ok((await (await get('/sitemap.xml')).text()).includes('/launcher</loc>'));
    const { apps } = await (await get('/api/apps')).json();
    assert.deepEqual(apps.map((a) => a.id), APP_NAMES, 'no launcher among the apps');
  } finally {
    server.close();
  }
});

test('the marketplace page: approved plugins as cards, filtered by app without a script, an empty state', async () => {
  const html = await renderPage('marketplace/index.html', 'https://lsuite.xyz', {}, false, new URLSearchParams('app=kimchi'));
  assert.ok(!/<!-- include:|%ORIGIN%/.test(html));
  assert.ok(html.includes('data-app="marketplace" aria-current="page"'));
  assert.ok(html.includes('The first plugins are on their way — publish yours'));
  assert.ok(html.includes('href="/marketplace?app=kimchi#plugins" aria-current="page"'));
  for (const id of ['publish', 'pass', 'safety']) assert.ok(html.includes(`id="${id}"`), id);
  assert.ok(html.includes('market.publish path='));
  assert.ok(!/<script>(?!\s*$)|on(click|load|submit)=/i.test(html), 'no inline script');

  const listing = (over) => ({ id: 'com.example.tape-warmth', app: 'ryolune', name: 'Tape <warmth>', kind: 'effect', description: 'Saturation.', version: '1.2.0', abi: 1, author: { name: 'Ada', verified: false }, platforms: { 'macos-arm64': { size: 1, sha256: 'a' }, 'linux-x86_64': { size: 1, sha256: 'b' } }, downloads: 1234, updatedAt: '2026-10-07T12:00:00Z', notes: '', ...over });
  const cards = marketplaceHtml([listing(), listing({ id: 'xyz.lsuite.nori.duotone', app: 'nori', name: 'Duotone', author: { name: 'lsuite', verified: true }, platforms: { 'windows-x86_64': { size: 1, sha256: 'c' } } })]);
  assert.equal((cards.match(/<article class="plugin"/g) ?? []).length, 2);
  assert.ok(cards.includes('Tape &lt;warmth&gt;'), 'escaped');
  assert.ok(cards.includes('src="/assets/img/icons/ryolune.webp"') && cards.includes('src="/assets/img/icons/nori.webp"'));
  assert.ok(cards.includes('by Ada') && cards.includes('<span class="badge badge--sm">by lsuite</span>'));
  assert.ok(cards.includes('<dd>macOS Apple silicon, Linux</dd>') && cards.includes('<dd>Windows</dd>'));
  assert.ok(cards.includes('<dd>1,234</dd>') && cards.includes('<dd>1.2.0</dd>'));
  assert.ok(cards.includes('href="/marketplace#plugins" aria-current="page"'), 'every app');
  assert.ok(!cards.includes('The first plugins'));
});
