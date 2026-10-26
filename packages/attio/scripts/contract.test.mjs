// Exercise the actual package operations against a mocked Submilli host and a
// mocked Attio API, without network access or credentials. Native build tests
// separately verify Submilli compatibility; this harness checks the requests
// each operation sends and what `check()` sees.
//
//   node --test packages/attio/scripts/contract.test.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import { test } from 'node:test';

const DEAL = '8a1b2c3d-1111-4222-8333-944455556666';
const COMPANY = '0c0ffee0-aaaa-4bbb-8ccc-ddddeeeeffff';
const PERSON = '5e5e5e5e-1111-4222-8333-944455556666';
const MEMBER = 'fbe75eb0-d704-4d12-9e41-aa187e60ed73';

// Every request and `check()` since the last reset, in order, interleaved in `events`.
let events = [];
let requests = [];
let checks = [];
let secretReads = 0;
let denied = new Set();
let respond = () => reply(500, { message: 'no handler' });

function reset(handler) {
    events = [];
    requests = [];
    checks = [];
    secretReads = 0;
    denied = new Set();
    respond = handler;
}

function reply(status, body, headers = {}) {
    const text = typeof body === 'string' ? body : JSON.stringify(body);
    return {
        ok: status >= 200 && status < 300,
        status,
        statusText: { 200: 'OK', 404: 'Not Found', 409: 'Conflict', 429: 'Too Many Requests', 500: 'Internal Server Error' }[status] ?? '',
        headers: new Map(Object.entries(headers)),
        body: text,
        url: '',
        json: () => JSON.parse(text),
    };
}

function send(method, url, body, headers) {
    const request = { method, url, body, headers, path: url.replace('https://api.attio.com/v2', '') };
    requests.push(request);
    events.push({ kind: 'request', method, path: request.path });
    return respond(request);
}

globalThis.__attioHost = {
    get: (url, headers) => send('GET', url, undefined, headers),
    post: (url, body, headers) => send('POST', url, body, headers),
    put: (url, body, headers) => send('PUT', url, body, headers),
    patch: (url, body, headers) => send('PATCH', url, body, headers),
    secrets: { get: (name) => { secretReads += 1; return name === 'ATTIO_API_KEY' ? 'attio-test-key' : undefined; } },
    check: (capability, context) => {
        // A deep copy: the package must not change the context after the check.
        checks.push({ capability, context: JSON.parse(JSON.stringify(context)) });
        events.push({ kind: 'check', capability });
        if (denied.has(capability)) {
            const error = new Error('Capability denied: ' + capability);
            error.name = 'PermissionDeniedError';
            throw error;
        }
    },
};

// lib.ts imports its helpers from three sibling modules; inline them ahead of
// it, since a data: URL module cannot resolve relative imports.
async function source(name) {
    return (await readFile(new URL(`../src/${name}`, import.meta.url), 'utf8'))
        .replace(/^import [\s\S]*?from "[^"]+";\n/gm, '')
        .replace(/^export \{[\s\S]*?\} from "[^"]+";\n/gm, '');
}
const bundle = 'const { get, post, put, patch, secrets, check } = globalThis.__attioHost;\n' +
    (await source('errors.ts')) + (await source('request.ts')) + (await source('response.ts')) + (await source('lib.ts'));
const attio = await import(`data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(bundle)).toString('base64')}`);

// ---------------------------------------------------------------------------
// Attio fixtures.
// ---------------------------------------------------------------------------

const STAGES = [
    { id: { status_id: 's-lead' }, title: 'Lead', is_archived: false, celebration_enabled: false },
    { id: { status_id: 's-progress' }, title: 'In Progress', is_archived: false, celebration_enabled: false },
    { id: { status_id: 's-won' }, title: 'Won 🎉', is_archived: false, celebration_enabled: true },
    { id: { status_id: 's-old' }, title: 'Parked', is_archived: true, celebration_enabled: false },
];

function attribute(api_slug, type, extra = {}) {
    return { id: { attribute_id: 'a-' + api_slug }, api_slug, title: api_slug, description: null, type,
        is_multiselect: false, is_writable: true, is_required: false, is_unique: false, is_archived: false, config: {}, ...extra };
}

const PRIORITIES = [
    { id: { status_id: 'p-high' }, title: 'High', is_archived: false, celebration_enabled: false },
    { id: { status_id: 'p-low' }, title: 'Low', is_archived: false, celebration_enabled: false },
];

const DEAL_ATTRIBUTES = [
    attribute('name', 'text'),
    attribute('stage', 'status'),
    attribute('priority', 'status'),
    attribute('owner', 'actor-reference'),
    attribute('value', 'currency', { config: { currency: { default_currency_code: 'USD' } } }),
    attribute('associated_company', 'record-reference'),
    attribute('associated_people', 'record-reference', { is_multiselect: true }),
    attribute('legacy', 'text', { is_archived: true }),
];

