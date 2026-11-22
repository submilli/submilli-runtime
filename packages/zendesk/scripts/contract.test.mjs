// Exercise the package's operations against a mocked Submilli host and a mocked
// Zendesk, without network access or credentials. Native build tests verify
// Submilli compatibility and the live tests Zendesk's real answers; this
// harness checks the requests each operation sends, what `check()` sees, and
// that a denied check sends no write.
//
//   node --test packages/zendesk/scripts/contract.test.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import { test } from 'node:test';

const ACCOUNT = 'https://acme.zendesk.com';
const API = `${ACCOUNT}/api/v2/`;
const TOKEN = 'test-token';
const UPDATED = '2026-11-20T10:00:00Z';

// ---------------------------------------------------------------------------
// The mocked host
// ---------------------------------------------------------------------------

let requests = [];
let checks = [];
let routes = [];
let denied = () => false;
const files = new Map();

function reset() {
    requests = [];
    checks = [];
    routes = [];
    denied = () => false;
    files.clear();
}

// Answer requests whose method and path (after `/api/v2/`, with its query) match.
function on(method, pattern, answer) {
    routes.push({ method, pattern, answer });
}

function reply(status, data, headers = {}) {
    return { status, data, headers };
}

function respond(method, url, body, headers) {
    const sent = { method, url, path: url.startsWith(API) ? url.slice(API.length) : url, body, headers };
    requests.push(sent);
    for (const route of routes) {
        if (route.method !== method) continue;
        const matched = typeof route.pattern === 'string' ? sent.path === route.pattern : route.pattern.test(sent.path);
        if (!matched) continue;
        const answer = typeof route.answer === 'function' ? route.answer(sent) : route.answer;
        return response(answer);
    }
    return response(reply(404, { error: 'RecordNotFound', description: 'no route for ' + method + ' ' + sent.path }));
}

function response(answer) {
    const text = typeof answer.data === 'string' ? answer.data : answer.data === undefined ? '' : JSON.stringify(answer.data);
    const status = answer.status ?? 200;
    return {
        status, ok: status >= 200 && status < 300, statusText: '', body: text,
        headers: { get: (name) => answer.headers?.[name.toLowerCase()] ?? null },
    };
}

function asObject(headers) {
    return headers instanceof Map ? Object.fromEntries(headers) : headers;
}

function copy(body) {
    return body instanceof Uint8Array ? body : JSON.parse(JSON.stringify(body));
}

const host = {
    get: (url, headers) => respond('GET', url, null, asObject(headers)),
    post: (url, body, headers) => respond('POST', url, copy(body), asObject(headers)),
    put: (url, body, headers) => respond('PUT', url, copy(body), asObject(headers)),
    exists: (path) => files.has(path),
    stat: (path) => ({ kind: 'file', size: (files.get(path) ?? new Uint8Array()).length }),
    read: (path) => files.get(path) ?? new Uint8Array(),
    check: (capability, context) => {
        const name = capability.replace('zendesk.com/', '');
        checks.push({ capability: name, context });
        if (denied(name, context)) throw new Error('Capability denied');
    },
    encodeComponent: encodeURIComponent,
    secrets: { get: (name) => (name === 'ZENDESK_ACCESS_TOKEN' ? TOKEN : undefined) },
    accountUrl: ACCOUNT,
};

// The package's modules, merged into one: a data: URL module cannot resolve
// relative imports. `lib.ts` comes last so every helper it uses is defined.
async function loadPackage() {
    const modules = ['errors', 'request', 'response', 'account', 'lib'];
    const sources = await Promise.all(modules.map((name) => readFile(new URL(`../src/${name}.ts`, import.meta.url), 'utf8')));
    const source = sources.join('\n')
        .replace(/^import\s+[^;]*?from "submilli:[^"]+";\n/gm, '')
        .replace(/^(import|export) [^;]*? from "\.\/[^"]+";\n/gm, '');
    globalThis.__zendeskHost = host;
    const prelude = 'const { get, post, put, exists, stat, read, check, encodeComponent, secrets, accountUrl } = globalThis.__zendeskHost;\n';
    return import(`data:text/javascript;base64,${Buffer.from(prelude + stripTypeScriptTypes(source)).toString('base64')}`);
}

