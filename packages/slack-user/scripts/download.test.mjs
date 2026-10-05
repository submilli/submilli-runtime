import assert from 'node:assert/strict';
import { test } from 'node:test';
import { contractHost, loadPackage } from '../../../scripts/package-contract-host.mjs';
const host = contractHost();
const transfers = [];
host.download = (url, path, options) => { transfers.push({ url, path, options }); return { status: 200 }; };
const slack = await loadPackage(new URL('../src/lib.ts', import.meta.url), host);

test('Slack checks before metadata and transfers with the approved byte limit', () => {
    host.reset(); host.response(() => ({ data: { ok: true, file: { id: 'file', url_private_download: 'https://files.slack.com/file' } } }));
    slack.downloadFile('file', '/downloads/file');
    assert.deepEqual(host.checks.map((c) => c.capability), ['slack.com/user/downloadFile', 'fs.write']);
    assert.deepEqual(host.checks[1].context, { path: '/downloads/file', max_bytes: 20000000 });
    assert.equal(transfers[0].options.maxBytes, 20000000);
    assert.equal(transfers[0].url, 'https://files.slack.com/file');
    host.reset(); transfers.length = 0; host.denial((cap) => cap === 'fs.write');
    assert.throws(() => slack.downloadFile('file', '/downloads/file'), /Capability denied/);
    assert.equal(host.requests.length, 0); assert.equal(transfers.length, 0);
});

test('Slack still refuses private download URLs outside files.slack.com', () => {
    host.reset(); transfers.length = 0;
    host.response(() => ({ data: { ok: true, file: { id: 'file', url_private_download: 'https://evil.test/file' } } }));
    assert.throws(() => slack.downloadFile('file', '/downloads/file'), (e) => e instanceof slack.SlackError && e.code === 'unsafe_file_url');
    assert.equal(transfers.length, 0);
});
