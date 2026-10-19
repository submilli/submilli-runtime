import assert from 'node:assert/strict';
import { test, after } from 'node:test';
import { readFile, writeFile, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { stripTypeScriptTypes } from 'node:module';
import { contractHost } from '../../../scripts/package-contract-host.mjs';

const operations = JSON.parse(await readFile(new URL('./operations.json', import.meta.url)));
const fixtures = JSON.parse(await readFile(new URL('./fixtures.json', import.meta.url)));
const host = contractHost();
let secretReads = 0;
let key = 'test-token';
host.secrets.get = (name) => { secretReads++; assert.equal(name, 'SCRAPECREATORS_API_KEY'); return key; };
// Merge the package's modules only for this host-adapter test. Native package tests
// independently compile/link the actual module graph and check response assertions.
let source = '';
for (const file of ['transport', 'reddit', 'youtube', 'linkedin', 'github', 'twitter', 'lib']) {
    source += (await readFile(process.env.SCRAPECREATORS_EMITTED_DIR
        ? join(process.env.SCRAPECREATORS_EMITTED_DIR, `${file}.js`)
        : new URL(`../src/${file}.ts`, import.meta.url), 'utf8'))
        .replace(/import\s+[\s\S]*?from "[^"]+";\n/g, '')
        .replace(/export \{[^}]+\} from "[^"]+";\n/g, '') + '\n';
}
globalThis.__scrapeCreatorsContractHost = host;
const prelude = 'const { get, post, check, secrets, encodeComponent, parse } = globalThis.__scrapeCreatorsContractHost;\n';
const scratch = await mkdtemp(join(tmpdir(), 'scrapecreators-contract-'));
const modulePath = join(scratch, 'package.mjs');
await writeFile(modulePath, prelude + stripTypeScriptTypes(source));
const api = await import(pathToFileURL(modulePath));
after(() => rm(scratch, { recursive: true, force: true }));

function reset() { host.reset(); secretReads = 0; key = 'test-token'; }
function minimal(op) {
    const options = {};
    for (const param of op.parameters) if (param.required) options[param.option] = param.example;
    if (!op.parameters.some(p => p.required)) {
        const identity = op.parameters.find(p => ['handle', 'url', 'channelId', 'subreddit'].includes(p.wire));
        if (identity) options[identity.option] = identity.example;
    }
    return options;
}
function outcome(op) { return fixtures[`${op.platform}.${op.method}`]; }
function call(op, options) { return api[op.platform][op.method](options); }
function expectCode(code) { return error => error instanceof api.ScrapeCreatorsError && error.code === code; }

for (const op of operations) {
    test(`${op.platform}.${op.method}: exact minimal request, result, auth and denial boundary`, () => {
        reset(); host.response(() => ({ data: outcome(op) }));
        const options = minimal(op);
        const response = call(op, options);
        assert.deepEqual(response.data, outcome(op));
        assert.deepEqual(response.raw, outcome(op));
        assert.deepEqual(host.checks, [{ capability: op.capability, context: {} }]);
        assert.equal(secretReads, 1); assert.equal(host.requests.length, 1);
        const request = host.requests[0]; const url = new URL(request.url);
        assert.equal(request.method, 'get'); assert.equal(url.origin, 'https://api.scrapecreators.com');
        assert.equal(url.pathname, op.path);
        assert.equal(url.searchParams.size, Object.keys(options).length);
        for (const p of op.parameters) if (options[p.option] !== undefined) assert.equal(url.searchParams.get(p.wire), String(options[p.option]));
        assert.equal(request.headers.get('x-api-key'), 'test-token');
        assert.ok(!request.url.includes('test-token'));
        reset(); host.denial(() => true);
        assert.throws(() => call(op, options), /Capability denied/);
        assert.equal(secretReads, 0); assert.equal(host.requests.length, 0);
    });
    test(`${op.platform}.${op.method}: missing endpoint data is rejected`, () => {
        reset(); host.response(() => ({ data: { success: true } }));
        assert.throws(() => call(op, minimal(op)), expectCode('invalid_response'));
    });
    test(`${op.platform}.${op.method}: every documented option reaches its wire parameter`, () => {
        for (const p of op.parameters) {
            reset(); host.response(() => ({ data: outcome(op) }));
            const options = { ...minimal(op), [p.option]: p.example };
            call(op, options);
            assert.equal(new URL(host.requests[0].url).searchParams.get(p.wire), String(p.example));
        }
    });
}

test('encoding preserves Unicode, punctuation and opaque cursors without query injection', () => {
    reset(); host.response(() => ({ data: fixtures['reddit.search'] }));
    const text = '🚀 "quoted" &cursor=bad + / ? # %';
    api.reddit.search({ query: text, after: text, trim: false });
    const url = new URL(host.requests[0].url);
    assert.equal(url.searchParams.get('query'), text); assert.equal(url.searchParams.get('after'), text);
    assert.equal(url.searchParams.get('trim'), 'false'); assert.equal(url.searchParams.size, 3);
});

test('Reddit POST sends the identical opaque cursor as JSON without query parameters', () => {
    reset(); host.response(() => ({ data: fixtures['reddit.listComments'] }));
    const cursor = 'opaque+/='.repeat(2000);
    api.reddit.listComments({ url: 'https://www.reddit.com/r/test/comments/123/x/', cursor, trim: false, usePost: true });
    const request = host.requests[0];
    assert.equal(request.method, 'post'); assert.equal(new URL(request.url).search, '');
    assert.deepEqual(JSON.parse(request.body), { url: 'https://www.reddit.com/r/test/comments/123/x/', cursor, trim: false });
    assert.equal(host.checks[0].capability, 'scrapecreators.com/reddit.listComments');
});

