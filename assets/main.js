// lsuite — page behaviour. Loaded in <head> without defer so `html.js` is set before first paint
// (reveals only hide content once script is known to run); the rest waits for the DOM.
document.documentElement.classList.add('js');

// The download routes the server knows, by platform (see server.js).
function detectPlatform() {
  const ua = navigator.userAgent;
  if (/Android|iPhone|iPad/i.test(ua)) return null;
  if (/Windows/i.test(ua)) return 'windows';
  if (/Mac OS X|Macintosh/i.test(ua)) return 'macos';
  if (/Linux|X11/i.test(ua)) return 'linux';
  return null;
}
const OS_NAME = { macos: 'macOS', windows: 'Windows', linux: 'Linux' };

function ready() {
  // Scroll reveals, once each. Wired first so a later error cannot leave the page hidden.
  const reveals = document.querySelectorAll('.reveal');
  if ('IntersectionObserver' in window) {
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            e.target.classList.add('in');
            io.unobserve(e.target);
          }
        }
      },
      { rootMargin: '0px 0px -8% 0px', threshold: 0.08 },
    );
    reveals.forEach((el) => io.observe(el));
  } else {
    reveals.forEach((el) => el.classList.add('in'));
  }

  // "Download for <your OS>": buttons carry data-download="<app>" and per-OS hrefs. An OS with no
  // href is coming soon (lsuite's beta is Linux only): the button says so and points at #downloads.
  const os = detectPlatform();
  if (os) {
    document.querySelectorAll('[data-download]').forEach((a) => {
      const href = a.getAttribute(`data-href-${os}`);
      a.setAttribute('href', href || '#downloads');
      const label = a.querySelector('[data-os]');
      if (label) label.textContent = href ? `Download for ${OS_NAME[os]}` : `Coming soon on ${OS_NAME[os]}`;
    });
    document.querySelectorAll(`.dl a[data-os-match~="${os}"]`).forEach((a, i) => {
      if (i === 0) a.classList.add('is-you');
    });
  }

  // Copy buttons on code blocks.
  document.querySelectorAll('.code').forEach((block) => {
    const pre = block.querySelector('pre');
    if (!pre || !navigator.clipboard) return;
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'code__copy';
    btn.textContent = 'Copy';
    btn.addEventListener('click', async () => {
      const text = [...pre.querySelectorAll('.line')].map((l) => l.textContent).join('\n') || pre.textContent;
      try {
        await navigator.clipboard.writeText(text.trim());
        btn.textContent = 'Copied';
        btn.classList.add('done');
        setTimeout(() => {
          btn.textContent = 'Copy';
          btn.classList.remove('done');
        }, 1600);
      } catch {}
    });
    block.append(btn);
  });

  // Theme gallery (ryolune page): tab lists keyed by `data-key` (`mode`, and `theme` if the app
  // has several). The picture is `theme-<theme>-<mode>.webp`; the caption is the selected tab's
  // `data-desc` (the theme tab's when there is one, else the mode tab's).
  const gallery = document.querySelector('[data-gallery]');
  if (gallery) {
    const img = gallery.querySelector('img');
    const shot = gallery.querySelector('.gallery__shot');
    const desc = gallery.querySelector('.gallery__desc');
    const state = { theme: gallery.dataset.theme || 'ryolune', mode: 'dark' };
    const show = () => {
      const src = `/assets/img/ryolune/theme-${state.theme}-${state.mode}.webp`;
      const tab =
        gallery.querySelector(`[data-key="theme"] [data-theme="${state.theme}"]`) ||
        gallery.querySelector(`[data-key="mode"] [data-mode="${state.mode}"]`);
      if (desc && tab?.dataset.desc) desc.innerHTML = tab.dataset.desc;
      if (img.getAttribute('src') === src) return;
      shot.classList.add('is-loading');
      const next = new Image();
      next.onload = next.onerror = () => {
        img.src = src;
        img.alt = `ryolune in ${state.mode} mode`;
        shot.classList.remove('is-loading');
      };
      next.src = src;
    };
    gallery.querySelectorAll('[role="tablist"]').forEach((list) => {
      const key = list.dataset.key;
      const tabs = [...list.querySelectorAll('[role="tab"]')];
      const select = (tab) => {
        tabs.forEach((t) => {
          const on = t === tab;
          t.setAttribute('aria-selected', String(on));
          t.tabIndex = on ? 0 : -1;
        });
        state[key] = tab.dataset[key];
        show();
      };
      tabs.forEach((tab, i) => {
        tab.addEventListener('click', () => select(tab));
        tab.addEventListener('keydown', (e) => {
          const step = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
          if (!step) return;
          e.preventDefault();
          const next = tabs[(i + step + tabs.length) % tabs.length];
          next.focus();
          select(next);
        });
      });
    });
    show();
  }
}

if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', ready);
else ready();
