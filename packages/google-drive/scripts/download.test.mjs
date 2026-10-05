import assert from 'node:assert/strict';
import { test } from 'node:test';
import { contractHost, loadPackage, nullableFields } from '../../../scripts/package-contract-host.mjs';
const host = contractHost();
const transfers = [];
host.download = (url, path, options) => { transfers.push({ url, path, options }); return { status: 200 }; };
const drive = await loadPackage(new URL('../src/lib.ts', import.meta.url), host);

test('Drive checks the caller before metadata, then transfers with the approved limit', () => {
    for (const maxBytes of [null, 0, 100, 20000001]) {
        host.reset(); transfers.length = 0;
        host.response(() => ({ data: { id: 'file', mimeType: 'text/plain' } }));
        const options = maxBytes === null ? null : nullableFields({ maxBytes, overwrite: true });
        drive.downloadFile('file', '/downloads/file', options);
        assert.deepEqual(host.checks.map((c) => c.capability), ['submilli/google-drive.downloadFile', 'fs.write']);
        assert.deepEqual(host.checks[1].context, { path: '/downloads/file', max_bytes: maxBytes ?? 20000000 });
        assert.equal(transfers[0].options.maxBytes, maxBytes ?? 20000000);
        if (options !== null) assert.equal(transfers[0].options.overwrite, true);
    }
    host.reset(); transfers.length = 0; host.denial((cap) => cap === 'fs.write');
    assert.throws(() => drive.downloadFile('file', '/downloads/file'), /Capability denied/);
    assert.equal(host.requests.length, 0); assert.equal(transfers.length, 0);
});

test('Drive refuses invalid limits and snapshots getter-backed maxBytes once', () => {
    for (const maxBytes of [-1, 0.5, NaN, Infinity, 9007199254740992]) {
        host.reset(); transfers.length = 0;
        assert.throws(() => drive.downloadFile('file', '/downloads/file', nullableFields({ maxBytes })), RangeError);
        assert.equal(host.requests.length, 0); assert.equal(transfers.length, 0);
    }
    host.reset(); transfers.length = 0;
    let reads = 0;
    const options = nullableFields({ get maxBytes() { return ++reads === 1 ? 100 : 999; } });
    drive.downloadFile('file', '/downloads/file', options);
    assert.equal(reads, 1); assert.equal(host.checks[1].context.max_bytes, 100);
    assert.equal(transfers[0].options.maxBytes, 100);
});
