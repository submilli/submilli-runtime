import assert from 'node:assert/strict';
import test from 'node:test';
import { createDocsInvitationController } from '../src/lib/docs-invitation-client.ts';

function fixture({ storage = new Map(), session = new Map(), response = { ok: true, json: async () => ({ ok: true }) }, fetchImpl, invitation = true, overlay = false, mutationObserver } = {}) {
  let now = 0;
  let timeout;
  const requests = [];
  const listeners = {};
  const fields = {
    form: { addEventListener(name, fn) { this[name] = fn; } },
    input: { value: '', addEventListener(name, fn) { this[name] = fn; }, setCustomValidity(value) { this.error = value; }, reportValidity() { this.reported = true; } },
    submit: { disabled: false, hidden: false, textContent: '' },
    message: { textContent: '' },
    dismiss: { addEventListener(name, fn) { this[name] = fn; } },
  };
  const root = {
    hidden: true,
    dataset: { endpoint: 'https://forms.example.test/contact' },
    remove() { this.removed = true; },
    querySelector(selector) {
      return selector === 'form' ? fields.form : selector.includes('email') ? fields.input : selector.includes('submit') ? fields.submit : selector.includes('message') ? fields.message : fields.dismiss;
    },
  };
  const document = {
    visibilityState: 'visible',
    body: {},
    querySelector(selector) { return selector === '[data-docs-invitation]' ? (invitation ? root : undefined) : (overlay ? {} : undefined); },
    getElementById() { return undefined; },
    addEventListener(name, fn) { listeners[name] = fn; },
  };
  const storageApi = {
    getItem(key) { return storage.get(key) ?? null; },
    setItem(key, value) { storage.set(key, value); },
  };
  const sessionApi = {
    getItem(key) { return session.get(key) ?? null; },
    setItem(key, value) { session.set(key, value); },
  };
  const controller = createDocsInvitationController({
    document,
    fetch: async (_url, options) => { requests.push({ url: _url, options }); return fetchImpl ? fetchImpl(_url, options) : response; },
    localStorage: storageApi,
    sessionStorage: sessionApi,
    setTimeout(fn, delay) { timeout = { fn, delay }; return timeout; },
    clearTimeout() { timeout = undefined; },
    now: () => now,
    mutationObserver,
  });
  return { controller, root, fields, listeners, storage, session, document, requests, get timeout() { return timeout; }, advance(ms) { now += ms; } };
}

test('shows after ten seconds of visible browsing without taking focus', () => {
  const f = fixture();
  f.controller.bind();
  assert.equal(f.timeout.delay, 10_000);
  assert.equal(f.root.hidden, true);
  f.advance(10_000); f.timeout.fn();
  assert.equal(f.root.hidden, false);
  assert.equal(f.fields.input.reported, undefined);
});

test('pauses while hidden and resumes with remaining visible time', () => {
  const f = fixture();
  f.controller.bind(); f.advance(4_000);
  f.document.visibilityState = 'hidden';
  f.controller.onVisibilityChange();
  assert.equal(f.timeout, undefined);
  f.document.visibilityState = 'visible';
  f.controller.onVisibilityChange();
  assert.equal(f.timeout.delay, 6_000);
  f.advance(6_000); f.timeout.fn();
  assert.equal(f.root.hidden, false);
});

test('a restored document resumes its earned time after navigation', () => {
  const f = fixture();
  f.controller.bind();
  f.advance(4_000);
  f.controller.pauseForNavigation();
  assert.equal(f.timeout, undefined);
  f.controller.bind();
  assert.equal(f.timeout.delay, 6_000);
  f.advance(6_000); f.timeout.fn();
  assert.equal(f.root.hidden, false);
});

test('carries visible time across route bindings and reveals an already-earned invitation', () => {
  const session = new Map();
  const first = fixture({ session });
  first.controller.bind(); first.advance(6_000); first.document.visibilityState = 'hidden'; first.controller.onVisibilityChange();
  assert.equal(session.get('submilli.docs-invitation.elapsed.v1'), '6000');
  const second = fixture({ session });
  second.controller.bind(); second.advance(4_000); second.document.visibilityState = 'hidden'; second.controller.onVisibilityChange();
  assert.equal(second.root.hidden, true);
  second.document.visibilityState = 'visible'; second.controller.onVisibilityChange();
  assert.equal(second.root.hidden, false);
});

test('pauses and defers on routes or overlays where the invitation cannot be shown', () => {
  const hiddenRoute = fixture({ invitation: false });
  hiddenRoute.controller.bind(); hiddenRoute.advance(20_000); hiddenRoute.controller.onVisibilityChange();
  assert.equal(hiddenRoute.root.hidden, true);

  const session = new Map([['submilli.docs-invitation.elapsed.v1', '10000']]);
  const covered = fixture({ session, overlay: true });
  covered.controller.bind();
  assert.equal(covered.root.hidden, true);
  covered.document.querySelector = selector => selector === '[data-docs-invitation]' ? covered.root : undefined;
  covered.controller.onVisibilityChange();
  assert.equal(covered.root.hidden, false);
});