const zd = await loadPackage();

function ticket(id, fields = {}) {
    return {
        id, subject: 'Help', description: 'It broke', status: 'open', priority: 'normal', type: null, tags: ['vip'],
        group_id: 100, organization_id: 7, brand_id: 1, assignee_id: 20, requester_id: 10, custom_fields: [],
        updated_at: UPDATED, created_at: '2026-11-01T09:00:00Z', via: { channel: 'email' }, ...fields,
    };
}

function onTicket(id, fields = {}) {
    on('GET', `tickets/${id}.json`, reply(200, { ticket: ticket(id, fields) }));
    on('PUT', `tickets/${id}.json`, (sent) => reply(200, { ticket: ticket(id, sent.body.ticket) }));
}

const FACTS = { group: '100', organization: '7', brand: '1', assignee: '20', requester: '10', status: 'open' };

function writes() {
    return requests.filter((request) => request.method !== 'GET').map((request) => ({ method: request.method, path: request.path, body: request.body }));
}

function codeOf(action) {
    try {
        action();
    } catch (error) {
        return error.errorCode ?? error.message;
    }
    return 'no error';
}

// ---------------------------------------------------------------------------
// Tickets
// ---------------------------------------------------------------------------

test('getTicket reads the ticket, checks its facts and maps it', () => {
    reset();
    onTicket(35436);
    const found = zd.getTicket('035436');
    assert.equal(requests[0].url, `${API}tickets/35436.json`);
    assert.equal(requests[0].headers.Authorization, `Bearer ${TOKEN}`);
    assert.deepEqual(checks, [{ capability: 'tickets.get', context: FACTS }]);
    assert.equal(found.id, '35436');
    assert.equal(found.groupId, '100');
});

test('a missing ticket is checked with empty facts before it reads as null', () => {
    reset();
    assert.equal(zd.getTicket('9'), null);
    assert.deepEqual(checks[0].context, { group: '', organization: '', brand: '', assignee: '', requester: '', status: '' });
});

test('findTickets writes the search query and checks the filter', () => {
    reset();
    on('GET', /^search\.json\?/, reply(200, { results: [ticket(1)], count: 250, next_page: 'https://acme.zendesk.com/api/v2/search.json?page=2' }));
    const page = zd.findTickets(
        { status: ['open', 'new', 'open'], groupId: '100', assigneeId: null, tags: ['VIP'], createdAfter: '2026-11-01', text: 'refund please' },
        { pageSize: 50 },
    );
    const query = new URL(requests[0].url).searchParams;
    assert.equal(query.get('query'), 'type:ticket status:new status:open group:100 assignee:none tags:vip created>=2026-11-01 "refund please"');
    assert.equal(query.get('sort_by'), 'updated_at');
    assert.equal(query.get('sort_order'), 'desc');
    assert.equal(query.get('per_page'), '50');
    assert.equal(query.get('page'), '1');
    assert.deepEqual(checks, [{ capability: 'tickets.find', context: {
        group: '100', organization: '', brand: '', assignee: 'none', requester: '', status: ['new', 'open'], tags: ['vip'],
    } }]);
    assert.equal(page.count, 250);
    assert.equal(page.nextCursor, '2');
});

test('the last search page within 1,000 results has no cursor', () => {
    reset();
    on('GET', /^search\.json\?/, reply(200, { results: [], count: 5000, next_page: 'x' }));
    assert.equal(zd.findTickets({}, { pageSize: 100, cursor: '10' }).nextCursor, null);
});

