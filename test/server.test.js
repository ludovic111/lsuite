import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { hostRedirect, osFor, supportTarget, downloadTarget, renderPage, handle } from '../server.js';

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

test('unknown kimchi platforms land on its public releases', async () => {
  assert.equal(await downloadTarget('kimchi', 'nope'), 'https://github.com/ludovic111/kimchi/releases/latest');
});

test('coming-soon apps send old downloads to their empty pages without private repository links', async () => {
  for (const app of ['ryolune', 'zenith']) {
    for (const platform of [undefined, 'macos-arm64', 'macos-x86_64', 'windows-x86_64', 'linux-x86_64', 'nope']) {
      assert.equal(await downloadTarget(app, platform, 'Macintosh'), `/${app}`);
    }
    const html = await renderPage(`${app}/index.html`, 'https://lsuite.xyz', {});
    assert.ok(html.includes('Coming soon'));
    // Empty until the app wears design v2: its name and status, no summary and no logo.
    assert.equal([...html.matchAll(/<p\b/g)].length, 0, `${app}: an empty page`);
    assert.ok(!html.includes('appid__mark'), `${app}: no logo`);
    assert.ok(!/\/download|data-download|softwareVersion|downloadUrl/.test(html));
  }
  for (const page of ['index.html', 'kimchi/index.html', 'ryolune/index.html', 'zenith/index.html']) {
    const html = await renderPage(page, 'https://lsuite.xyz', { kimchi: '8.8.8' });
    assert.ok(!/github\.com\/ludovic111\/(ryolune|zenith)|\/(ryolune|zenith)\/download/.test(html), page);
  }
});

test('donations only go to https', () => {
  assert.equal(supportTarget('https://example.org/give'), 'https://example.org/give');
  assert.equal(supportTarget('javascript:alert(1)'), 'https://github.com/sponsors/ludovic111');
  assert.equal(supportTarget(undefined), 'https://github.com/sponsors/ludovic111');
});

test('pages are rendered with their includes and the current app', async () => {
  for (const app of ['ryolune', 'kimchi', 'zenith']) {
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