function dealRecord(stage = 'Lead', statusId = 's-lead', extra = {}) {
    return {
        id: { workspace_id: 'w', object_id: 'o', record_id: DEAL },
        created_at: '2026-10-26T10:00:00.000000000Z',
        web_url: 'https://app.attio.com/acme/deals/' + DEAL,
        values: {
            name: [{ attribute_type: 'text', value: 'Acme pilot', active_until: null }],
            stage: stage === '' ? [] : [{ attribute_type: 'status', status: { id: { status_id: statusId }, title: stage } }],
            owner: [{ attribute_type: 'actor-reference', referenced_actor_type: 'workspace-member', referenced_actor_id: MEMBER }],
            ...extra,
        },
    };
}

const MEMBERS = [{ id: { workspace_member_id: MEMBER }, first_name: 'Ehud', last_name: 'F', email_address: 'Ehud@Example.com', access_level: 'admin' }];

// A router for the common read endpoints; `writes` answers the rest.
function attioApi(writes = () => reply(500, { message: 'unexpected write' }), record = dealRecord()) {
    return (request) => {
        const { method, path } = request;
        if (method === 'GET' && path === '/objects/deals/attributes/stage/statuses') return reply(200, { data: STAGES });
        if (method === 'GET' && path === '/objects/deals/attributes/priority/statuses') return reply(200, { data: PRIORITIES });
        if (method === 'GET' && path === '/objects/deals/attributes') return reply(200, { data: DEAL_ATTRIBUTES });
        if (method === 'GET' && path === '/workspace_members') return reply(200, { data: MEMBERS });
        if (method === 'GET' && path === `/objects/deals/records/${DEAL}`) return reply(200, { data: record });
        return writes(request);
    };
}

function writeRequests() {
    return requests.filter((request) => request.method !== 'GET');
}

function sequence() {
    return events.map((event) => event.kind === 'check' ? 'check ' + event.capability : event.method + ' ' + event.path);
}

function attioError(fn) {
    try {
        fn();
    } catch (error) {
        return error;
    }
    assert.fail('expected an error');
}

// ---------------------------------------------------------------------------
// Request construction.
// ---------------------------------------------------------------------------

test('every request is HTTPS to api.attio.com with the bearer key and a User-Agent', () => {
    reset(() => reply(200, { data: [{ id: { workspace_id: 'w', object_id: 'o' }, api_slug: 'deals', singular_noun: 'Deal', plural_noun: 'Deals', created_at: 't' }] }));
    assert.deepEqual(attio.listObjects(), [{ objectId: 'o', slug: 'deals', singularNoun: 'Deal', pluralNoun: 'Deals', createdAt: 't' }]);
    const [request] = requests;
    assert.equal(request.url, 'https://api.attio.com/v2/objects');
    assert.equal(request.headers.get('Authorization'), 'Bearer attio-test-key');
    assert.equal(request.headers.get('User-Agent'), 'submilli-attio/0.1.0');
    assert.equal(request.headers.get('Accept'), 'application/json');
    assert.deepEqual(checks, [{ capability: 'attio.com/objects.list', context: {} }]);
});

test('discovery: attributes, statuses and members drop archived entries and normalize slugs', () => {
    reset(attioApi());
    const attributes = attio.listAttributes(' Deals ');
    assert.deepEqual(checks.at(-1), { capability: 'attio.com/attributes.list', context: { object: 'deals' } });
    assert.equal(requests.at(-1).path, '/objects/deals/attributes');
    assert.ok(!attributes.some((item) => item.slug === 'legacy'));
    assert.equal(attributes.find((item) => item.slug === 'value').currencyCode, 'USD');

    const stages = attio.listStatuses('deals', 'STAGE');
    assert.deepEqual(checks.at(-1), { capability: 'attio.com/statuses.list', context: { object: 'deals', attribute: 'stage' } });
    assert.deepEqual(stages.map((stage) => stage.title), ['Lead', 'In Progress', 'Won 🎉']);

    assert.deepEqual(attio.listMembers(), [{ memberId: MEMBER, email: 'ehud@example.com', firstName: 'Ehud', lastName: 'F', accessLevel: 'admin' }]);
    assert.deepEqual(checks.at(-1), { capability: 'attio.com/members.list', context: {} });
});