test('can defer through a second overlay cycle after the first one clears', () => {
  const observers = [];
  class FakeObserver {
    constructor(callback) { this.callback = callback; observers.push(this); }
    observe() {}
    disconnect() { this.disconnected = true; }
  }
  const session = new Map([['submilli.docs-invitation.elapsed.v1', '10000']]);
  const covered = fixture({ session, overlay: true, mutationObserver: FakeObserver });
  covered.controller.bind();
  assert.equal(observers.length, 1);
  covered.document.querySelector = selector => selector === '[data-docs-invitation]' ? covered.root : undefined;
  observers[0].callback();
  assert.equal(covered.root.hidden, false);
  covered.root.hidden = true;
  covered.document.querySelector = selector => selector === '[data-docs-invitation]' ? covered.root : {};
  covered.controller.bind();
  assert.equal(observers.length, 2);
  covered.document.querySelector = selector => selector === '[data-docs-invitation]' ? covered.root : undefined;
  observers[1].callback();
  assert.equal(covered.root.hidden, false);
});

test('dismissal is remembered and blocked storage does not throw', () => {
  const storage = { getItem() { throw new Error('blocked'); }, setItem() { throw new Error('blocked'); } };
  const f = fixture();
  const blocked = createDocsInvitationController({ document: f.document, fetch: async () => ({ ok: true, json: async () => ({ ok: true }) }), localStorage: storage, setTimeout() { return undefined; } });
  assert.doesNotThrow(() => blocked.bind());
  const normal = fixture();
  normal.controller.bind(); normal.fields.dismiss.click();
  assert.equal(normal.root.removed, true);
  assert.equal(normal.storage.get('submilli.docs-invitation.dismissed.v1'), '1');
  const revisit = fixture({ storage: normal.storage });
  revisit.controller.bind();
  assert.equal(revisit.root.removed, true);
});

test('validates email and only shows receipt for a 2xx {ok:true} response', async () => {
  const f = fixture({ response: { ok: true, json: async () => ({ ok: false }) } });
  f.controller.bind(); f.fields.form.submit({ preventDefault() {} });
  assert.equal(f.fields.input.reported, true);
  f.fields.input.value = 'reader@example.com';
  await f.fields.form.submit({ preventDefault() {} });
  assert.match(f.fields.message.textContent, /couldn’t save/);
  assert.equal(f.fields.submit.hidden, false);

  const success = fixture({ response: { ok: true, json: async () => ({ ok: true }) } });
  success.controller.bind(); success.fields.input.value = 'reader@example.com';
  await success.fields.form.submit({ preventDefault() {} });
  assert.match(success.fields.message.textContent, /Thanks/);
  assert.equal(success.fields.form.hidden, true);
  assert.deepEqual(JSON.parse(success.requests[0].options.body), { email: 'reader@example.com', form_id: 'docs-invitation' });
  assert.equal(success.storage.get('submilli.docs-invitation.dismissed.v1'), '1');
  const returnVisit = fixture({ storage: success.storage });
  returnVisit.controller.bind();
  assert.equal(returnVisit.root.removed, true);
});

test('clears corrected invalid input and recovers from a timed out request without a false receipt', async () => {
  const f = fixture({ fetchImpl: (_url, options) => new Promise((_resolve, reject) => options.signal.addEventListener('abort', () => reject(new Error('aborted')))) });
  f.controller.bind();
  f.fields.form.submit({ preventDefault() {} });
  f.fields.input.value = 'reader@example.com';
  f.fields.input.input();
  assert.equal(f.fields.input.error, '');
  const submission = f.fields.form.submit({ preventDefault() {} });
  f.timeout.fn();
  await submission;
  assert.match(f.fields.message.textContent, /couldn’t save/);
  assert.equal(f.fields.submit.hidden, false);
  assert.equal(f.storage.get('submilli.docs-invitation.dismissed.v1'), undefined);
});

test('navigation aborts the old request without cancelling a later request', async () => {
  const pending = [];
  const f = fixture({ fetchImpl: (_url, options) => new Promise((resolve, reject) => pending.push({ resolve, reject, signal: options.signal })) });
  f.controller.bind();
  f.fields.input.value = 'old@example.com';
  const oldSubmission = f.fields.form.submit({ preventDefault() {} });
  const nextRoot = { ...f.root, dataset: { endpoint: f.root.dataset.endpoint }, remove() { this.removed = true; } };
  f.document.querySelector = selector => selector === '[data-docs-invitation]' ? nextRoot : undefined;
  f.controller.pauseForNavigation();
  f.controller.bind();
  f.fields.submit.disabled = false;
  f.fields.input.value = 'new@example.com';
  const newSubmission = f.fields.form.submit({ preventDefault() {} });
  assert.equal(pending.length, 2);
  pending[0].resolve({ ok: true, json: async () => ({ ok: true }) });
  await oldSubmission;
  await Promise.resolve();
  assert.equal(f.fields.form.hidden, undefined);
  assert.equal(f.fields.submit.disabled, true);
  pending[1].resolve({ ok: true, json: async () => ({ ok: true }) });
  await newSubmission;
  assert.match(f.fields.message.textContent, /Thanks/);
});
