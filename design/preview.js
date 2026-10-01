// Design preview: switch the app (signature color) and the mode, remembered per viewer.
const root = document.documentElement;
function set(key, value) {
  root.dataset[key] = value;
  document.querySelectorAll(`[data-set-${key}]`).forEach((b) => b.setAttribute('aria-pressed', String(b.dataset[`set${key[0].toUpperCase()}${key.slice(1)}`] === value)));
  if (key === 'app') document.querySelectorAll('[data-app-name]').forEach((n) => (n.textContent = value));
  try { localStorage.setItem(`ls-design-${key}`, value); } catch {}
}
for (const key of ['app', 'mode']) {
  let saved = null;
  try { saved = localStorage.getItem(`ls-design-${key}`); } catch {}
  if (saved) set(key, saved);
  document.querySelectorAll(`[data-set-${key}]`).forEach((b) => b.addEventListener('click', () => set(key, b.getAttribute(`data-set-${key}`))));
}
