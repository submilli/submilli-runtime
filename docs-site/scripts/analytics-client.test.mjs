import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import { attributionFor, mergeAttribution, safeUrl } from '../src/lib/analytics-utils.ts';

function browser({ consent, optedOut = false, search = '', session = new Map(), hostname = 'submilli.ai', storageFailure = false, attribution, cookieStore = { value: '' } } = {}) {
  const storage = new Map(consent ? [['submilli.analytics-consent.v1', consent]] : []);
  if (attribution) storage.set('submilli.attribution.v1', JSON.stringify(attribution));
  const events = [], listeners = {}, buttons = {};
  const location = { href: `https://${hostname}/docs/${search}`, pathname: '/docs/', search, host: hostname, hostname };
  const storageApi = values => ({ getItem: key => { if (storageFailure) throw new Error('blocked'); return values.get(key) ?? null; }, setItem: (key, value) => { if (storageFailure) throw new Error('blocked'); values.set(key, value); }, removeItem: key => values.delete(key) });
  let options, initCount = 0, stopped = 0;
  const posthog = {
    init(_key, value) { options = value; initCount++; },
    has_opted_out_capturing: () => optedOut,
    opt_in_capturing() { optedOut = false; },
    opt_out_capturing() { optedOut = true; },
    stopSessionRecording() { stopped++; },
    register() {},
    capture(event, properties) { events.push(options.before_send({ event, properties })); },
  };
  class Element { closest() { return this; } }
  const document = {
    referrer: '',
    getElementById: () => ({ hidden: true, addEventListener() {} }),
    querySelectorAll: () => ['granted', 'denied'].map(consent => ({ dataset: { consent }, addEventListener(_event, fn) { buttons[consent] = fn; } })),
    addEventListener(name, fn) { listeners[name] = fn; },
    get cookie() { return cookieStore.value; },
    set cookie(value) { cookieStore.value = value.split(';', 1)[0]; },
  };
  const source = readFileSync(new URL('../src/lib/analytics-client.ts', import.meta.url), 'utf8')
    .replace(/^import .*;\n/gm, '').replaceAll('import.meta.env.PUBLIC_POSTHOG_KEY', "'test-key'").replaceAll('import.meta.env.PUBLIC_POSTHOG_HOST', "'https://test.invalid'");
  const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const window = { addEventListener(name, fn) { listeners[name] = fn; } };
  vm.runInNewContext(js, { posthog, attributionFor, mergeAttribution, safeUrl, location, document, window, localStorage: storageApi(storage), sessionStorage: storageApi(session), URL, URLSearchParams, Element });
  return { events, buttons, listeners, location, options, storage, get stopped() { return stopped; }, get initCount() { return initCount; }, click(cta, analyticsLocation) { const target = new Element(); target.href = 'https://calendar.google.com/book?private=value'; target.dataset = { analyticsCta: cta, analyticsLocation }; listeners.click({ target }); } };
}

test('default tracking deduplicates route notification and emits next route', () => {
  const b = browser();
  b.listeners['astro:page-load']();
  assert.equal(b.events.length, 1);
  b.location.href = 'https://submilli.ai/docs/quickstart?private=value'; b.location.pathname = '/docs/quickstart';
  b.listeners['astro:page-load']();
  assert.equal(b.events.length, 2);
  assert.equal(b.events[1].properties.$current_url, 'https://submilli.ai/docs/quickstart');
  assert.equal(b.options.disable_session_recording, false);
  assert.equal(b.options.session_recording.maskAllInputs, false);
  assert.equal(b.options.session_recording.maskInputOptions.password, false);
  assert.equal(b.options.session_recording.maskTextClass, '');
  assert.equal(b.options.session_recording.maskTextSelector, '');
  assert.equal(b.options.session_recording.blockClass, '');
  assert.equal(b.options.session_recording.blockSelector, '');
  assert.equal(b.options.session_recording.recordHeaders, false);
  assert.equal(b.options.session_recording.recordBody, false);
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

test('invitation booking click is attributed to its actual location', () => {
  const b = browser(); b.click('meeting', 'docs-invitation');
  assert.equal(b.events.at(-1).properties.cta_location, 'docs-invitation');
});

test('team opt-out link stops initialization even when storage is unavailable', () => {
  const b = browser({ search: '?analytics=off' });
  assert.equal(b.initCount, 0);
  assert.equal(b.storage.get('submilli.analytics-consent.v1'), 'denied');
  assert.equal(b.events.length, 0);
  assert.equal(browser({ search: '?analytics=off', storageFailure: true }).initCount, 0);
});

test('inline query opt-out writes before analytics dependencies load', () => {
	const source = readFileSync(new URL('../src/components/Analytics.astro', import.meta.url), 'utf8');
	const inline = source.match(/<script is:inline>([\s\S]*?)<\/script>/)?.[1];
	const store = new Map();
	const cookies = { value: '' };
	vm.runInNewContext(inline, {
		URL,
		location: new URL('https://submilli.ai/docs/?analytics=off'),
		localStorage: { setItem: (key, value) => store.set(key, value) },
		document: { get cookie() { return cookies.value; }, set cookie(value) { cookies.value = value.split(';', 1)[0]; } },
	});
	assert.equal(store.get('submilli.analytics-consent.v1'), 'denied');
	assert.equal(cookies.value, 'submilli_analytics_consent=denied');
});

test('cookie denial survives a fresh context when localStorage is unavailable', () => {
	const cookieStore = { value: '' };
	const first = browser({ search: '?analytics=off', storageFailure: true, cookieStore });
	assert.equal(first.initCount, 0);
	assert.equal(cookieStore.value, 'submilli_analytics_consent=denied');
	const restarted = browser({ storageFailure: true, cookieStore });
	assert.equal(restarted.initCount, 0);
});

test('explicit docs opt-in clears cookie denial', () => {
	const cookieStore = { value: 'submilli_analytics_consent=denied' };
	const b = browser({ consent: 'denied', cookieStore });
	b.buttons.granted();
	assert.equal(cookieStore.value, 'submilli_analytics_consent=');
	assert.equal(b.initCount, 1);
});

test('recordings are disabled on previews and marked verification sessions', () => {
  for (const options of [{hostname:'localhost'}, {hostname:'preview.onrender.com'}, {search:'?analytics_test=1'}, {search:'?utm_source=verification'}, {attribution:{first:{utm_campaign:'deployment_verification'}}}, {attribution:{latest:{utm_content:'utm_e2e'}}}]) {
    assert.equal(browser(options).options.disable_session_recording, true);
  }
});

test('shared opt-out immediately stops recording and event capture', () => {
  const b = browser();
  b.listeners.storage({key:'submilli.analytics-consent.v1',newValue:'denied'});
  b.click('meeting');
  assert.equal(b.events.length, 1);
  assert.equal(b.stopped, 1);
});

test('test marker survives full page navigation in the same tab', () => {
  const session = new Map(); browser({ search: '?analytics_test=1', session });
  const next = browser({ session }); next.click('github');
  assert.ok(next.events.every(event => event.properties.is_test === true));
  assert.equal(browser().events[0].properties.is_test, false);
});