test('empty and missing credentials never send HTTP', () => {
    for (const value of [undefined, '', '  ']) {
        reset(); key = value;
        assert.throws(() => api.twitter.getProfile({ handle: 'example' }), expectCode('missing_credentials'));
        assert.equal(host.requests.length, 0);
    }
});

test('invalid options fail before secret access and HTTP', () => {
    for (const action of [
        () => api.reddit.search({ query: ' ' }),
        () => api.youtube.getChannel({}),
        () => api.github.getUser({}),
        () => api.github.listUserActivity({ handle: 'octocat', cursor: -1 }),
        () => api.github.listUserRepositories({ handle: 'octocat', cursor: Infinity }),
        () => api.linkedin.listCompanyPosts({ url: 'https://www.linkedin.com/company/example', page: 1.5 }),
        () => api.linkedin.searchPosts({ query: 'agents', cursor: '12' }),
        () => api.twitter.searchPosts({ query: 'agents', cursor: '0' }),
        () => api.reddit.search({ query: 'agents', filter: 'comments', timeframe: 'month' }),
        () => api.reddit.search({ query: 'agents', filter: 'comments', sort: 'comment_count' }),
        () => api.reddit.getPost({ url: 'file:///etc/passwd' }),
        () => api.reddit.getPost({ url: 'https://user:password@example.test/' }),
        () => api.reddit.search({ query: 'agents', sort: 'invalid' }),
        () => api.github.listUserPullRequests({ handle: 'octocat', since: '2026-02-30' }),
        () => api.github.listUserPullRequests({ handle: 'octocat', since: '2026-02-02', until: '2026-02-01' }),
    ]) {
        reset(); assert.throws(action, expectCode('invalid_argument'));
        assert.equal(secretReads, 0); assert.equal(host.requests.length, 0);
    }
});

test('HTTP and HTTP-200 provider errors retain status, code and Retry-After', () => {
    for (const [status, code] of [[400, 'invalid_request'], [401, 'unauthorized'], [402, 'quota_exceeded'], [403, 'forbidden'], [404, 'unavailable_content'], [410, 'unavailable_content'], [429, 'rate_limited'], [503, 'provider_error']]) {
        reset(); host.response(() => ({ status, data: { success: false, code: 'upstream', message: 'failure' }, headers: { 'retry-after': '2' } }));
        assert.throws(() => api.twitter.getProfile({ handle: 'example' }), error => expectCode(code)(error) && error.status === status && error.providerCode === 'upstream' && error.retryAfter === '2');
        assert.equal(host.requests.length, 1);
    }
    for (const [message, code] of [['Insufficient credits', 'quota_exceeded'], ['Invalid API key', 'unauthorized'], ['Rate limit exceeded', 'rate_limited']]) {
        assert.throws(() => api.decodeResponse(JSON.stringify({ success: false, error: message })), expectCode(code));
    }
    assert.throws(() => api.decodeResponse('<html>bad gateway</html>', 503), expectCode('provider_error'));
    for (const text of ['{}', 'null', '[]', '1', '"text"', '{"success":"true"}', 'broken']) {
        assert.throws(() => api.decodeResponse(text), expectCode('invalid_response'));
    }
});

test('partial failures, unknown fields, nulls and confirmed zero survive without fabrication', () => {
    reset();
    const data = { success: true, posts: [{ rest_id: '2105732089685348719', url: 'https://x.com/example/status/2105732089685348719', legacy: { full_text: 'Synthetic post', favorite_count: 0, created_at: null } }], cursor: null, credits_charged: 1, partial: true, errors: [{ url: 'https://x.com/missing/status/1', code: 'unavailable', message: 'Unavailable' }], future: { field: true } };
    host.response(() => ({ data }));
    const result = api.twitter.searchPosts({ query: 'example' });
    assert.deepEqual(result.raw, data); assert.equal(result.data.posts[0].rest_id, data.posts[0].rest_id);
    assert.equal(result.data.posts[0].legacy.favorite_count, 0);
    assert.equal(result.data.posts[0].legacy.reply_count, undefined);
    assert.equal(result.data.posts[0].legacy.created_at, null); assert.equal(result.data.cursor, null);
    assert.deepEqual(result.data.errors, data.errors);
    assert.deepEqual(result.issues[0], { kind: 'error', code: 'unavailable', url: data.errors[0].url, message: 'Unavailable', raw: data.errors[0] }); assert.equal(host.requests.length, 1);
});

test('null captions remain explicit and transcript calls do not enable paid long mode', () => {
    reset(); host.response(() => ({ data: { success: true, transcript: null, transcript_only_text: null, language: null, credits_charged: 0 } }));
    assert.equal(api.youtube.getTranscript({ url: 'https://youtu.be/example' }).data.transcript, null);
    assert.equal(new URL(host.requests[0].url).searchParams.has('use_ai_as_fallback'), false);
    reset(); host.response(() => ({ data: { success: true, transcript: null } }));
    assert.equal(api.twitter.getTranscript({ url: 'https://x.com/example/status/1' }).data.transcript, null);
    assert.equal(new URL(host.requests[0].url).searchParams.has('allow_long_transcription'), false);
});


test('public and internal interface mirrors stay identical', async () => {
    const publicSource = await readFile(new URL('../src/lib.ts', import.meta.url), 'utf8');
    const internalTypes = await readFile(new URL('../src/types.ts', import.meta.url), 'utf8');
    assert.ok(publicSource.startsWith(internalTypes));
});