test('getRecord maps readable values with member ids, without an unchecked member lookup; 404 is null', () => {
    reset(attioApi());
    const record = attio.getRecord('deals', DEAL.toUpperCase());
    assert.deepEqual(checks, [{ capability: 'attio.com/records.get', context: { object: 'deals', recordId: DEAL } }]);
    assert.deepEqual(sequence(), ['check attio.com/records.get', `GET /objects/deals/records/${DEAL}`]);
    assert.equal(record.recordId, DEAL);
    assert.equal(record.values.stage[0].value, 'Lead');
    assert.deepEqual(record.values.owner[0], { type: 'actor-reference', value: MEMBER, actorType: 'workspace-member', actorId: MEMBER });
    assert.equal(record.values.stage.length, 1);

    reset(() => reply(404, { status_code: 404, type: 'invalid_request_error', code: 'not_found', message: 'Record not found.' }));
    assert.equal(attio.getRecord('deals', DEAL), null);
    reset(() => reply(404, { status_code: 404, type: 'invalid_request_error', code: 'merge_in_progress', message: 'Merging.' }));
    assert.equal(attioError(() => attio.getRecord('deals', DEAL)).code, 'merge_in_progress');
});

test('queryRecords sends one typed page and reports the next offset; it never loops', () => {
    const rows = Array.from({ length: 3 }, () => dealRecord('In Progress', 's-progress', { owner: [] }));
    reset(() => reply(200, { data: rows }));
    const page = attio.queryRecords('Deals', {
        filter: [
            { attribute: 'stage', operator: 'eq', value: 'In Progress' },
            { attribute: 'name', operator: 'contains', value: 'Acme' },
            { attribute: 'associated_company', operator: 'eq', value: { targetObject: 'companies', targetRecordId: COMPANY } },
        ],
        sorts: [{ attribute: 'created_at', direction: 'desc' }],
        limit: 3,
        offset: 6,
    });
    assert.deepEqual(checks, [{ capability: 'attio.com/records.query', context: { object: 'deals' } }]);
    assert.equal(requests.length, 1, 'one request per page');
    assert.equal(requests[0].method, 'POST');
    assert.equal(requests[0].path, '/objects/deals/records/query');
    assert.deepEqual(requests[0].body, {
        limit: 3,
        offset: 6,
        filter: { $and: [
            { stage: 'In Progress' },
            { name: { $contains: 'Acme' } },
            { associated_company: { target_object: 'companies', target_record_id: COMPANY } },
        ] },
        sorts: [{ direction: 'desc', attribute: 'created_at' }],
    });
    assert.equal(page.items.length, 3);
    assert.equal(page.nextOffset, 9);
    assert.equal(page.isComplete, false);

    reset(() => reply(200, { data: rows.slice(0, 1) }));
    const last = attio.queryRecords('deals', { limit: 3, offset: 9 });
    assert.deepEqual(requests[0].body, { limit: 3, offset: 9 });
    assert.equal(last.nextOffset, null);
    assert.equal(last.isComplete, true);

    reset(() => reply(200, { data: [] }));
    attio.queryRecords('deals');
    assert.deepEqual(requests[0].body, { limit: 50, offset: 0 }, 'default page of 50');
});

test('findPersonByEmail checks and sends the normalized email; no match is null; two matches are an error', () => {
    const person = { id: { record_id: PERSON }, created_at: 't', web_url: 'u', values: {} };
    reset(() => reply(200, { data: [person] }));
    assert.equal(attio.findPersonByEmail('  Dana@Acme.COM ').recordId, PERSON);
    assert.deepEqual(checks, [{ capability: 'attio.com/people.find', context: { email: 'dana@acme.com' } }]);
    assert.equal(requests[0].path, '/objects/people/records/query');
    assert.deepEqual(requests[0].body, { filter: { email_addresses: 'dana@acme.com' }, limit: 2, offset: 0 });

    reset(() => reply(200, { data: [] }));
    assert.equal(attio.findPersonByEmail('dana@acme.com'), null);
    reset(() => reply(200, { data: [person, person] }));
    const error = attioError(() => attio.findPersonByEmail('dana@acme.com'));
    assert.equal(error.code, 'multiple_match_results');
    assert.match(error.message, /more than one person/);
});