test('updateTicket writes only the changes, conditional on the ticket read', () => {
    reset();
    onTicket(1);
    zd.updateTicket('1', { status: 'pending', addTags: ['Waiting'], customFields: [{ id: '360001', value: ['Gold'] }] });
    assert.deepEqual(checks, [{ capability: 'tickets.update', context: {
        ...FACTS, status: 'pending', previousStatus: 'open', fields: ['custom.360001', 'status', 'tags'],
    } }]);
    assert.deepEqual(writes(), [{ method: 'PUT', path: 'tickets/1.json', body: { ticket: {
        status: 'pending', additional_tags: ['waiting'], custom_fields: [{ id: 360001, value: ['gold'] }],
        safe_update: true, updated_stamp: UPDATED,
    } } }]);
});

test('a denied update sends no write', () => {
    reset();
    onTicket(1);
    denied = (name) => name === 'tickets.update';
    assert.throws(() => zd.updateTicket('1', { priority: 'urgent' }), /denied/);
    assert.deepEqual(writes(), []);
});

test('a write to a ticket changed since it was read throws a retryable UpdateConflict', () => {
    reset();
    on('GET', 'tickets/1.json', reply(200, { ticket: ticket(1) }));
    on('PUT', 'tickets/1.json', reply(409, { error: 'UpdateConflict', description: 'changed' }));
    try {
        zd.updateTicket('1', { status: 'solved' });
        assert.fail('no conflict');
    } catch (error) {
        assert.equal(error.errorCode, 'UpdateConflict');
        assert.equal(error.retryable, true);
        assert.equal(error.status, 409);
    }
});

test('assignTicket checks where the ticket is and where it goes', () => {
    reset();
    onTicket(1);
    zd.assignTicket('1', { groupId: '200', assigneeId: null });
    assert.deepEqual(checks[0].context, { ...FACTS, toGroup: '200', toAssignee: 'none' });
    assert.deepEqual(writes()[0].body.ticket, { group_id: 200, assignee_id: null, safe_update: true, updated_stamp: UPDATED });
});

test('createTicket with a public first comment is also checked as a reply', () => {
    reset();
    on('POST', 'tickets.json', (sent) => reply(201, { ticket: ticket(5, sent.body.ticket) }));
    zd.createTicket({ subject: 'Help', body: 'Hello', requesterId: '10', groupId: '100', priority: 'high', tags: ['New'] });
    assert.deepEqual(checks.map((check) => check.capability), ['tickets.create', 'comments.reply']);
    assert.deepEqual(checks[0].context, {
        group: '100', organization: '', brand: '', assignee: '', requester: '10', public: true, fields: ['priority', 'subject', 'tags'],
    });
    assert.deepEqual(writes()[0].body, { ticket: {
        subject: 'Help', comment: { public: true, body: 'Hello' }, requester_id: 10, group_id: 100, priority: 'high', tags: ['new'],
    } });
    reset();
    on('POST', 'tickets.json', (sent) => reply(201, { ticket: ticket(6, sent.body.ticket) }));
    zd.createTicket({ subject: 'Internal', body: 'Note', public: false });
    assert.deepEqual(checks.map((check) => check.capability), ['tickets.create']);
});

test('mergeTickets checks the target and every source', () => {
    reset();
    onTicket(1);
    onTicket(2, { group_id: 200 });
    onTicket(3);
    on('POST', 'tickets/1/merge.json', reply(200, { job_status: { id: 'job1', status: 'queued' } }));
    const job = zd.mergeTickets('1', ['3', '2'], { targetComment: 'Merged' });
    assert.deepEqual(checks.map((check) => check.context.role + ':' + check.context.group), ['target:100', 'source:200', 'source:100']);
    assert.deepEqual(writes()[0].body, { ids: [2, 3], target_comment: 'Merged', target_comment_is_public: false, source_comment_is_public: false });
    assert.equal(job.status, 'queued');
});

// ---------------------------------------------------------------------------
// Conversation
// ---------------------------------------------------------------------------

