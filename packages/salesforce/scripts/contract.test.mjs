// Exercise the package's operations against a mocked Submilli host and a mocked
// Salesforce, without network access or credentials. Native build tests verify
// Submilli compatibility and the live tests Salesforce's real answers; this
// harness checks the requests each operation sends, what `check()` sees, and
// that a denied check sends no write.
//
//   node --test packages/salesforce/scripts/contract.test.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import { test } from 'node:test';

const ORG = 'https://acme.my.salesforce.com';
const DATA = `${ORG}/services/data/v68.0`;
const TOKEN = 'test-token';

const ACCOUNT = '001000000000001AAA';
const CONTACT = '003000000000001AAA';
const OPPORTUNITY = '006000000000001AAA';
const LEAD = '00Q000000000001AAA';
const OWNER = '005000000000001AAA';
const OTHER_OWNER = '005000000000002AAA';
const WORK_ITEM = '04i000000000001AAA';
const VERSION = '068000000000001AAA';
const DOCUMENT = '069000000000001AAA';
const JOB = '750000000000001AAA';

// The Node of this repository may lack Uint8Array.prototype.toBase64, which the package uses.
if (Uint8Array.prototype.toBase64 === undefined) {
    Uint8Array.prototype.toBase64 = function () { return Buffer.from(this).toString('base64'); };
}

// ---------------------------------------------------------------------------
// The mocked host
// ---------------------------------------------------------------------------

let requests = [];
let checks = [];
let routes = [];
let denied = () => false;
const files = new Map();
const removed = [];

function reset() {
    requests = [];
    checks = [];
    routes = [];
    denied = () => false;
    files.clear();
    removed.length = 0;
}

// Answer requests whose method and URL (after the org's data path) match.
function on(method, pattern, answer) {
    routes.push({ method, pattern, answer });
}

function reply(status, data, headers = {}) {
    return { status, data, headers };
}

function respond(method, url, body, headers) {
    const sent = { method, url, path: url.startsWith(DATA) ? url.slice(DATA.length) : url, body, headers };
    requests.push(sent);
    for (const route of routes) {
        if (route.method !== method) continue;
        const matched = typeof route.pattern === 'string' ? sent.path === route.pattern : route.pattern.test(sent.path);
        if (!matched) continue;
        const answer = typeof route.answer === 'function' ? route.answer(sent) : route.answer;
        return response(answer);
    }
    return response(reply(404, [{ errorCode: 'NOT_FOUND', message: 'no route for ' + method + ' ' + sent.path }]));
}

function response(answer) {
    const text = typeof answer.data === 'string' ? answer.data : answer.data === undefined ? '' : JSON.stringify(answer.data);
    const status = answer.status ?? 200;
    return {
        status, ok: status >= 200 && status < 300, statusText: '', body: text,
        headers: { get: (name) => answer.headers?.[name.toLowerCase()] ?? null },
        json: () => JSON.parse(text),
        text: () => text,
    };
}

function asObject(headers) {
    return headers instanceof Map ? Object.fromEntries(headers) : headers;
}

const host = {
    get: (url, headers) => respond('GET', url, null, asObject(headers)),
    post: (url, body, headers) => respond('POST', url, typeof body === 'string' ? body : JSON.parse(JSON.stringify(body)), asObject(headers)),
    patch: (url, body, headers) => respond('PATCH', url, JSON.parse(JSON.stringify(body)), asObject(headers)),
    httpDelete: (url, headers) => respond('DELETE', url, null, asObject(headers)),
    download: (url, path, options) => {
        const answer = respond('GET', url, null, asObject(options.headers));
        files.set(path, answer.body);
        return { status: answer.status, bytesWritten: answer.body.length };
    },
    writeText: (path, text) => { files.set(path, text); },
    appendText: (path, text) => { files.set(path, (files.get(path) ?? '') + text); },
    exists: (path) => files.has(path),
    stat: (path) => ({ kind: 'file', size: (files.get(path) ?? '').length }),
    read: (path) => new TextEncoder().encode(files.get(path) ?? ''),
    remove: (path) => { removed.push(path); files.delete(path); },
    check: (capability, context) => {
        checks.push({ capability: capability.replace('salesforce.com/', ''), context });
        if (denied(capability.replace('salesforce.com/', ''), context)) throw new Error('Capability denied');
    },
    encodeComponent: encodeURIComponent,
    secrets: { get: (name) => (name === 'SALESFORCE_ACCESS_TOKEN' ? TOKEN : undefined) },
    orgUrl: ORG,
    apiVersion: '68.0',
};