test('upserts send only the given fields on the matching attribute; ambiguity is a clear error', () => {
    const company = { id: { record_id: COMPANY }, created_at: 't', web_url: 'u', values: {} };
    reset(() => reply(200, { data: company }));
    attio.upsertCompany({ domain: 'https://www.Acme.com/pricing', name: 'Acme' });
    assert.deepEqual(checks, [{ capability: 'attio.com/companies.upsert', context: { domain: 'acme.com', attributes: ['domains', 'name'] } }]);
    assert.equal(requests[0].method, 'PUT');
    assert.equal(requests[0].path, '/objects/companies/records?matching_attribute=domains');
    assert.deepEqual(requests[0].body, { data: { values: { domains: [{ domain: 'acme.com' }], name: [{ value: 'Acme' }] } } });

    reset(() => reply(200, { data: company }));
    attio.upsertCompany({ domain: 'acme.com' });
    assert.deepEqual(requests[0].body, { data: { values: { domains: [{ domain: 'acme.com' }] } } }, 'name not sent when absent');
    assert.deepEqual(checks[0].context.attributes, ['domains']);
    assert.equal(attioError(() => attio.upsertCompany({ domain: 'bücher.de' })).code, 'invalid_domain', 'non-ASCII refused');

    reset(() => reply(200, { data: { id: { record_id: PERSON }, created_at: 't', web_url: 'u', values: {} } }));
    attio.upsertPerson({ email: 'Dana@Acme.com', firstName: 'Dana', companyRecordId: COMPANY });
    assert.deepEqual(checks, [{ capability: 'attio.com/people.upsert', context: { email: 'dana@acme.com', attributes: ['company', 'email_addresses', 'name'] } }]);
    assert.equal(requests[0].path, '/objects/people/records?matching_attribute=email_addresses');
    assert.deepEqual(requests[0].body, { data: { values: {
        email_addresses: [{ email_address: 'dana@acme.com' }],
        name: [{ first_name: 'Dana', last_name: '', full_name: 'Dana' }],
        company: [{ target_object: 'companies', target_record_id: COMPANY }],
    } } });

    // Every key the upsert body carries is in the checked attributes.
    assert.deepEqual(Object.keys(requests[0].body.data.values).sort(), checks[0].context.attributes);

    for (const status of [400, 409]) {
        reset(() => reply(status, { status_code: status, type: 'invalid_request_error', code: 'multiple_match_results', message: 'Multiple records matched.' }));
        const error = attioError(() => attio.upsertPerson({ email: 'dana@acme.com' }));
        assert.equal(error.code, 'multiple_match_results');
        assert.equal(error.status, status);
        assert.match(error.message, /more than one person in Attio has the email dana@acme\.com/);
        assert.equal(error.retryable, false);
    }
});

test('createRecord resolves the status, checks it, and writes values and status id', () => {
    reset(attioApi((request) => reply(200, { data: dealRecord('Lead') })));
    attio.createRecord('deals', {
        values: [
            { attribute: 'name', type: 'text', text: 'Acme pilot' },
            { attribute: 'value', type: 'currency', amount: 5000 },
            { attribute: 'owner', type: 'member', email: 'Ehud@Example.com' },
            { attribute: 'associated_people', type: 'record', targetObject: 'people', targetRecordId: PERSON },
            { attribute: 'associated_people', type: 'record', targetObject: 'people', targetRecordId: DEAL },
        ],
        status: { attribute: 'stage', title: 'lead' },
    });
    assert.deepEqual(checks, [
        { capability: 'attio.com/statuses.list', context: { object: 'deals', attribute: 'stage' } },
        {
            capability: 'attio.com/records.create',
            context: { object: 'deals', attributes: ['associated_people', 'name', 'owner', 'value'], statusAttribute: 'stage', status: 'Lead' },
        },
    ]);
    assert.deepEqual(sequence(), [
        'check attio.com/statuses.list',
        'GET /objects/deals/attributes/stage/statuses',
        'check attio.com/records.create',
        'GET /objects/deals/attributes',
        'POST /objects/deals/records',
    ]);
    assert.deepEqual(writeRequests()[0].body, { data: { values: {
        associated_people: [
            { target_object: 'people', target_record_id: PERSON },
            { target_object: 'people', target_record_id: DEAL },
        ],
        name: [{ value: 'Acme pilot' }],
        owner: [{ workspace_member_email_address: 'ehud@example.com' }],
        value: [{ currency_value: 5000 }],
        stage: [{ status: 's-lead' }],
    } } });

    reset(attioApi((request) => reply(200, { data: dealRecord('') })));
    attio.createRecord('deals', { values: [{ attribute: 'name', type: 'text', text: 'No stage' }] });
    assert.deepEqual(checks, [{ capability: 'attio.com/records.create', context: { object: 'deals', attributes: ['name'], statusAttribute: '', status: '' } }]);
    assert.ok(!requests.some((request) => request.path.endsWith('/statuses')), 'no status lookup without a status');
});

test('createRecord refuses a status attribute anywhere in values, after the check and before any write', () => {
    for (const values of [
        [{ attribute: 'stage', type: 'select', option: 'Won 🎉' }],
        [{ attribute: 'name', type: 'text', text: 'Acme' }, { attribute: 'stage', type: 'select', option: 'Won 🎉' }],
        [{ attribute: 'name', type: 'text', text: 'Acme' }, { attribute: 'value', type: 'currency', amount: 1 }, { attribute: 'priority', type: 'select', option: 'High' }],
    ]) {
        reset(attioApi(() => reply(200, { data: dealRecord() })));
        const error = attioError(() => attio.createRecord('deals', { values, status: { attribute: 'stage', title: 'Lead' } }));
        assert.equal(error.code, 'status_attribute', JSON.stringify(values));
        assert.equal(error.status, 0);
        assert.deepEqual(writeRequests(), []);
    }
});