test('reply posts a public comment and checks the status it sets', () => {
    reset();
    onTicket(1);
    zd.reply('1', 'Fixed now', { status: 'solved', attachments: ['tok1'] });
    assert.deepEqual(checks, [{ capability: 'comments.reply', context: { ...FACTS, status: 'solved', previousStatus: 'open' } }]);
    assert.deepEqual(writes()[0].body.ticket, {
        comment: { public: true, body: 'Fixed now', uploads: ['tok1'] }, status: 'solved', safe_update: true, updated_stamp: UPDATED,
    });
});

test('a denied reply sends nothing; a note is private', () => {
    reset();
    onTicket(1);
    denied = (name) => name === 'comments.reply';
    assert.throws(() => zd.reply('1', 'Hi'), /denied/);
    assert.deepEqual(writes(), []);
    zd.addNote('1', '<b>Checked</b>', { html: true });
    assert.deepEqual(writes()[0].body.ticket.comment, { public: false, html_body: '<b>Checked</b>' });
    assert.equal(checks[1].capability, 'comments.note');
});

test('listComments pages with the cursor Zendesk returned', () => {
    reset();
    onTicket(1);
    on('GET', /^tickets\/1\/comments\.json\?/, reply(200, {
        comments: [{ id: 11, author_id: 10, plain_body: 'Hi', public: true }],
        meta: { has_more: true, after_cursor: 'abc=' },
    }));
    const page = zd.listComments('1', { pageSize: 10, cursor: 'xyz' });
    assert.equal(requests[1].path, 'tickets/1/comments.json?page%5Bsize%5D=10&page%5Bafter%5D=xyz');
    assert.equal(page.items[0].body, 'Hi');
    assert.equal(page.nextCursor, 'abc=');
});

test('uploadAttachment sends the file bytes with its name and type', () => {
    reset();
    files.set('/out/report.pdf', new Uint8Array([1, 2, 3]));
    on('POST', /^uploads\.json\?/, reply(201, { upload: { token: 'tok1', attachment: { id: 9, file_name: 'report.pdf', size: 3 } } }));
    const upload = zd.uploadAttachment('/out/report.pdf', { contentType: 'application/pdf' });
    assert.deepEqual(checks[0].context, { path: '/out/report.pdf', fileName: 'report.pdf', contentType: 'application/pdf' });
    assert.equal(requests[0].path, 'uploads.json?filename=report.pdf');
    assert.equal(requests[0].headers['Content-Type'], 'application/pdf');
    assert.deepEqual([...requests[0].body], [1, 2, 3]);
    assert.equal(upload.token, 'tok1');
});

// ---------------------------------------------------------------------------
// Views and macros
// ---------------------------------------------------------------------------

test('viewTickets checks each ticket it returns', () => {
    reset();
    on('GET', /^views\/5\/tickets\.json/, reply(200, { tickets: [ticket(1), ticket(2, { group_id: 200 })], meta: { has_more: false } }));
    denied = (name, context) => name === 'tickets.get' && context.group !== '100';
    assert.throws(() => zd.viewTickets('5'), /denied/);
    assert.deepEqual(checks.map((check) => check.capability + ':' + (check.context.group ?? check.context.view)), [
        'views.read:5', 'tickets.get:100', 'tickets.get:200',
    ]);
});

test('applyMacro writes what the macro changes and checks its comment', () => {
    reset();
    onTicket(1);
    on('GET', 'tickets/1/macros/50/apply.json', reply(200, { result: { ticket: {
        ...ticket(1), status: 'solved', tags: ['vip', 'done'], custom_fields: [{ id: 7, value: 'x' }],
        comment: { body: 'Thanks!', public: true },
    } } }));
    zd.applyMacro('1', '50');
    assert.deepEqual(checks.map((check) => check.capability), ['macros.apply', 'comments.reply']);
    assert.deepEqual(checks[0].context.fields, ['custom.7', 'status', 'tags']);
    assert.deepEqual(checks[1].context, { ...FACTS, status: 'solved', previousStatus: 'open' });
    assert.deepEqual(writes()[0].body.ticket, {
        status: 'solved', tags: ['vip', 'done'], custom_fields: [{ id: 7, value: 'x' }],
        comment: { public: true, body: 'Thanks!' }, safe_update: true, updated_stamp: UPDATED,
    });
});