// The package's modules, merged into one: a data: URL module cannot resolve
// relative imports. `lib.ts` comes last so every helper it uses is defined.
async function loadPackage() {
    const modules = ['errors', 'request', 'response', 'soql', 'objects', 'activity', 'process', 'reports', 'metadata', 'org', 'lib'];
    const sources = await Promise.all(modules.map((name) => readFile(new URL(`../src/${name}.ts`, import.meta.url), 'utf8')));
    const source = sources.join('\n')
        .replace(/^import\s+[^;]*?from "submilli:[^"]+";\n/gm, '')
        .replace(/^(import|export) [^;]*? from "\.\/[^"]+";\n/gm, '');
    globalThis.__salesforceHost = host;
    const prelude = 'const { get, post, patch, httpDelete, download, writeText, appendText, exists, stat, read, remove, check, encodeComponent, secrets, orgUrl, apiVersion } = globalThis.__salesforceHost;\n';
    return import(`data:text/javascript;base64,${Buffer.from(prelude + stripTypeScriptTypes(source)).toString('base64')}`);
}

const sf = await loadPackage();

// Answers shared by many tests: a record read by id, and SOQL by its text.
function record(object, id, fields) {
    on('GET', `/sobjects/${object}/${id}`, reply(200, { attributes: { type: object }, Id: id, ...fields }));
}

function soqlAnswer(match, records) {
    routes.push({
        method: 'GET',
        pattern: { test: (path) => path.startsWith('/query/?q=') && decodeURIComponent(path.slice('/query/?q='.length)).includes(match) },
        answer: reply(200, { totalSize: records.length, done: true, records }),
    });
}

function soqlOf(sent) {
    return decodeURIComponent(sent.path.slice('/query/?q='.length));
}

function writes() {
    return requests.filter((sent) => sent.method !== 'GET');
}

