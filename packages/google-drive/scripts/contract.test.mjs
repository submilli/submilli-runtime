import assert from 'node:assert/strict';
import { test } from 'node:test';
import { contractHost, loadPackage } from '../../../scripts/package-contract-host.mjs';
const host = contractHost();
const drive = await loadPackage(new URL('../src/lib.ts', import.meta.url), host);
const options = (value = {}) => ({ name: 'report.txt', mimeType: 'text/plain', ...value });
const folder = { id: 'parent', mimeType: 'application/vnd.google-apps.folder', driveId: 'actual-drive' };

test('upload authorization uses the resolved destination even without a drive hint', () => {
    host.reset(); host.response(() => ({ data: folder }));
    host.denial((cap, ctx) => ctx.driveId === 'actual-drive');
    assert.throws(() => drive.uploadFile('/report.txt', options({ parentId: 'parent' })), /Capability denied/);
    assert.deepEqual(host.checks[0].context, { path: '/report.txt', parentId: 'parent', driveId: 'actual-drive' });
    assert.equal(host.requests.length, 1); assert.equal(host.requests[0].method, 'get');
    assert.equal(new URL(host.requests[0].url).pathname, '/drive/v3/files/parent');
    assert.equal(new URL(host.requests[0].url).searchParams.get('fields'), 'mimeType,driveId');
});

test('caller cannot spoof the destination drive; invalid parent metadata fails closed', () => {
    for (const [data, status, opts, code] of [
        [folder, 200, { parentId: 'parent', driveId: 'allowed-drive' }, 'invalid_drive'],
        [{ ...folder, mimeType: 'text/plain' }, 200, { parentId: 'parent' }, 'invalid_parent'],
        [{}, 404, { parentId: 'missing' }, 'invalid_parent'],
        [{ ...folder, driveId: null }, 200, { driveId: 'shared-drive' }, 'invalid_drive'],
    ]) {
        host.reset(); host.response(() => ({ data, status }));
        assert.throws(() => drive.uploadFile('/report.txt', options(opts)), (e) => e instanceof drive.DriveError && e.code === code);
        assert.equal(host.requests.length, 1); assert.equal(host.checks.length, 0);
    }
});

test('My Drive root resolves to null; matching shared drive proceeds to the same parent', () => {
    host.reset(); host.response(() => ({ data: { ...folder, driveId: null } })); host.denial(() => true);
    assert.throws(() => drive.uploadFile('/report.txt', options()), /Capability denied/);
    assert.deepEqual(host.checks[0].context, { path: '/report.txt', parentId: '', driveId: null });
    assert.equal(new URL(host.requests[0].url).pathname, '/drive/v3/files/root');
    host.reset(); host.response(({ method }) => ({ data: method === 'get' ? folder : { id: 'uploaded' }, headers: method === 'post' ? { location: 'https://www.googleapis.com/upload/drive/v3/files?upload_id=test' } : {} }));
    assert.equal(drive.uploadFile('/report.txt', options({ parentId: 'parent', driveId: 'actual-drive' })).id, 'uploaded');
    assert.deepEqual(host.requests.map((r) => r.method), ['get', 'post', 'put']);
    assert.deepEqual(host.requests[1].body.parents, ['parent']);
    assert.equal(new URL(host.requests[1].url).searchParams.get('supportsAllDrives'), 'true');
});
