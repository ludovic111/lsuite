import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { hostRedirect, osFor, supportTarget, downloadTarget, renderPage, handle, appVersions, versionOf, DOWNLOADS, APP_NAMES, PAGES } from '../server.js';

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
  assert.equal(hostRedirect('lsuite.xyz', '/zenith', 'lsuite.xyz'), null);
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

test('download routes of every released app point to GitHub', async () => {
  for (const app of ['ryolune', 'kimchi', 'zenith', 'nori', 'folio']) {
    assert.equal(await downloadTarget(app, 'nope'), `https://github.com/ludovic111/${app}/releases/latest`, app);
    assert.equal(DOWNLOADS[app].repo, `ludovic111/${app}`);
    assert.notEqual(DOWNLOADS[app].published, false);
    const html = await renderPage(`${app}/index.html`, 'https://lsuite.xyz', {});
    assert.ok(html.includes(`href="/${app}/download`), `${app}: public download link`);
    assert.ok(!html.includes('First public build coming'), app);
  }
  assert.equal(await downloadTarget('photoshop', 'macos-arm64'), null);
});

test('beta apps: every page says Beta, the nav lists all five, nothing says coming soon', async () => {
  for (const app of ['ryolune', 'kimchi', 'zenith', 'nori', 'folio']) {
    const html = await renderPage(`${app}/index.html`, 'https://lsuite.xyz', { ryolune: '9.9.9', kimchi: '8.8.8', zenith: '7.7.7' });
    assert.ok(html.includes('<span class="badge">Beta</span>'), app);
    assert.ok(!/coming soon/i.test(html), `${app}: no "coming soon"`);
    for (const other of ['ryolune', 'kimchi', 'zenith', 'nori', 'folio']) assert.ok(html.includes(`href="/${other}" data-app="${other}"`), `${app}: nav has ${other}`);
    assert.ok(html.includes('id="changelog"'), `${app}: changelog`);
    assert.ok(html.includes('id="ai"') || app === 'zenith', `${app}: lsuite AI`);
  }
  const home = await renderPage('index.html', 'https://lsuite.xyz', { ryolune: '9.9.9', kimchi: '8.8.8', zenith: '7.7.7' });
  for (const app of ['ryolune', 'kimchi', 'zenith', 'nori', 'folio']) assert.ok(home.includes(`class="card app-${app}`), `home card ${app}`);
  assert.ok(!/coming soon/i.test(home));
});

test('donations only go to https', () => {
  assert.equal(supportTarget('https://example.org/give'), 'https://example.org/give');
  assert.equal(supportTarget('javascript:alert(1)'), 'https://github.com/sponsors/ludovic111');
  assert.equal(supportTarget(undefined), 'https://github.com/sponsors/ludovic111');
});

test('pages are rendered with their includes and the current app', async () => {
  for (const app of ['ryolune', 'kimchi', 'zenith', 'nori', 'folio']) {
    const html = await renderPage(`${app}/index.html`, 'https://lsuite.xyz', { ryolune: '9.9.9', kimchi: '8.8.8', zenith: '7.7.7' });
    assert.ok(!/%VERSION:/.test(html), `${app}: versions filled`);
    assert.ok(!html.includes('<!-- include:'), `${app}: includes filled`);
    assert.ok(html.includes(`data-app="${app}" aria-current="page"`), `${app}: current in nav`);
    assert.ok(!html.includes('%ORIGIN%'), `${app}: origin filled`);
    assert.match(html, /\/assets\/styles\.css\?v=[0-9a-f]{10}/);
  }
  const home = await renderPage('index.html', 'https://lsuite.xyz', { ryolune: '9.9.9', kimchi: '8.8.8', zenith: '7.7.7' });
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

test('lsuite AI and the account pages: plans from the API, scripts as files, flow steps unlisted', async () => {
  const ai = await renderPage('ai/index.html', 'https://lsuite.xyz', {});
  for (const plan of ['free', 'plus', 'pro', 'studio']) assert.ok(ai.includes(`data-plan="${plan}"`), plan);
  assert.ok(ai.includes('href="/account/checkout?plan=pro"'));
  assert.ok(ai.includes('data-app="ai" aria-current="page"'), 'AI is current in the nav');
  for (const logo of ['claude', 'openai', 'ollama', 'gemini']) assert.ok(ai.includes(`/assets/img/logos/${logo}.svg`), logo);
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
    for (const path of ['/ai', '/account', '/account/connect', '/account/checkout']) assert.equal((await fetch(base + path)).status, 200, path);
    const sitemap = await (await fetch(`${base}/sitemap.xml`)).text();
    assert.ok(sitemap.includes('/ai</loc>') && sitemap.includes('/account</loc>'));
    assert.ok(!sitemap.includes('/account/connect') && !sitemap.includes('/account/checkout'));
    assert.match(await (await fetch(`${base}/robots.txt`)).text(), /Disallow: \/account\//);
  } finally {
    server.close();
  }
});

test('the launcher: its page, its downloads from launcher-v tags, and never one of the five apps', async (t) => {
  assert.deepEqual(APP_NAMES, ['ryolune', 'kimchi', 'zenith', 'nori', 'folio']);
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
  assert.equal(await downloadTarget('launcher', 'macos-arm64'), `${base}lsuite-macos-arm64.dmg`);
  assert.equal(await downloadTarget('launcher', 'macos-x86_64'), `${base}lsuite-macos-x86_64.dmg`);
  assert.equal(await downloadTarget('launcher', 'windows-x86_64'), `${base}lsuite-windows-x86_64-setup.exe`);
  assert.equal(await downloadTarget('launcher', 'windows-zip'), `${base}lsuite-windows-x86_64.zip`);
  assert.equal(await downloadTarget('launcher', 'linux-x86_64'), `${base}lsuite-linux-x86_64.AppImage`);
  assert.equal(await downloadTarget('launcher', 'linux-tar'), `${base}lsuite-linux-x86_64.tar.gz`);
  assert.equal(await downloadTarget('launcher', undefined, 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'), `${base}lsuite-windows-x86_64-setup.exe`);
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