test('updateRecord PATCHes only the given attributes and refuses status attributes', () => {
    reset(attioApi((request) => reply(200, { data: dealRecord() })));
    attio.updateRecord('Deals', DEAL, [
        { attribute: 'Name', type: 'text', text: 'Acme pilot (paid)' },
        { attribute: 'associated_company', type: 'record', targetObject: 'companies', targetRecordId: COMPANY },
    ]);
    assert.deepEqual(checks, [{ capability: 'attio.com/records.update', context: { object: 'deals', recordId: DEAL, attributes: ['associated_company', 'name'] } }]);
    const [write] = writeRequests();
    assert.equal(write.method, 'PATCH', 'PATCH appends multi-value attributes instead of overwriting them');
    assert.equal(write.path, `/objects/deals/records/${DEAL}`);
    assert.deepEqual(write.body, { data: { values: {
        associated_company: [{ target_object: 'companies', target_record_id: COMPANY }],
        name: [{ value: 'Acme pilot (paid)' }],
    } } });

    for (const values of [
        [{ attribute: 'stage', type: 'select', option: 'Won 🎉' }],
        // A status attribute after a plain one is refused too: every entry is validated.
        [{ attribute: 'name', type: 'text', text: 'Acme' }, { attribute: 'stage', type: 'select', option: 'Won 🎉' }],
        [{ attribute: 'value', type: 'currency', amount: 1 }, { attribute: 'name', type: 'text', text: 'Acme' }, { attribute: 'priority', type: 'select', option: 'High' }],
    ]) {
        reset(attioApi(() => reply(200, { data: dealRecord() })));
        const error = attioError(() => attio.updateRecord('deals', DEAL, values));
        assert.equal(error.code, 'status_attribute', JSON.stringify(values));
        assert.match(error.message, /setStatus/);
        assert.deepEqual(writeRequests(), [], 'nothing written');
    }

    reset(attioApi());
    assert.equal(attioError(() => attio.updateRecord('deals', DEAL, [{ attribute: 'nickname', type: 'text', text: 'x' }])).code, 'unknown_attribute');
    assert.equal(attioError(() => attio.updateRecord('deals', DEAL, [
        { attribute: 'name', type: 'text', text: 'a' }, { attribute: 'name', type: 'text', text: 'b' },
    ])).code, 'invalid_value', 'two values for a single-value attribute');
    assert.equal(attioError(() => attio.updateRecord('deals', DEAL, [{ attribute: 'value', type: 'text', text: '5000' }])).code, 'invalid_value');
    assert.equal(attioError(() => attio.updateRecord('deals', DEAL, [])).code, 'empty_update');
    assert.deepEqual(writeRequests(), []);
});

test('setStatus resolves the title, reports the previous status, and writes the status id by PATCH', () => {
    reset(attioApi((request) => reply(200, { data: dealRecord('Won 🎉', 's-won') })));
    const change = attio.setStatus('deals', DEAL, 'Stage', '  WON 🎉 ');
    assert.deepEqual(checks, [
        { capability: 'attio.com/statuses.list', context: { object: 'deals', attribute: 'stage' } },
        { capability: 'attio.com/records.get', context: { object: 'deals', recordId: DEAL } },
        {
            capability: 'attio.com/records.setStatus',
            context: { object: 'deals', recordId: DEAL, attribute: 'stage', status: 'Won 🎉', previousStatus: 'Lead' },
        },
    ]);
    assert.deepEqual(sequence(), [
        'check attio.com/statuses.list',
        'GET /objects/deals/attributes/stage/statuses',
        'check attio.com/records.get',
        `GET /objects/deals/records/${DEAL}`,
        'check attio.com/records.setStatus',
        `PATCH /objects/deals/records/${DEAL}`,
    ]);
    assert.deepEqual(writeRequests()[0].body, { data: { values: { stage: [{ status: 's-won' }] } } });
    assert.ok(!requests.some((request) => request.path.includes('/attributes/stage/values')), 'never the attribute-values endpoint');
    assert.deepEqual({ status: change.status, previousStatus: change.previousStatus, changed: change.changed }, { status: 'Won 🎉', previousStatus: 'Lead', changed: true });
    assert.equal(change.record.values.stage[0].value, 'Won 🎉');

    reset(attioApi(() => reply(200, { data: dealRecord('In Progress', 's-progress') })));
    attio.setStatus('deals', DEAL, 'stage', 's-progress');
    assert.equal(statusCheck().status, 'In Progress', 'a status id resolves to its canonical title');

    reset(attioApi(() => reply(200, { data: dealRecord() }), dealRecord('', '')));
    attio.setStatus('deals', DEAL, 'stage', 'Lead');
    assert.equal(statusCheck().previousStatus, '', 'no previous status');
});

