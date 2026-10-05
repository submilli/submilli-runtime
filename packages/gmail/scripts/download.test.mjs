import assert from 'node:assert/strict';
import { test } from 'node:test';
import { contractHost, loadPackage } from '../../../scripts/package-contract-host.mjs';
const host = contractHost();
const writes = [];
host.write = (path, bytes) => { writes.push({ path, bytes }); };
const gmail = await loadPackage(new URL('../src/lib.ts', import.meta.url), host);
const limit = 20000000;

test('Gmail checks before fetching and writes the decoded attachment', () => {
    host.reset(); writes.length = 0; host.response(() => ({ data: { data: 'AP_-', size: 0 } }));
    gmail.downloadAttachment('message', 'attachment', '/downloads/file');
    assert.deepEqual(host.checks.map((c) => c.capability), ['submilli/gmail.downloadAttachment', 'fs.write']);
    assert.deepEqual(host.checks[1].context, { path: '/downloads/file', max_bytes: limit });
    assert.deepEqual(Array.from(writes[0].bytes), [0, 255, 254]);
    host.reset(); writes.length = 0; host.denial((cap) => cap === 'fs.write');
    assert.throws(() => gmail.downloadAttachment('message', 'attachment', '/downloads/file'), /Capability denied/);
    assert.equal(host.requests.length, 0); assert.equal(writes.length, 0);
});

test('Gmail bounds actual bytes even when the server claims a smaller size', () => {
    for (const size of [0, limit, limit + 1, limit + 3]) {
        host.reset(); writes.length = 0;
        const data = Buffer.alloc(size).toString('base64url');
        host.response(() => ({ data: { data, size: 0 } }));
        const action = () => gmail.downloadAttachment('message', 'attachment', '/downloads/file');
        if (size <= limit) {
            action(); assert.equal(writes[0].bytes.length, size);
        } else {
            assert.throws(action, (e) => e instanceof gmail.GmailError && e.code === 'attachment_too_large');
            assert.equal(writes.length, 0);
        }
    }
    host.reset(); writes.length = 0; host.response(() => ({ data: { data: '!', size: 0 } }));
    assert.throws(() => gmail.downloadAttachment('message', 'attachment', '/downloads/file'), SyntaxError);
    assert.equal(writes.length, 0);
});