test('a macro that moves the ticket names group and assignee', () => {
    reset();
    onTicket(1);
    on('GET', 'tickets/1/macros/52/apply.json', reply(200, { result: { ticket: { group_id: 200, assignee_id: null } } }));
    denied = (name, context) => name === 'macros.apply' && context.fields.includes('group');
    assert.throws(() => zd.applyMacro('1', '52'), /denied/);
    assert.deepEqual(checks[0].context.fields, ['assignee', 'group']);
    assert.deepEqual(writes(), []);
});

// ---------------------------------------------------------------------------
// People, Help Center and errors
// ---------------------------------------------------------------------------

test('updateUser checks the role and organization read from Zendesk', () => {
    reset();
    on('GET', 'users/10.json', reply(200, { user: { id: 10, role: 'end-user', organization_id: 7 } }));
    on('PUT', 'users/10.json', (sent) => reply(200, { user: { id: 10, ...sent.body.user } }));
    zd.updateUser('10', { notes: 'VIP', userFields: [{ key: 'plan', value: 'gold' }] });
    assert.deepEqual(checks[0].context, { role: 'end-user', organization: '7', fields: ['notes', 'user_fields.plan'] });
    assert.deepEqual(writes()[0].body, { user: { notes: 'VIP', user_fields: { plan: 'gold' } } });
});

test('searchArticles sends the query, locale and section', () => {
    reset();
    on('GET', /^help_center\/articles\/search\.json\?/, reply(200, { results: [{ id: 1, title: 'Refunds', snippet: '<em>refund</em>' }], count: 1, next_page: null }));
    const page = zd.searchArticles('refund policy', { locale: 'EN-US', sectionId: '3' });
    const query = new URL(requests[0].url).searchParams;
    assert.equal(query.get('query'), 'refund policy');
    assert.equal(query.get('locale'), 'en-us');
    assert.equal(query.get('section'), '3');
    assert.deepEqual(checks[0].context, { query: 'refund policy', locale: 'en-us', section: '3' });
    assert.equal(page.items[0].snippet, '<em>refund</em>');
    assert.equal(page.nextCursor, null);
});

test('errors carry Zendesk\'s code, details and Retry-After', () => {
    reset();
    on('GET', 'tickets/1.json', reply(429, { error: 'TooManyRequests' }, { 'retry-after': '42' }));
    try {
        zd.getTicket('1');
        assert.fail('no error');
    } catch (error) {
        assert.equal(error.name, 'ZendeskError');
        assert.equal(error.retryable, true);
        assert.equal(error.retryAfter, 42);
    }
    reset();
    on('POST', 'users.json', reply(422, { error: 'RecordInvalid', description: 'Record validation errors', details: { email: [{ description: 'Email is already taken' }] } }));
    try {
        zd.createUser({ name: 'Dana', email: 'dana@example.com' });
        assert.fail('no error');
    } catch (error) {
        assert.equal(error.errorCode, 'RecordInvalid');
        assert.deepEqual(error.details, ['email: Email is already taken']);
    }
});

test('input is refused before any check or request', () => {
    reset();
    assert.equal(codeOf(() => zd.getTicket('1 OR 2')), 'invalid_id');
    assert.equal(codeOf(() => zd.findTickets({ text: 'x" status:closed "' })), 'invalid_input');
    assert.equal(codeOf(() => zd.updateTicket('1', { tags: ['a b'] })), 'invalid_tag');
    assert.deepEqual(checks, []);
    assert.deepEqual(requests, []);
});
