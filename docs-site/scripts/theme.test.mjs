import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

const source = readFileSync(new URL('../src/components/ThemeProvider.astro', import.meta.url), 'utf8').match(/<script is:inline>([\s\S]*?)<\/script>/)[1];
function run({ url = 'https://submilli.ai/docs/', stored, lightSystem = true, blocked = false } = {}) {
  const root = { dataset: {}, classList: { toggle(_, on) { this.light = on; } } };
  const button = { setAttribute(_, value) { this.label = value; } };
  const storage = new Map(stored ? [['starlight-theme', stored]] : []);
  const window = { location: { href: url }, matchMedia: () => ({ matches: lightSystem }), history: { state: { keep: true }, replaceState(state, _, value) { this.replaced = { state, url: value }; } } };
  vm.runInNewContext(source, { URL, window, document: { documentElement: root, querySelectorAll: () => [button] }, localStorage: {
    getItem(key) { if (blocked) throw Error('blocked'); return storage.get(key); },
    setItem(key, value) { if (blocked) throw Error('blocked'); storage.set(key, value); },
  } });
  return { window, root, button, storage };
}

test('dark website handoff overrides light OS and stored light preference before paint', () => {
  const h = run({ url: 'https://submilli.ai/docs/quickstart/?theme=dark&from=site#setup', stored: 'light' });
  assert.equal(h.root.dataset.theme, 'dark');
  assert.equal(h.root.classList.light, false);
  assert.equal(h.storage.get('starlight-theme'), 'dark');
  assert.equal(h.window.history.replaced.url, 'https://submilli.ai/docs/quickstart/?from=site#setup');
  assert.equal(h.window.history.replaced.state.keep, true);
  h.window.StarlightThemeProvider.setTheme('light');
  assert.equal(h.root.dataset.theme, 'light');
  assert.equal(h.root.classList.light, true);
  assert.equal(h.button.label, 'Switch to dark theme');
  assert.equal(h.storage.get('starlight-theme'), 'light');
});

test('direct docs visits preserve reader preferences and ignore invalid handoffs', () => {
  assert.equal(run({ stored: 'dark' }).root.dataset.theme, 'dark');
  assert.equal(run({ stored: 'light', lightSystem: false }).root.dataset.theme, 'light');
  assert.equal(run({ url: 'https://submilli.ai/docs/?theme=invalid', stored: 'dark' }).root.dataset.theme, 'dark');
  assert.equal(run().root.dataset.theme, 'light');
});

test('handoff and toggle still work when browser storage is blocked', () => {
  const h = run({ url: 'https://submilli.ai/docs/?theme=dark', blocked: true });
  assert.equal(h.root.dataset.theme, 'dark');
  h.window.StarlightThemeProvider.setTheme('light');
  assert.equal(h.root.dataset.theme, 'light');
});
