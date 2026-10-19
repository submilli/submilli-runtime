import assert from 'node:assert/strict';
import test from 'node:test';
import { createDocsInvitationController } from '../src/lib/docs-invitation-client.ts';

function fixture({ storage = new Map(), session = new Map(), response = { ok: true, json: async () => ({ ok: true }) }, fetchImpl, invitation = true, overlay = false, mutationObserver, arrivedViaDocsLink = true, baseURI = 'https://docs.example.test/docs/quickstart/', initialNow = 0 } = {}) {
  if (arrivedViaDocsLink) session.set('submilli.docs-invitation.navigation.v1', JSON.stringify({ path: new URL(baseURI).pathname.replace(/\/$/, ''), createdAt: initialNow, elapsed: 0 }));
  let now = initialNow;
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
    baseURI,
    scrollingElement: { scrollTop: 0 },
    body: { append(node) { node.parentElement = this; } },
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
    removeItem(key) { session.delete(key); },
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

function docsLink(href, attributes = {}) {
  const link = { getAttribute(name) { return name === 'href' ? href : attributes[name] ?? null; }, hasAttribute(name) { return name in attributes; } };
  return { closest(selector) { return selector === 'a[href]' ? link : undefined; } };
}

test('shows after ten seconds of engaged visible browsing without taking focus', () => {
  const f = fixture();
  f.controller.bind();
  assert.equal(f.timeout.delay, 10_000);
  assert.equal(f.root.hidden, true);
  f.advance(10_000); f.timeout.fn();
  assert.equal(f.root.hidden, false);
  assert.equal(f.fields.input.reported, undefined);
});

test('mounts the fixed invitation outside the isolated docs pane', () => {
  const f = fixture();
  f.controller.bind();
  assert.equal(f.root.parentElement, f.document.body);
});

test('landing and elapsed time alone never show the invitation', () => {
  const f = fixture({ arrivedViaDocsLink: false });
  f.controller.bind(); f.advance(10_000); f.timeout.fn();
  assert.equal(f.root.hidden, true);
  assert.equal(f.controller.state.engaged, false);
  f.controller.onVisibilityChange();
  assert.equal(f.root.hidden, true);
});

test('a real docs link qualifies, preserves engagement across pages, and keeps the time minimum', () => {
  const f = fixture({ arrivedViaDocsLink: false });
  f.controller.bind();
  const target = docsLink('/docs/packages/');
  f.listeners.click({ isTrusted: false, button: 0, target });
  assert.equal(f.controller.state.engaged, false);
  f.advance(4_000);
  f.listeners.click({ isTrusted: true, button: 0, target });
  assert.equal(f.controller.state.engaged, true);
  assert.equal(f.root.hidden, true);
  f.controller.pauseForNavigation();
  const next = fixture({ session: f.session, arrivedViaDocsLink: false, initialNow: 4_000, baseURI: 'https://docs.example.test/docs/packages' });
  next.controller.bind();
  assert.equal(next.timeout.delay, 10_000);
  next.advance(10_000); next.timeout.fn();
  assert.equal(next.root.hidden, false);
});

test('external links, empty search and synthetic input do not qualify', () => {
  const f = fixture({ arrivedViaDocsLink: false }); f.controller.bind();
  const external = { closest(selector) { return selector === 'a[href]' ? { getAttribute: () => 'https://example.org/' } : undefined; } };
  f.listeners.click({ isTrusted: true, button: 0, target: external });
  f.listeners.input({ isTrusted: true, target: { matches: () => true, value: ' ' } });
  f.listeners.input({ isTrusted: false, target: { matches: () => true, value: 'blueprints' } });
  assert.equal(f.controller.state.engaged, false);
  f.advance(10_000); f.timeout.fn();
  f.listeners.input({ isTrusted: true, target: { matches: selector => selector.includes('.pagefind-ui__search-input'), value: 'blueprints' } });
  assert.equal(f.root.hidden, false);
});

test('using a copy control qualifies without taking focus', () => {
  const f = fixture({ arrivedViaDocsLink: false }); f.controller.bind();
  f.advance(10_000); f.timeout.fn();
  const target = { closest(selector) { return selector.startsWith('[data-copy-page]') ? {} : undefined; } };
  f.listeners.click({ isTrusted: true, button: 0, target });
  assert.equal(f.root.hidden, false);
  assert.equal(f.fields.input.reported, undefined);
});

test('scrolling needs reader input and movement, not restoration or tiny jitter', () => {
  const f = fixture({ arrivedViaDocsLink: false }); f.controller.bind();
  f.advance(10_000); f.timeout.fn();
  f.document.scrollingElement.scrollTop = 500; f.listeners.scroll();
  assert.equal(f.root.hidden, true);
  f.listeners.wheel({ isTrusted: false, type: 'wheel' });
  f.document.scrollingElement.scrollTop = 600; f.listeners.scroll();
  assert.equal(f.root.hidden, true);
  f.listeners.wheel({ isTrusted: true, type: 'wheel' });
  f.document.scrollingElement.scrollTop = 679; f.listeners.scroll();
  assert.equal(f.root.hidden, true);
  f.document.scrollingElement.scrollTop = 680; f.listeners.scroll();
  assert.equal(f.root.hidden, false);
});

test('expired scroll intent and editing arrow keys do not qualify, but touch and reading keys do', () => {
  const f = fixture({ arrivedViaDocsLink: false }); f.controller.bind();
  f.listeners.wheel({ isTrusted: true, type: 'wheel' }); f.advance(1501);
  f.document.scrollingElement.scrollTop = 200; f.listeners.scroll();
  assert.equal(f.controller.state.engaged, false);
  f.listeners.keydown({ isTrusted: true, type: 'keydown', key: 'ArrowDown', target: { closest: () => ({}) } });
  f.document.scrollingElement.scrollTop = 400; f.listeners.scroll();
  assert.equal(f.controller.state.engaged, false);
  f.listeners.touchmove({ isTrusted: true, type: 'touchmove' });
  f.document.scrollingElement.scrollTop = 480; f.listeners.scroll();
  assert.equal(f.controller.state.engaged, true);
  const keyboard = fixture({ arrivedViaDocsLink: false }); keyboard.controller.bind();
  keyboard.listeners.keydown({ isTrusted: true, type: 'keydown', key: 'PageDown', target: { closest: () => undefined } });
  keyboard.document.scrollingElement.scrollTop = 80; keyboard.listeners.scroll();
  assert.equal(keyboard.controller.state.engaged, true);
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

test('a restored document requires a fresh delay and engagement after navigation', () => {
  const f = fixture();
  f.controller.bind();
  f.advance(4_000);
  f.controller.pauseForNavigation();
  assert.equal(f.timeout, undefined);
  f.controller.bind();
  assert.equal(f.timeout.delay, 10_000);
  assert.equal(f.controller.state.engaged, false);
  f.listeners.input({isTrusted:true,target:{matches:()=>true,value:'blueprints'}});
  f.advance(10_000); f.timeout.fn();
  assert.equal(f.root.hidden, false);
  f.controller.pauseForNavigation();
  assert.equal(f.root.hidden, true);
});

test('reloads and old session flags require fresh engagement and visible time', () => {
  const first = fixture();
  first.controller.bind(); first.advance(10_000); first.timeout.fn();
  assert.equal(first.root.hidden, false);
  first.session.set('submilli.docs-invitation.engaged.v1', '1');
  first.session.set('submilli.docs-invitation.elapsed.v1', '10000');
  const reload = fixture({ session: first.session, arrivedViaDocsLink: false });
  reload.controller.bind();
  assert.equal(reload.root.hidden, true);
  assert.equal(reload.timeout.delay, 10_000);
  reload.advance(10_000); reload.timeout.fn();
  assert.equal(reload.root.hidden, true);
  reload.listeners.wheel({ isTrusted: true, type: 'wheel' });
  reload.document.scrollingElement.scrollTop = 80; reload.listeners.scroll();
  assert.equal(reload.root.hidden, false);
});

test('navigation carry is one-use, path-specific and short-lived', () => {
  const key = 'submilli.docs-invitation.navigation.v1';
  for (const token of ['broken', JSON.stringify({path:'/docs/elsewhere',createdAt:0,elapsed:10000}), JSON.stringify({path:'/docs/quickstart',createdAt:0,elapsed:10000}), JSON.stringify({path:'/docs/quickstart',createdAt:40000,elapsed:10000})]) {
    const session = new Map([[key, token]]);
    const f = fixture({session, arrivedViaDocsLink:false, initialNow:30001});
    f.controller.bind();
    assert.equal(f.controller.state.engaged, false);
    assert.equal(session.has(key), false);
  }
  const session = new Map([[key, JSON.stringify({path:'/docs/quickstart',createdAt:0,elapsed:10000})]]);
  const visit = fixture({session, arrivedViaDocsLink:false}); visit.controller.bind();
  assert.equal(visit.root.hidden, true);
  assert.equal(visit.timeout.delay, 10_000);
  visit.advance(10_000); visit.timeout.fn();
  assert.equal(visit.root.hidden, false);
  const reload = fixture({session, arrivedViaDocsLink:false}); reload.controller.bind();
  assert.equal(reload.root.hidden, true);
});

test('new-tab, modified, download and same-page links do not carry into another load', () => {
  const key = 'submilli.docs-invitation.navigation.v1';
  for (const [target, event] of [[docsLink('/docs/packages/', {target:'_blank'}), {}], [docsLink('/docs/packages/'), {metaKey:true}], [docsLink('/docs/packages/', {download:''}), {}], [docsLink('#installing'), {}]]) {
    const f = fixture({arrivedViaDocsLink:false}); f.controller.bind();
    f.listeners.click({isTrusted:true,button:0,target,...event});
    f.controller.pauseForNavigation();
    assert.equal(f.session.has(key), false);
  }
});

test('later click cancellation never carries engagement into a fresh landing', () => {
  const f = fixture({arrivedViaDocsLink:false}); f.controller.bind();
  f.advance(10_000); f.timeout.fn();
  const event = {isTrusted:true,button:0,target:docsLink('/docs/packages/'),defaultPrevented:false};
  f.listeners.click(event);
  event.defaultPrevented = true;
  f.controller.pauseForNavigation();
  const landing = fixture({session:f.session,arrivedViaDocsLink:false,baseURI:'https://docs.example.test/docs/packages/',initialNow:10_000});
  landing.controller.bind();
  assert.equal(landing.controller.state.engaged, false);
  assert.equal(landing.root.hidden, true);
});

test('pauses and defers on routes or overlays where the invitation cannot be shown', () => {
  const hiddenRoute = fixture({ invitation: false });
  hiddenRoute.controller.bind(); hiddenRoute.advance(20_000); hiddenRoute.controller.onVisibilityChange();
  assert.equal(hiddenRoute.root.hidden, true);

  const session = new Map([['submilli.docs-invitation.navigation.v1', JSON.stringify({path:'/docs/quickstart',createdAt:0,elapsed:10000})]]);
  const covered = fixture({ session, arrivedViaDocsLink:false, overlay: true });
  covered.controller.bind();
  covered.advance(10_000); covered.timeout.fn();
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
  const session = new Map([['submilli.docs-invitation.navigation.v1', JSON.stringify({path:'/docs/quickstart',createdAt:0,elapsed:10000})]]);
  const covered = fixture({ session, arrivedViaDocsLink:false, overlay: true, mutationObserver: FakeObserver });
  covered.controller.bind();
  covered.advance(10_000); covered.timeout.fn();
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