function lastCheck() {
    return checks.at(-1);
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

test('every request goes to the org under its API version, with the token', () => {
    reset();
    soqlAnswer('FROM Account', [{ attributes: { type: 'Account' }, Id: ACCOUNT, Name: 'Acme' }]);
    const page = sf.findAccounts({ name: 'Ac' }, { pageSize: 500 });
    assert.equal(page.records[0].name, 'Acme');
    const sent = requests[0];
    assert.ok(sent.url.startsWith(`${DATA}/query/?q=`), sent.url);
    assert.equal(sent.headers.Authorization, `Bearer ${TOKEN}`);
    assert.equal(sent.headers.Accept, 'application/json');
    assert.equal(sent.headers['Sforce-Query-Options'], 'batchSize=500');
    assert.match(soqlOf(sent), /^SELECT FIELDS\(STANDARD\), .* FROM Account WHERE Name LIKE '%Ac%' ORDER BY LastModifiedDate DESC$/);
    assert.deepEqual(lastCheck(), { capability: 'accounts.find', context: { ownerId: '' } });
});

test('a cursor reads the next page by its locator', () => {
    reset();
    on('GET', /^\/query\/\?q=/, reply(200, { totalSize: 3, done: false, nextRecordsUrl: '/services/data/v68.0/query/01gD0000002HU6KIAW-2000', records: [] }));
    on('GET', '/query/01gD0000002HU6KIAW-2000', reply(200, { totalSize: 3, done: true, records: [] }));
    const first = sf.soql('SELECT Id FROM Account');
    assert.equal(first.nextCursor, '01gD0000002HU6KIAW-2000');
    const second = sf.soql('SELECT Id FROM Account', { cursor: first.nextCursor });
    assert.equal(second.nextCursor, null);
    assert.equal(requests[1].path, '/query/01gD0000002HU6KIAW-2000');
    assert.throws(() => sf.soql('SELECT Id FROM Account', { cursor: '../../sobjects' }), /cursor/);
    assert.equal(requests.length, 2, 'an invalid cursor is refused before a request');
});

test('query, aggregate, search and SOSL send what was checked', () => {
    reset();
    on('GET', /^\/query\/\?q=/, reply(200, { totalSize: 1, done: true, records: [{ attributes: { type: 'AggregateResult' }, g0: 'Prospecting', m0: 2 }] }));
    on('POST', '/parameterizedSearch', reply(200, { searchRecords: [{ attributes: { type: 'Account' }, Id: ACCOUNT, Name: "O'Brien" }] }));
    on('GET', /^\/search\/\?q=/, reply(200, { searchRecords: [] }));

    sf.query({ object: 'Opportunity', fields: ['Name', 'Account.Name'], where: [{ field: 'StageName', op: '=', value: "it's" }], limit: 5 });
    assert.equal(soqlOf(requests[0]), "SELECT Id, Name, Account.Name FROM Opportunity WHERE StageName = 'it\\'s' LIMIT 5");
    assert.deepEqual(lastCheck(), { capability: 'query.run', context: { object: 'opportunity', fields: ['account.name', 'name', 'stagename'] } }, 'Id is always read, so it is no check field');

    const totals = sf.aggregate({ object: 'Opportunity', groupBy: [{ field: 'StageName' }], measures: [{ fn: 'count' }] });
    assert.equal(soqlOf(requests[1]), 'SELECT StageName g0, COUNT(Id) m0 FROM Opportunity GROUP BY StageName LIMIT 2000');
    assert.deepEqual(totals.rows, [{ groups: { StageName: 'Prospecting' }, measures: { 'count()': 2 } }]);
    sf.aggregate({ object: 'Opportunity', measures: [{ fn: 'count' }] });
    assert.equal(soqlOf(requests[2]), 'SELECT COUNT(Id) m0 FROM Opportunity', 'an ungrouped aggregate has no LIMIT');

    const found = sf.search({ text: 'Acme {Corp}', objects: [{ object: 'Account', fields: ['Name'] }] });
    assert.equal(found[0].fields.Name, "O'Brien");
    assert.equal(requests[3].body.q, 'Acme \\{Corp\\}');
    assert.deepEqual(requests[3].body.sobjects, [{ name: 'Account', fields: ['Id', 'Name'] }]);
    assert.deepEqual(lastCheck(), { capability: 'query.search', context: { objects: ['account'], fields: ['name'] } });

    sf.sosl('FIND {Acme}');
    assert.equal(requests[4].path, '/search/?q=' + encodeURIComponent('FIND {Acme}'));
    assert.deepEqual(lastCheck(), { capability: 'query.sosl', context: {} });
});

test('Salesforce errors keep their code, fields and status; 404 reads as null', () => {
    reset();
    on('GET', `/sobjects/Account/${ACCOUNT}`, reply(404, [{ errorCode: 'NOT_FOUND', message: 'gone' }]));
    on('GET', /^\/query\/\?q=.*Bad/, reply(400, [{ errorCode: 'INVALID_FIELD', message: 'No such column', fields: ['Bad__c'] }]));
    on('GET', /^\/query\/\?q=.*Limit/, reply(403, [{ errorCode: 'REQUEST_LIMIT_EXCEEDED', message: 'TotalRequests Limit exceeded.' }]));
    on('GET', /^\/query\/\?q=.*Expired/, reply(401, [{ errorCode: 'INVALID_SESSION_ID', message: 'Session expired or invalid' }]));

    assert.equal(sf.getRecord('Account', ACCOUNT), null);
    assert.deepEqual(lastCheck().context, { object: 'account', ownerId: '', accountId: '' });
    assert.throws(() => sf.soql('SELECT Bad__c FROM Account'), (error) =>
        error.name === 'SalesforceError' && error.errorCode === 'INVALID_FIELD' && error.status === 400 && error.fields[0] === 'Bad__c' && !error.retryable);
    assert.throws(() => sf.soql('SELECT Id FROM Limit'), (error) => error.errorCode === 'REQUEST_LIMIT_EXCEEDED' && error.retryable);
    assert.throws(() => sf.soql('SELECT Id FROM Expired'), (error) => error.errorCode === 'INVALID_SESSION_ID' && error.status === 401);
});

// ---------------------------------------------------------------------------
// Writes and their checks
// ---------------------------------------------------------------------------

test('createRecord checks the fields, owner and account it sends', () => {
    reset();
    on('POST', '/sobjects/Opportunity', reply(201, { id: OPPORTUNITY, success: true }));
    const values = [
        { field: 'Name', value: 'Renewal' }, { field: 'StageName', value: 'Prospecting' },
        { field: 'CloseDate', value: '2026-12-31' }, { field: 'AccountId', value: ACCOUNT }, { field: 'OwnerId', value: OWNER },
    ];
    assert.equal(sf.createRecord('Opportunity', values), OPPORTUNITY);
    assert.deepEqual(lastCheck(), {
        capability: 'records.create',
        context: { object: 'opportunity', fields: ['accountid', 'closedate', 'name', 'ownerid', 'stagename'], ownerId: OWNER, accountId: ACCOUNT },
    });
    assert.deepEqual(requests[0].body, { Name: 'Renewal', StageName: 'Prospecting', CloseDate: '2026-12-31', AccountId: ACCOUNT, OwnerId: OWNER });

    denied = (capability) => capability === 'records.create';
    assert.throws(() => sf.createRecord('Opportunity', values), /denied/);
    assert.equal(requests.length, 1, 'a denied create sends nothing');
    assert.throws(() => sf.createRecord('Opportunity', [{ field: 'Id', value: OPPORTUNITY }]), /Id/);
});

test('updateRecord reads the owner and account first, then patches; null clears', () => {
    reset();
    record('Opportunity', OPPORTUNITY, { OwnerId: OWNER, AccountId: ACCOUNT });
    on('PATCH', `/sobjects/Opportunity/${OPPORTUNITY}`, reply(204));
    sf.updateRecord('Opportunity', OPPORTUNITY, [{ field: 'StageName', value: 'Closed Won' }, { field: 'NextStep', value: null }]);
    assert.deepEqual(lastCheck(), {
        capability: 'records.update',
        context: { object: 'opportunity', fields: ['nextstep', 'stagename'], ownerId: OWNER, accountId: ACCOUNT },
    });
    assert.deepEqual(requests.map((sent) => sent.method), ['GET', 'PATCH']);
    assert.deepEqual(requests[1].body, { StageName: 'Closed Won', NextStep: null });

    reset();
    record('Opportunity', OPPORTUNITY, { OwnerId: OTHER_OWNER, AccountId: ACCOUNT });
    denied = (capability, context) => capability === 'records.update' && context.ownerId !== OWNER;
    assert.throws(() => sf.updateRecord('Opportunity', OPPORTUNITY, [{ field: 'StageName', value: 'Closed Won' }]), /denied/);
    assert.deepEqual(writes(), [], "another owner's record is not patched");
    assert.throws(() => sf.updateRecord('Opportunity', OPPORTUNITY, [{ field: 'OwnerId', value: OWNER }]), /changeOwner|OwnerId/);
});

test('deleteRecord reports whether it deleted', () => {
    reset();
    record('Account', ACCOUNT, { OwnerId: OWNER });
    on('DELETE', `/sobjects/Account/${ACCOUNT}`, reply(204));
    assert.equal(sf.deleteRecord('Account', ACCOUNT), true);
    assert.equal(requests.at(-1).method, 'DELETE');
    assert.deepEqual(lastCheck().context, { object: 'account', ownerId: OWNER, accountId: ACCOUNT });

    reset();
    assert.equal(sf.deleteRecord('Account', ACCOUNT), false);
    assert.deepEqual(writes(), [], 'an absent record is not deleted');
});

test('upsertRecord patches by external id and reads back an update without a body', () => {
    reset();
    on('PATCH', '/sobjects/Account/Erp_Id__c/A%2F1', reply(201, { id: ACCOUNT, success: true, created: true }));
    assert.deepEqual(sf.upsertRecord('Account', 'Erp_Id__c', 'A/1', [{ field: 'Name', value: 'Acme' }]), { id: ACCOUNT, created: true });
    assert.deepEqual(lastCheck(), {
        capability: 'records.upsert',
        context: { object: 'account', externalIdField: 'erp_id__c', externalId: 'A/1', fields: ['name'] },
    });

    reset();
    on('PATCH', '/sobjects/Account/Erp_Id__c/A1', reply(204));
    soqlAnswer("WHERE Erp_Id__c = 'A1'", [{ attributes: { type: 'Account' }, Id: ACCOUNT }]);
    assert.deepEqual(sf.upsertRecord('Account', 'Erp_Id__c', 'A1', [{ field: 'Name', value: 'Acme' }]), { id: ACCOUNT, created: false });
    assert.throws(() => sf.upsertRecord('Account', 'Erp_Id__c', 'A1', [{ field: 'Erp_Id__c', value: 'B' }]), /external id/);
});

test('batch writes are all or none, and check every owner and account', () => {
    reset();
    on('POST', '/composite/sobjects', reply(200, [{ id: CONTACT, success: true, errors: [] }, { success: false, errors: [{ statusCode: 'ALL_OR_NONE_OPERATION_ROLLED_BACK', message: 'rolled back', fields: [] }] }]));
    const outcomes = sf.createRecords('Contact', [
        [{ field: 'LastName', value: 'A' }, { field: 'AccountId', value: ACCOUNT }],
        [{ field: 'LastName', value: 'B' }, { field: 'OwnerId', value: OWNER }],
    ]);
    assert.deepEqual(outcomes[1], { id: null, success: false, errors: [{ errorCode: 'ALL_OR_NONE_OPERATION_ROLLED_BACK', message: 'rolled back', fields: [] }] });
    assert.deepEqual(requests[0].body, {
        allOrNone: true,
        records: [
            { attributes: { type: 'Contact' }, LastName: 'A', AccountId: ACCOUNT },
            { attributes: { type: 'Contact' }, LastName: 'B', OwnerId: OWNER },
        ],
    });
    assert.deepEqual(lastCheck().context, { object: 'contact', fields: ['accountid', 'lastname', 'ownerid'], count: 2, ownerIds: ['', OWNER], accountIds: ['', ACCOUNT] });

    reset();
    soqlAnswer('FROM Contact WHERE Id IN', [{ attributes: { type: 'Contact' }, Id: CONTACT, OwnerId: OWNER, AccountId: ACCOUNT }]);
    on('PATCH', '/composite/sobjects', reply(200, [{ id: CONTACT, success: true, errors: [] }]));
    sf.updateRecords('Contact', [{ id: CONTACT, values: [{ field: 'Title', value: 'CTO' }] }]);
    assert.equal(soqlOf(requests[0]), `SELECT FIELDS(STANDARD) FROM Contact WHERE Id IN ('${CONTACT}')`);
    assert.deepEqual(requests[1].body, { allOrNone: true, records: [{ attributes: { type: 'Contact' }, Title: 'CTO', Id: CONTACT }] });
    assert.deepEqual(lastCheck().context, { object: 'contact', fields: ['title'], count: 1, ownerIds: [OWNER], accountIds: [ACCOUNT] });
    assert.throws(() => sf.updateRecords('Contact', [{ id: CONTACT, values: [{ field: 'Title', value: 'A' }] }, { id: CONTACT, values: [{ field: 'Title', value: 'B' }] }]), /repeats/);
});

test('changeOwner checks both owners and patches only OwnerId', () => {
    reset();
    record('Opportunity', OPPORTUNITY, { OwnerId: OWNER, AccountId: ACCOUNT });
    on('PATCH', `/sobjects/Opportunity/${OPPORTUNITY}`, reply(204));
    sf.changeOwner('Opportunity', OPPORTUNITY, OTHER_OWNER);
    assert.deepEqual(lastCheck(), {
        capability: 'records.changeOwner',
        context: { object: 'opportunity', fromOwnerId: OWNER, toOwnerId: OTHER_OWNER, accountId: ACCOUNT },
    });
    assert.deepEqual(requests.at(-1).body, { OwnerId: OTHER_OWNER });
});

// ---------------------------------------------------------------------------
// Collaboration
// ---------------------------------------------------------------------------

test('logActivity sets the subtype only, and checks the account of the record or the contact', () => {
    reset();
    record('Opportunity', OPPORTUNITY, { OwnerId: OWNER, AccountId: ACCOUNT });
    on('POST', '/sobjects/Task', reply(201, { id: '00T000000000001AAA', success: true }));
    sf.logActivity({ type: 'call', subject: 'Renewal call', whatId: OPPORTUNITY, date: '2026-11-19' });
    assert.deepEqual(requests.at(-1).body, { TaskSubtype: 'Call', Subject: 'Renewal call', Status: 'Completed', WhatId: OPPORTUNITY, ActivityDate: '2026-11-19' });
    assert.deepEqual(lastCheck().context, { type: 'call', object: 'opportunity', accountId: ACCOUNT, fields: ['activitydate', 'status', 'subject', 'tasksubtype', 'whatid'] });

    reset();
    record('Contact', CONTACT, { OwnerId: OWNER, AccountId: ACCOUNT });
    on('POST', '/sobjects/Task', reply(201, { id: '00T000000000001AAA', success: true }));
    sf.logActivity({ type: 'email', subject: 'Sent terms', whoId: CONTACT });
    assert.equal(lastCheck().context.accountId, ACCOUNT, "a contact's account stands in for a missing whatId");
    assert.throws(() => sf.logActivity({ type: 'meeting', subject: 'Lunch' }), /call, email or task/);
});

test('createNote links the note to the record, and removes it when linking fails', () => {
    reset();
    record('Account', ACCOUNT, { OwnerId: OWNER });
    on('POST', '/sobjects/ContentNote', reply(201, { id: DOCUMENT, success: true }));
    on('POST', '/sobjects/ContentDocumentLink', reply(201, { id: '06A000000000001AAA', success: true }));
    assert.equal(sf.createNote(ACCOUNT, 'Call notes', 'a < b\nsecond'), DOCUMENT);
    const note = requests.find((sent) => sent.path === '/sobjects/ContentNote');
    assert.equal(Buffer.from(note.body.Content, 'base64').toString(), '<p>a &lt; b</p><p>second</p>');
    assert.deepEqual(requests.find((sent) => sent.path === '/sobjects/ContentDocumentLink').body, { ContentDocumentId: DOCUMENT, LinkedEntityId: ACCOUNT, ShareType: 'I' });

    reset();
    record('Account', ACCOUNT, { OwnerId: OWNER });
    on('POST', '/sobjects/ContentNote', reply(201, { id: DOCUMENT, success: true }));
    on('POST', '/sobjects/ContentDocumentLink', reply(400, [{ errorCode: 'INSUFFICIENT_ACCESS', message: 'no' }]));
    on('DELETE', `/sobjects/ContentNote/${DOCUMENT}`, reply(204));
    assert.throws(() => sf.createNote(ACCOUNT, 'Call notes', 'x'), (error) => error.errorCode === 'INSUFFICIENT_ACCESS');
    assert.equal(requests.at(-1).method, 'DELETE', 'the unlinked note is removed');

    reset();
    record('Account', ACCOUNT, { OwnerId: OWNER });
    assert.throws(() => sf.createNote(ACCOUNT, 'Call notes', 'x'), /Notes turned off/);
});

test('files upload to the record and download from their latest version', () => {
    reset();
    record('Account', ACCOUNT, { OwnerId: OWNER });
    on('POST', '/sobjects/ContentVersion', reply(201, { id: VERSION, success: true }));
    soqlAnswer('FROM ContentVersion', [{ attributes: { type: 'ContentVersion' }, Id: VERSION, ContentDocumentId: DOCUMENT }]);
    files.set('/out/q3.csv', 'a,b\n1,2\n');
    assert.deepEqual(sf.uploadFile(ACCOUNT, '/out/q3.csv'), { contentDocumentId: DOCUMENT, contentVersionId: VERSION });
    const upload = requests.find((sent) => sent.path === '/sobjects/ContentVersion');
    assert.deepEqual(upload.body, { Title: 'q3.csv', PathOnClient: 'q3.csv', VersionData: Buffer.from('a,b\n1,2\n').toString('base64'), FirstPublishLocationId: ACCOUNT });
    assert.deepEqual(lastCheck().context, { object: 'account', accountId: ACCOUNT, ownerId: OWNER, path: '/out/q3.csv' });
    assert.throws(() => sf.uploadFile(ACCOUNT, '/missing.txt'), /not a VFS file/);

    reset();
    soqlAnswer('FROM ContentDocument', [{ attributes: { type: 'ContentDocument' }, Id: DOCUMENT, Title: 'Q3', FileExtension: 'csv', LatestPublishedVersionId: VERSION }]);
    on('GET', `/sobjects/ContentVersion/${VERSION}/VersionData`, reply(200, 'a,b\n'));
    assert.deepEqual(sf.downloadFile(DOCUMENT, '/in/q3.csv'), { path: '/in/q3.csv', bytes: 4, title: 'Q3', fileExtension: 'csv' });
    assert.equal(requests.at(-1).headers.Authorization, `Bearer ${TOKEN}`);

    reset();
    soqlAnswer('FROM ContentDocument', [{ attributes: { type: 'ContentDocument' }, Id: DOCUMENT, Title: 'Q3', LatestPublishedVersionId: VERSION }]);
    on('GET', `/sobjects/ContentVersion/${VERSION}/VersionData`, reply(403, [{ errorCode: 'FORBIDDEN', message: 'no' }]));
    assert.throws(() => sf.downloadFile(DOCUMENT, '/in/q3.csv'), (error) => error.status === 403);
    assert.deepEqual(removed, ['/in/q3.csv'], "an error body written in the file's place is removed");
});

test('postToFeed sends text then each mention, and checks the mentions', () => {
    reset();
    record('Account', ACCOUNT, { OwnerId: OWNER });
    on('POST', '/chatter/feed-elements', reply(201, { id: '0D5000000000001AAA' }));
    sf.postToFeed(ACCOUNT, 'Renewal signed', { mentions: [OTHER_OWNER, OWNER, OTHER_OWNER] });
    assert.deepEqual(requests.at(-1).body, {
        feedElementType: 'FeedItem', subjectId: ACCOUNT,
        body: { messageSegments: [
            { type: 'Text', text: 'Renewal signed' },
            { type: 'Text', text: ' ' }, { type: 'Mention', id: OTHER_OWNER },
            { type: 'Text', text: ' ' }, { type: 'Mention', id: OWNER },
        ] },
    });
    assert.deepEqual(lastCheck().context.mentions, [OWNER, OTHER_OWNER]);
});

test('sendEmail runs emailSimple as the user and checks the recipients and their domain', () => {
    reset();
    record('Opportunity', OPPORTUNITY, { OwnerId: OWNER, AccountId: ACCOUNT });
    on('POST', '/actions/standard/emailSimple', reply(200, [{ isSuccess: true, outputValues: {} }]));
    sf.sendEmail({ to: ['Dana@Acme.com', 'lee@acme.com'], subject: 'Terms', body: 'Attached.', relatedTo: OPPORTUNITY });
    assert.deepEqual(requests.at(-1).body, { inputs: [{
        emailAddresses: 'dana@acme.com,lee@acme.com', emailSubject: 'Terms', emailBody: 'Attached.', senderType: 'CurrentUser',
        relatedRecordId: OPPORTUNITY, logEmailOnSend: true,
    }] });
    assert.deepEqual(lastCheck().context, { to: ['dana@acme.com', 'lee@acme.com'], domains: ['acme.com'], domain: 'acme.com', object: 'opportunity', accountId: ACCOUNT });
    assert.throws(() => sf.sendEmail({ to: ['a@b.com'], subject: 'S', body: 'B', relatedTo: CONTACT }), /a contact, lead or user cannot be/);

    reset();
    on('POST', '/actions/standard/emailSimple', reply(200, [{ isSuccess: false, errors: [{ statusCode: 'NO_MASS_MAIL_PERMISSION', message: 'not allowed' }] }]));
    assert.throws(() => sf.sendEmail({ to: ['a@b.com'], subject: 'S', body: 'B' }), (error) => error.errorCode === 'NO_MASS_MAIL_PERMISSION');
    // A refused action answers 400 with the same result shape.
    reset();
    on('POST', '/actions/standard/emailSimple', reply(400, [{ isSuccess: false, errors: [{ statusCode: 'UNKNOWN_EXCEPTION', message: 'SaveAsActivity is not allowed', fields: [] }] }]));
    assert.throws(() => sf.sendEmail({ to: ['a@b.com'], subject: 'S', body: 'B' }), (error) => error.errorCode === 'UNKNOWN_EXCEPTION' && /SaveAsActivity is not allowed/.test(error.message));
    assert.throws(() => sf.sendEmail({ to: ['a@b.com', 'c@d.com', 'e@f.com', 'g@h.com', 'i@j.com', 'k@l.com'], subject: 'S', body: 'B' }), /1 to 5/);
});

// ---------------------------------------------------------------------------
// Process
// ---------------------------------------------------------------------------

const approved = [{ success: true, instanceId: '04g000000000001AAA', instanceStatus: 'Pending', entityId: OPPORTUNITY, newWorkitemIds: [WORK_ITEM], actorIds: [OWNER] }];

test('approvals: submit names the process; a decision checks the work item\'s record', () => {
    reset();
    record('Opportunity', OPPORTUNITY, { OwnerId: OWNER, AccountId: ACCOUNT });
    on('POST', '/process/approvals', reply(200, approved));
    const result = sf.submitForApproval(OPPORTUNITY, { comment: 'Discount', processName: 'Discount_Approval' });
    assert.deepEqual(result.workItemIds, [WORK_ITEM]);
    assert.deepEqual(requests.at(-1).body, { requests: [{ actionType: 'Submit', contextId: OPPORTUNITY, comments: 'Discount', processDefinitionNameOrId: 'Discount_Approval' }] });
    assert.deepEqual(lastCheck().context, { object: 'opportunity', processName: 'discount_approval', accountId: ACCOUNT, ownerId: OWNER });

    reset();
    soqlAnswer('FROM ProcessInstanceWorkitem', [{
        attributes: { type: 'ProcessInstanceWorkitem' }, Id: WORK_ITEM,
        ProcessInstance: { TargetObjectId: OPPORTUNITY, TargetObject: { Type: 'Opportunity' }, ProcessDefinition: { DeveloperName: 'Discount_Approval' } },
    }]);
    record('Opportunity', OPPORTUNITY, { OwnerId: OWNER, AccountId: ACCOUNT });
    on('POST', '/process/approvals', reply(200, approved));
    sf.approve(WORK_ITEM, { comment: 'OK' });
    assert.deepEqual(lastCheck(), { capability: 'approvals.approve', context: { object: 'opportunity', processName: 'discount_approval', accountId: ACCOUNT } });
    assert.deepEqual(requests.at(-1).body, { requests: [{ actionType: 'Approve', contextId: WORK_ITEM, comments: 'OK' }] });

    denied = (capability) => capability === 'approvals.reject';
    assert.throws(() => sf.reject(WORK_ITEM), /denied/);
    assert.equal(writes().length, 1, 'a denied rejection is not sent');
});

test('runFlow sends one set of inputs and returns the outputs', () => {
    reset();
    on('POST', '/actions/custom/flow/Renewal_Quote', reply(200, [{ isSuccess: true, outputValues: { QuoteId: '0Q0000000000001AAA', Lines: [1, 2] } }]));
    const result = sf.runFlow('Renewal_Quote', [{ name: 'OpportunityId', value: OPPORTUNITY }]);
    assert.deepEqual(requests.at(-1).body, { inputs: [{ OpportunityId: OPPORTUNITY }] });
    assert.deepEqual(result.outputs, { QuoteId: '0Q0000000000001AAA', Lines: '[1,2]' });
    assert.deepEqual(lastCheck(), { capability: 'flows.run', context: { flow: 'renewal_quote', inputs: ['opportunityid'] } });
    assert.throws(() => sf.runFlow('ns.Flow'), /API name/);
});

test('convertLead sends a SOAP call with the token in its session header', () => {
    reset();
    soqlAnswer('FROM Lead WHERE', [{ attributes: { type: 'Lead' }, Id: LEAD, OwnerId: OWNER }]);
    soqlAnswer('FROM LeadStatus', [{ attributes: { type: 'LeadStatus' }, MasterLabel: 'Closed - Converted' }]);
    on('POST', `${ORG}/services/Soap/u/68.0`, reply(200,
        '<soapenv:Envelope><soapenv:Body><convertLeadResponse><result>' +
        `<accountId>${ACCOUNT}</accountId><contactId>${CONTACT}</contactId><leadId>${LEAD}</leadId>` +
        `<opportunityId>${OPPORTUNITY}</opportunityId><success>true</success></result></convertLeadResponse></soapenv:Body></soapenv:Envelope>`));
    const converted = sf.convertLead(LEAD, { opportunityName: 'Acme & Co' });
    assert.deepEqual(converted, { leadId: LEAD, accountId: ACCOUNT, contactId: CONTACT, opportunityId: OPPORTUNITY });
    const soap = requests.at(-1);
    assert.equal(soap.headers.SOAPAction, '""');
    assert.equal(soap.headers.Authorization, undefined, 'SOAP carries the token in the envelope, not a header');
    assert.ok(soap.body.includes(`<urn:sessionId>${TOKEN}</urn:sessionId>`));
    assert.ok(soap.body.includes('<urn:convertedStatus>Closed - Converted</urn:convertedStatus><urn:doNotCreateOpportunity>false</urn:doNotCreateOpportunity>'));
    assert.ok(soap.body.includes('<urn:opportunityName>Acme &amp; Co</urn:opportunityName>'));
    assert.deepEqual(lastCheck().context, { ownerId: OWNER, toOwnerId: '', accountId: '', contactId: '', createOpportunity: true });

    reset();
    soqlAnswer('FROM Lead WHERE', [{ attributes: { type: 'Lead' }, Id: LEAD, OwnerId: OWNER }]);
    on('POST', `${ORG}/services/Soap/u/68.0`, reply(500, '<soapenv:Fault><faultcode>sf:INVALID_CROSS_REFERENCE_KEY</faultcode><faultstring>bad id</faultstring></soapenv:Fault>'));
    assert.throws(() => sf.convertLead(LEAD, { convertedStatus: 'Converted' }), (error) => error.errorCode === 'INVALID_CROSS_REFERENCE_KEY' && error.status === 500);
    assert.throws(() => sf.convertLead(LEAD, { contactId: CONTACT }), /accountId/);
});

// ---------------------------------------------------------------------------
// Reports and exports
// ---------------------------------------------------------------------------

test('runReport adds filters to the report\'s own metadata', () => {
    const report = '00O000000000001EAA';
    const result = {
        attributes: { reportId: report, reportName: 'Pipeline' },
        reportMetadata: { reportFormat: 'TABULAR', detailColumns: ['AMOUNT'], aggregates: ['RowCount'] },
        reportExtendedMetadata: { detailColumnInfo: { AMOUNT: { label: 'Amount', dataType: 'currency' } }, groupingColumnInfo: {}, aggregateColumnInfo: { RowCount: { label: 'Record Count', dataType: 'int' } } },
        factMap: { 'T!T': { rows: [{ dataCells: [{ value: { amount: 1200, currency: null }, label: '$1,200' }] }], aggregates: [{ value: 1, label: '1' }] } },
        groupingsDown: { groupings: [] }, groupingsAcross: { groupings: [] }, allData: true,
    };
    reset();
    on('GET', `/analytics/reports/${report}?includeDetails=true`, reply(200, result));
    const flat = sf.runReport(report);
    assert.deepEqual(flat.rows, [{ AMOUNT: 1200 }]);
    assert.deepEqual(flat.totals, [{ groups: {}, values: { RowCount: 1 } }]);

    reset();
    on('GET', `/analytics/reports/${report}/describe`, reply(200, { reportMetadata: { reportFormat: 'TABULAR', reportFilters: [{ column: 'STAGE_NAME', operator: 'equals', value: 'Won' }], reportBooleanFilter: null } }));
    on('POST', `/analytics/reports/${report}?includeDetails=false`, reply(200, result));
    sf.runReport(report, { includeDetails: false, filters: [{ column: 'AMOUNT', operator: 'greaterThan', value: '1000' }] });
    assert.deepEqual(requests.at(-1).body.reportMetadata.reportFilters, [
        { column: 'STAGE_NAME', operator: 'equals', value: 'Won' },
        { column: 'AMOUNT', operator: 'greaterThan', value: '1000' },
    ]);
    assert.deepEqual(lastCheck(), { capability: 'reports.run', context: { reportId: report } });
});

test('exportQuery creates a Bulk job, polls it, and writes every chunk with one header', () => {
    reset();
    on('POST', '/jobs/query', reply(200, { id: JOB, state: 'UploadComplete' }));
    let polls = 0;
    on('GET', `/jobs/query/${JOB}`, () => reply(200, { id: JOB, state: (polls += 1) < 2 ? 'InProgress' : 'JobComplete' }));
    on('GET', `/jobs/query/${JOB}/results?maxRecords=10000`, reply(200, 'Id,Name\n1,A\n2,B\n', { 'sforce-locator': 'MTAwMDA' }));
    on('GET', `/jobs/query/${JOB}/results?maxRecords=10000&locator=MTAwMDA`, reply(200, 'Id,Name\n3,C\n', { 'sforce-locator': 'null' }));
    const done = sf.exportQuery({ object: 'Account', fields: ['Name'], where: [{ field: 'Industry', op: '=', value: 'Tech' }] }, '/out/accounts.csv');
    assert.deepEqual(done, { jobId: JOB, state: 'JobComplete', done: true, path: '/out/accounts.csv', records: 3 });
    assert.equal(files.get('/out/accounts.csv'), 'Id,Name\n1,A\n2,B\n3,C\n');
    assert.deepEqual(requests[0].body, { operation: 'query', query: "SELECT Id, Name FROM Account WHERE Industry = 'Tech'", contentType: 'CSV', lineEnding: 'LF' });
    assert.deepEqual(lastCheck().context, { object: 'account', fields: ['industry', 'name'], path: '/out/accounts.csv' });

    reset();
    on('GET', `/jobs/query/${JOB}`, reply(200, { id: JOB, state: 'InProgress' }));
    const pending = sf.exportQuery({ object: 'Account', fields: ['Name'] }, '/out/accounts.csv', { jobId: JOB });
    assert.equal(pending.done, false);
    assert.deepEqual(writes(), [], 'continuing a job creates none');
    assert.throws(() => sf.exportQuery({ object: 'Account', fields: ['Name'], orderBy: [{ field: 'Name' }] }, '/x.csv'), /orderBy/);
});
