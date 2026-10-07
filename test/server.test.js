import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { hostRedirect, osFor, supportTarget, downloadTarget, renderPage, handle, DOWNLOADS } from '../server.js';

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

test('download routes of every app: released apps to GitHub, unreleased ones to their pages', async () => {
  for (const app of ['ryolune', 'kimchi', 'zenith', 'folio']) {
    assert.equal(await downloadTarget(app, 'nope'), `https://github.com/ludovic111/${app}/releases/latest`, app);
  }
  for (const app of ['nori']) {
    for (const platform of [undefined, 'macos-arm64', 'windows-x86_64', 'linux-x86_64', 'nope']) {
      assert.equal(await downloadTarget(app, platform, 'Macintosh'), `/${app}`);
    }
    const html = await renderPage(`${app}/index.html`, 'https://lsuite.xyz', {});
    assert.ok(html.includes('First public build coming'), app);
    assert.ok(!/data-download|softwareVersion|%VERSION/.test(html), `${app}: no version, no download yet`);
  }
  // The route table is ready for each app's repository.
  for (const app of ['ryolune', 'kimchi', 'zenith', 'nori', 'folio']) assert.equal(DOWNLOADS[app].repo, `ludovic111/${app}`);
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