function statusCheck() {
    const found = checks.filter((entry) => entry.capability === 'attio.com/records.setStatus');
    assert.equal(found.length, 1);
    return found[0].context;
}

test('setStatus checks the attribute it changes, not a fixed one', () => {
    reset(attioApi(() => reply(200, { data: dealRecord() })));
    const change = attio.setStatus('deals', DEAL, 'Priority', 'high');
    assert.deepEqual(statusCheck(), { object: 'deals', recordId: DEAL, attribute: 'priority', status: 'High', previousStatus: '' });
    assert.deepEqual(checks[0], { capability: 'attio.com/statuses.list', context: { object: 'deals', attribute: 'priority' } });
    assert.equal(requests[0].path, '/objects/deals/attributes/priority/statuses');
    assert.deepEqual(writeRequests()[0].body, { data: { values: { priority: [{ status: 'p-high' }] } } });
    assert.equal(change.status, 'High');

    // A rule that allows only `stage` must see `priority` and deny it: the write never happens.
    reset(attioApi(() => reply(200, { data: dealRecord() })));
    denied.add('attio.com/records.setStatus');
    assert.throws(() => attio.setStatus('deals', DEAL, 'priority', 'High'), /Capability denied/);
    assert.equal(statusCheck().attribute, 'priority');
    assert.deepEqual(writeRequests(), []);
});

test('setStatus and createRecord check each read before it happens', () => {
    for (const [capability, call, reads] of [
        ['attio.com/statuses.list', () => attio.setStatus('deals', DEAL, 'stage', 'Won 🎉'), 0],
        ['attio.com/records.get', () => attio.setStatus('deals', DEAL, 'stage', 'Won 🎉'), 1],
        ['attio.com/statuses.list', () => attio.createRecord('deals', { values: [{ attribute: 'name', type: 'text', text: 'x' }], status: { attribute: 'stage', title: 'Lead' } }), 0],
    ]) {
        reset(attioApi(() => reply(200, { data: dealRecord() })));
        denied.add(capability);
        const error = attioError(call);
        assert.match(error.message, /Capability denied/);
        assert.equal(checks.at(-1).capability, capability);
        assert.equal(requests.length, reads, capability + ' is checked before its read');
        assert.ok(!error.message.includes('Lead'), 'a denial does not reveal the current status');
    }
    // A missing record is not revealed to a caller who may not read records.
    reset((request) => request.path.endsWith('/statuses') ? reply(200, { data: STAGES }) : reply(404, { code: 'not_found', message: 'Record not found.' }));
    denied.add('attio.com/records.get');
    const hidden = attioError(() => attio.setStatus('deals', DEAL, 'stage', 'Won 🎉'));
    assert.match(hidden.message, /Capability denied/);
    assert.ok(!requests.some((request) => request.path.includes('/records/')), 'the record is never requested');
});

test('setStatus refuses unknown and archived statuses before the check, and skips a no-op write', () => {
    reset(attioApi());
    const error = attioError(() => attio.setStatus('deals', DEAL, 'stage', 'Parked'));
    assert.equal(error.code, 'unknown_status');
    assert.match(error.message, /"Lead", "In Progress", "Won 🎉"/);
    assert.deepEqual(checks.map((entry) => entry.capability), ['attio.com/statuses.list'], 'refused before the setStatus check');
    assert.deepEqual(writeRequests(), []);

    reset(attioApi());
    const same = attio.setStatus('deals', DEAL, 'stage', 'lead');
    assert.equal(same.changed, false);
    statusCheck();
    assert.deepEqual(writeRequests(), []);

    reset((request) => request.path.endsWith('/statuses') ? reply(200, { data: STAGES }) : reply(404, { code: 'not_found', message: 'Record not found.' }));
    const missing = attioError(() => attio.setStatus('deals', DEAL, 'stage', 'Won 🎉'));
    assert.equal(missing.code, 'not_found');
    assert.equal(missing.status, 404);
});

