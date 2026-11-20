import assert from 'node:assert/strict';
import { test } from 'node:test';
import { contractHost, loadPackage } from '../../../scripts/package-contract-host.mjs';

const host = contractHost();
const drive = await loadPackage(new URL('../src/lib.ts', import.meta.url), host);

test('about requests only user(emailAddress) and returns the email', () => {
    host.reset();
    host.response(() => ({ data: { user: { emailAddress: 'agents@submilli.ai' } } }));
    assert.deepEqual(drive.about(), { emailAddress: 'agents@submilli.ai' });
    assert.equal(host.checks.length, 1);
    assert.equal(host.checks[0].capability, 'submilli/google-drive.about');
    assert.deepEqual(host.checks[0].context, {});
    assert.equal(host.requests.length, 1);
    assert.equal(host.requests[0].method, 'get');
    const url = new URL(host.requests[0].url);
    assert.equal(url.origin + url.pathname, 'https://www.googleapis.com/drive/v3/about');
    assert.equal(url.searchParams.get('fields'), 'user(emailAddress)');
    assert.equal([...url.searchParams.keys()].join(','), 'fields');
});

test('about fails when the email is missing or not a bare address', () => {
    for (const data of [{}, { user: {} }, { user: { emailAddress: '' } }, { user: { emailAddress: 'not-an-email' } }, { user: null }]) {
        host.reset();
        host.response(() => ({ data }));
        assert.throws(() => drive.about(), (error) => error instanceof drive.DriveError && error.code === 'missing_email');
        assert.equal(host.requests.length, 1);
    }
});

test('about fails closed on an HTTP error', () => {
    host.reset();
    host.response(() => ({ data: { error: { message: 'forbidden', status: 'PERMISSION_DENIED' } }, status: 403, statusText: 'Forbidden' }));
    assert.throws(() => drive.about(), (error) => error instanceof drive.DriveError && error.status === 403);
    assert.equal(host.requests.length, 1);
});

test('about is denied before any request', () => {
    host.reset();
    host.denial(() => true);
    host.response(() => { throw new Error('must not request Drive'); });
    assert.throws(() => drive.about(), /Capability denied/);
    assert.equal(host.requests.length, 0);
    assert.equal(host.checks[0].capability, 'submilli/google-drive.about');
});
