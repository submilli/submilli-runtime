import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import { attributionFor, mergeAttribution, safeUrl } from '../src/lib/analytics-utils.ts';

function browser({ consent, optedOut = false, search = '', session = new Map() } = {}) {
  const storage = new Map(consent ? [['submilli.analytics-consent.v1', consent]] : []);
  const events = [], listeners = {}, buttons = {};
  const location = { href: `https://submilli.ai/docs/${search}`, pathname: '/docs/', search, host: 'submilli.ai' };
  const storageApi = values => ({ getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: key => values.delete(key) });
  let options, initCount = 0;
  const posthog = {
    init(_key, value) { options = value; initCount++; },
    has_opted_out_capturing: () => optedOut,
    opt_in_capturing() { optedOut = false; },
    opt_out_capturing() { optedOut = true; },
    register() {},
    capture(event, properties) { events.push(options.before_send({ event, properties })); },
  };
  class Element { closest() { return this; } }
  const document = {
    referrer: '',
    getElementById: () => ({ hidden: true, addEventListener() {} }),
    querySelectorAll: () => ['granted', 'denied'].map(consent => ({ dataset: { consent }, addEventListener(_event, fn) { buttons[consent] = fn; } })),
    addEventListener(name, fn) { listeners[name] = fn; },
  };
  const source = readFileSync(new URL('../src/lib/analytics-client.ts', import.meta.url), 'utf8')
    .replace(/^import .*;\n/gm, '').replaceAll('import.meta.env.PUBLIC_POSTHOG_KEY', "'test-key'").replaceAll('import.meta.env.PUBLIC_POSTHOG_HOST', "'https://test.invalid'");
  const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  vm.runInNewContext(js, { posthog, attributionFor, mergeAttribution, safeUrl, location, document, localStorage: storageApi(storage), sessionStorage: storageApi(session), URL, URLSearchParams, Element });
  return { events, buttons, listeners, location, options, get initCount() { return initCount; }, click(cta) { const target = new Element(); target.href = 'https://calendar.google.com/book?private=value'; target.dataset = { analyticsCta: cta }; listeners.click({ target }); } };
}

test('default tracking deduplicates route notification and emits next route', () => {
  const b = browser();
  b.listeners['astro:page-load']();
  assert.equal(b.events.length, 1);
  b.location.href = 'https://submilli.ai/docs/quickstart?private=value'; b.location.pathname = '/docs/quickstart';
  b.listeners['astro:page-load']();
  assert.equal(b.events.length, 2);
  assert.equal(b.events[1].properties.$current_url, 'https://submilli.ai/docs/quickstart');
  assert.equal(b.options.disable_session_recording, true);
});

test('honors saved and SDK opt-out, and explicit opt-in resumes capture', () => {
  for (const initial of [{ consent: 'denied', optedOut: true }, { optedOut: true }]) {
    const b = browser(initial);
    assert.equal(b.events.length, 0);
    b.buttons.granted();
    assert.equal(b.events.length, 1);
    b.buttons.denied(); b.click('meeting');
    assert.equal(b.events.length, 1);
    assert.equal(b.initCount, 1);
  }
});

test('meeting click retains docs and canonical booking contracts', () => {
  const b = browser(); b.click('meeting');
  assert.deepEqual(b.events.map(event => event.event), ['$pageview', 'link_clicked', 'docs_cta_clicked', 'booking_cta_clicked']);
  const booking = b.events.at(-1).properties;
  assert.equal(booking.cta_location, 'docs');
  assert.equal(booking.booking_provider, 'google_calendar');
  assert.equal(booking.destination, 'https://calendar.google.com/book');
});

test('test marker survives full page navigation in the same tab', () => {
  const session = new Map(); browser({ search: '?analytics_test=1', session });
  const next = browser({ session }); next.click('github');
  assert.ok(next.events.every(event => event.properties.is_test === true));
  assert.equal(browser().events[0].properties.is_test, false);
});