test('a denied check sends no write', () => {
    for (const [capability, call] of [
        ['attio.com/records.setStatus', () => attio.setStatus('deals', DEAL, 'stage', 'Won 🎉')],
        ['attio.com/records.update', () => attio.updateRecord('deals', DEAL, [{ attribute: 'name', type: 'text', text: 'x' }])],
        ['attio.com/records.create', () => attio.createRecord('deals', { values: [{ attribute: 'name', type: 'text', text: 'x' }], status: { attribute: 'stage', title: 'Lead' } })],
        ['attio.com/notes.create', () => attio.createNote('deals', DEAL, { title: 'Met', content: 'x' })],
        ['attio.com/companies.upsert', () => attio.upsertCompany({ domain: 'acme.com' })],
        ['attio.com/people.upsert', () => attio.upsertPerson({ email: 'dana@acme.com' })],
    ]) {
        reset(attioApi());
        denied.add(capability);
        assert.throws(call, /Capability denied/);
        assert.deepEqual(writeRequests(), [], capability);
        assert.equal(checks.at(-1).capability, capability);
    }
});

test('invalid input fails before the check and before the secret is read', () => {
    for (const call of [
        () => attio.getRecord('97052eb9-e65e-443f-a297-f2d9a4a7f795', DEAL),
        () => attio.queryRecords('deals', { limit: 0 }),
        () => attio.findPersonByEmail('not-an-email'),
        () => attio.upsertCompany({ domain: 'dana@acme.com' }),
        () => attio.updateRecord('deals', DEAL, [{ attribute: 'name', type: 'text', number: 1 }]),
        () => attio.setStatus('deals', 'not-a-uuid', 'stage', 'Won'),
        () => attio.createNote('deals', DEAL, { title: '', content: 'x' }),
        () => attio.listNotes('deals', DEAL, { offset: -1 }),
    ]) {
        reset(attioApi());
        const error = attioError(call);
        assert.equal(error.name, 'AttioError');
        assert.equal(error.status, 0);
        assert.deepEqual(checks, []);
        assert.equal(secretReads, 0);
        assert.deepEqual(requests, []);
    }
});

test('notes: create sends the parent and format; list sends both parent filters and keeps only that record', () => {
    const note = (parent) => ({ id: { workspace_id: 'w', note_id: 'n-' + parent }, parent_object: 'deals', parent_record_id: parent,
        title: 'Met', content_plaintext: 'Wants a demo', content_markdown: 'Wants a demo', created_at: 't' });
    reset(() => reply(200, { data: note(DEAL) }));
    attio.createNote('Deals', DEAL, { title: ' Met at meetup ', content: 'Wants a demo\nhttps://slack.example/p1', format: 'markdown' });
    assert.deepEqual(checks, [{ capability: 'attio.com/notes.create', context: { parentObject: 'deals', parentRecordId: DEAL } }]);
    assert.equal(requests[0].path, '/notes');
    assert.deepEqual(requests[0].body, { data: { parent_object: 'deals', parent_record_id: DEAL, title: 'Met at meetup', format: 'markdown',
        content: 'Wants a demo\nhttps://slack.example/p1' } });

    const otherObject = { ...note(DEAL), parent_object: 'people' };
    reset(() => reply(200, { data: [note(DEAL), note(COMPANY), otherObject] }));
    const page = attio.listNotes('deals', DEAL, { limit: 3 });
    assert.deepEqual(checks, [{ capability: 'attio.com/notes.list', context: { parentObject: 'deals', parentRecordId: DEAL } }]);
    assert.equal(requests[0].path, `/notes?parent_object=deals&parent_record_id=${DEAL}&limit=3&offset=0`);
    assert.deepEqual(page.items.map((item) => item.parentRecordId), [DEAL], 'notes of another record or object are dropped');
    assert.equal(page.nextOffset, 3, 'pagination counts what Attio returned');
    assert.equal(page.isComplete, false);
});

test('429 carries Retry-After and is retryable; 409 concurrent_write_conflict is retryable', () => {
    reset(() => reply(429, { status_code: 429, type: 'rate_limit_error', code: 'rate_limit_exceeded', message: 'Rate limit exceeded, please try again later' },
        { 'retry-after': 'Tue, 23 May 2023 14:42:01 GMT' }));
    const limited = attioError(() => attio.listObjects());
    assert.equal(limited.name, 'AttioError');
    assert.equal(limited.code, 'rate_limit_exceeded');
    assert.equal(limited.status, 429);
    assert.equal(limited.type, 'rate_limit_error');
    assert.equal(limited.retryable, true);
    assert.equal(limited.retryAfter, 'Tue, 23 May 2023 14:42:01 GMT');

    reset(attioApi(() => reply(409, { status_code: 409, type: 'invalid_request_error', code: 'concurrent_write_conflict', message: 'The record was modified by another request.' })));
    const conflict = attioError(() => attio.updateRecord('deals', DEAL, [{ attribute: 'name', type: 'text', text: 'x' }]));
    assert.equal(conflict.code, 'concurrent_write_conflict');
    assert.equal(conflict.status, 409);
    assert.equal(conflict.retryable, true);
    assert.equal(conflict.retryAfter, '');

    reset(attioApi(() => reply(409, { code: 'uniqueness_conflict', message: 'Duplicate.' })));
    assert.equal(attioError(() => attio.createRecord('deals', { values: [{ attribute: 'name', type: 'text', text: 'x' }] })).retryable, false);

    reset(() => reply(500, '<html>oops</html>'));
    const server = attioError(() => attio.listMembers());
    assert.equal(server.code, 'server_error');
    assert.match(server.message, /HTTP 500/);
});

