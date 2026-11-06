import assert from 'node:assert/strict';
import { test } from 'node:test';
import { contractHost, loadPackage } from '../../../scripts/package-contract-host.mjs';
const host = contractHost();
const gmail = await loadPackage(new URL('../src/lib.ts', import.meta.url), host);
function draft(headers) { host.response(() => ({ data: { message: { payload: { headers } } } })); }

test('attachment reads translate size failures and preserve other errors', async () => {
    const originalTemporal = globalThis.Temporal;
    globalThis.Temporal = { Now: { instant: () => ({ epochMilliseconds: 0 }) } };
    try {
        for (const failure of [new RangeError('file exceeds fs.maxReadSize()'), new Error('Capability denied')]) {
            const readHost = contractHost();
            readHost.read = () => { throw failure; };
            const reader = await loadPackage(new URL('../src/lib.ts', import.meta.url), readHost);
            assert.throws(() => reader.sendEmail({
                to: ['dana@example.com'], subject: 'report', text: '', attachments: [{ path: '/report.txt' }],
            }), (error) => failure instanceof RangeError
                ? error instanceof reader.GmailError && error.code === 'attachment_unreadable'
                : error === failure);
            assert.equal(readHost.requests.length, 0);
        }
    } finally {
        if (originalTemporal === undefined) delete globalThis.Temporal;
        else globalThis.Temporal = originalTemporal;
    }
});

test('stored From and every recipient are checked before sending a draft', () => {
    host.reset(); draft([{ name: 'From', value: 'Alias <BLOCKED@Example.Com.>' }, { name: 'To', value: 'allowed@example.com' }, { name: 'Bcc', value: 'hidden@example.com' }]);
    host.denial((cap, context) => cap.endsWith('sendDraft') && context.from === 'blocked@example.com');
    assert.throws(() => gmail.sendDraft('draft-id'), /Capability denied/);
    assert.deepEqual(host.checks.at(-1), { capability: 'submilli/gmail.sendDraft', context: { recipients: ['allowed@example.com', 'hidden@example.com'], from: 'blocked@example.com' } });
    assert.equal(host.requests.length, 1); assert.equal(host.requests[0].method, 'get');
    assert.equal(new URL(host.requests[0].url).searchParams.get('format'), 'metadata');
});

test('missing From is explicit null; ambiguous or injected headers cannot send', () => {
    host.reset(); draft([{ name: 'To', value: 'allowed@example.com' }]);
    host.denial(() => true);
    assert.throws(() => gmail.sendDraft('id'), /Capability denied/);
    assert.equal(host.checks[0].context.from, null);
    for (const headers of [
        [{ name: 'From', value: 'a@example.com, b@example.com' }],
        [{ name: 'From', value: 'a@example.com' }, { name: 'fRoM', value: 'b@example.com' }],
        [{ name: 'From', value: 'a@example.com\r\nBcc: b@example.com' }],
        [{ name: 'From', value: '' }],
    ]) {
        host.reset(); draft(headers);
        assert.throws(() => gmail.sendDraft('id'), gmail.GmailError);
        assert.equal(host.requests.length, 1); assert.equal(host.checks.length, 0);
    }
    host.reset(); host.response(() => ({ status: 404, data: {} }));
    assert.throws(() => gmail.sendDraft('missing'), (e) => e.code === 'not_found');
    assert.equal(host.checks.length, 0);
});

test('allowed alias sends the same stored draft ID after its check', () => {
    host.reset(); host.response(({ method }) => ({ data: method === 'get' ? { message: { payload: { headers: [{ name: 'From', value: 'Alias <alias@example.com>' }] } } } : { id: 'sent' } }));
    assert.equal(gmail.sendDraft('stored').id, 'sent');
    assert.equal(host.checks[0].context.from, 'alias@example.com');
    assert.deepEqual(host.requests[1].body, { id: 'stored' });
});