// ---------------------------------------------------------------------------
// The checked values are the values sent: inputs whose getters change answer.
// ---------------------------------------------------------------------------

function flipping(first, later) {
    let reads = 0;
    return { reads: () => reads, get value() { reads += 1; return reads === 1 ? first : later; } };
}

test('check() sees exactly the values the request uses, even when inputs change on a second read', () => {
    // An attribute slug that would say `name` to the check and `stage` to the request.
    const slug = flipping('name', 'stage');
    reset(attioApi((request) => reply(200, { data: dealRecord() })));
    attio.updateRecord('deals', DEAL, [{ get attribute() { return slug.value; }, type: 'text', text: 'x' }]);
    assert.equal(slug.reads(), 1, 'attribute read once');
    assert.deepEqual(checks[0].context.attributes, ['name']);
    assert.deepEqual(Object.keys(writeRequests()[0].body.data.values), ['name'], 'the request writes the checked attribute');

    // A domain that would be acme.com to the check and victim.com to the request.
    const domain = flipping('acme.com', 'victim.com');
    reset(() => reply(200, { data: { id: { record_id: COMPANY }, created_at: 't', web_url: 'u', values: {} } }));
    attio.upsertCompany({ get domain() { return domain.value; } });
    assert.equal(domain.reads(), 1);
    assert.equal(checks[0].context.domain, 'acme.com');
    assert.deepEqual(requests[0].body.data.values.domains, [{ domain: 'acme.com' }]);

    // An email for upsertPerson.
    const email = flipping('dana@acme.com', 'ceo@victim.com');
    reset(() => reply(200, { data: { id: { record_id: PERSON }, created_at: 't', web_url: 'u', values: {} } }));
    attio.upsertPerson({ get email() { return email.value; } });
    assert.equal(email.reads(), 1);
    assert.equal(checks[0].context.email, 'dana@acme.com');
    assert.deepEqual(requests[0].body.data.values.email_addresses, [{ email_address: 'dana@acme.com' }]);

    // A status attribute and title for createRecord.
    const attribute = flipping('stage', 'other_stage');
    const title = flipping('Lead', 'Won 🎉');
    reset(attioApi((request) => reply(200, { data: dealRecord() })));
    attio.createRecord('deals', {
        values: [{ attribute: 'name', type: 'text', text: 'x' }],
        status: { get attribute() { return attribute.value; }, get title() { return title.value; } },
    });
    assert.equal(attribute.reads(), 1);
    assert.equal(title.reads(), 1);
    assert.deepEqual(checks[0].context, { object: 'deals', attribute: 'stage' });
    assert.deepEqual(checks[1].context, { object: 'deals', attributes: ['name'], statusAttribute: 'stage', status: 'Lead' });
    assert.deepEqual(writeRequests()[0].body.data.values.stage, [{ status: 's-lead' }]);
    assert.ok(!('other_stage' in writeRequests()[0].body.data.values));

    // The values list of createRecord: its entries are read once each.
    let iterations = 0;
    const values = {
        *[Symbol.iterator]() {
            iterations += 1;
            if (iterations === 1) yield { attribute: 'name', type: 'text', text: 'x' };
            else yield { attribute: 'stage', type: 'select', option: 'Won 🎉' };
        },
    };
    reset(attioApi((request) => reply(200, { data: dealRecord() })));
    attio.createRecord('deals', { values });
    assert.equal(iterations, 1, 'values iterated once');
    assert.deepEqual(checks[0].context.attributes, ['name']);
    assert.deepEqual(Object.keys(writeRequests()[0].body.data.values), ['name']);

    // Every check context equals what the request carried, operation by operation.
    reset(attioApi((request) => reply(200, { data: dealRecord() })));
    attio.setStatus(' DEALS ', DEAL.toUpperCase(), ' Stage ', 'won 🎉');
    const context = statusCheck();
    const write = writeRequests()[0];
    assert.equal(write.path, `/objects/${context.object}/records/${context.recordId}`);
    assert.deepEqual(Object.keys(write.body.data.values), [context.attribute]);
    assert.equal(STAGES.find((stage) => stage.id.status_id === write.body.data.values.stage[0].status).title, context.status);
});
