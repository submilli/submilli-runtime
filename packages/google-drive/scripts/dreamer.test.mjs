import assert from 'node:assert/strict';
import { test } from 'node:test';
import { contractHost, loadPackage } from '../../../scripts/package-contract-host.mjs';

const agent = 'agents@submilli.ai';
const about = '/drive/v3/about';
const files = '/drive/v3/files';
const fileDetail = { permissionType: 'file', inherited: false, role: 'reader' };
const direct = { type: 'user', emailAddress: agent, permissionDetails: [fileDetail] };
const domain = { type: 'domain', domain: 'submilli.ai', allowFileDiscovery: true, permissionDetails: [fileDetail] };
const metadata = (id, mimeType = 'text/plain') => ({ id, name: id, mimeType });

async function setup() {
    const host = contractHost();
    const drive = await loadPackage(new URL('../src/lib.ts', import.meta.url), host);
    return { host, drive };
}

test('each Dreamer caller fails before any file request if identity verification fails', async () => {
    const { host, drive } = await setup();
    const callers = [
        () => drive.searchDreamerFiles('domain'),
        () => drive.searchDreamerFiles('sharedWithMe'),
        () => drive.getDreamerFile('known'),
        () => drive.readDreamerText('known'),
    ];
    for (const call of callers) {
        for (const reply of [
            { data: { user: { emailAddress: 'personal@example.com' } } },
            { data: { user: {} } },
            { status: 403, data: {} },
            { status: 401, body: '{"error":' },
            { body: '{' },
        ]) {
            host.reset();
            host.response(() => reply);
            assert.throws(call);
            assert.deepEqual(host.requests.map(r => new URL(r.url).pathname), [about]);
        }
        host.reset();
        host.denial(() => true);
        assert.throws(call, /Capability denied/);
        assert.equal(host.requests.length, 0);
    }
});

test('one credential is verified and pinned through a read, and rotation triggers a new check', async () => {
    const { host, drive } = await setup();
    let token = 'first';
    const bindings = [];
    host.secrets.get = name => { bindings.push(name); return token; };
    host.response(({ url, headers }) => {
        const path = new URL(url).pathname;
        if (path === about) {
            const emailAddress = headers.get('Authorization') === 'Bearer first' ? agent : 'personal@example.com';
            token = 'rotated';
            return { data: { user: { emailAddress } } };
        }
        if (path.endsWith('/permissions')) return { data: { permissions: [direct] } };
        if (new URL(url).searchParams.get('alt') === 'media') {
            assert.equal(new URL(url).searchParams.get('supportsAllDrives'), 'true');
            return { body: 'file text' };
        }
        return { data: { ...metadata('doc'), driveId: 'shared-drive' } };
    });
    assert.equal(drive.readDreamerText('doc'), 'file text');
    assert.deepEqual(host.requests.map(r => r.headers.get('Authorization')), Array(4).fill('Bearer first'));
    assert.throws(() => drive.readDreamerText('doc'), e => e.code === 'wrong_account');
    assert.equal(new URL(host.requests.at(-1).url).pathname, about);
    assert.equal(host.requests.length, 5);
    assert.deepEqual(bindings, ['DREAMER_GOOGLE_ACCESS_TOKEN', 'DREAMER_GOOGLE_ACCESS_TOKEN']);
    token = undefined;
    assert.throws(() => drive.searchDreamerFiles('domain'), e => e.code === 'missing_token');
    assert.equal(host.requests.length, 5);
});

const ineligible = {
    private: [{ ...direct, permissionDetails: [{ ...fileDetail, role: 'owner' }] }],
    member: [{ ...direct, permissionDetails: [{ permissionType: 'member', role: 'reader', inherited: true }] }],
    folderInherited: [{ ...direct, permissionDetails: [{ ...fileDetail, inherited: true }] }],
    link: [{ type: 'anyone', permissionDetails: [fileDetail] }],
    domainLink: [{ ...domain, allowFileDiscovery: false }],
    group: [{ ...direct, type: 'group' }],
    otherDomain: [{ ...domain, domain: 'elsewhere.example' }],
    otherUser: [{ ...direct, emailAddress: 'someone@submilli.ai' }],
    deleted: [{ ...direct, deleted: true }],
    limited: [{ ...direct, view: 'metadata' }],
    unknownSource: [{ type: 'user', emailAddress: agent }],
};
const candidates = {
    ...ineligible,
    direct: [direct],
    domain: [{ ...domain, permissionDetails: [{ ...fileDetail, inherited: true }] }],
};

test('only the two discovery modes run and both filter candidate metadata by file grants', async () => {
    const { host, drive } = await setup();
    for (const mode of [undefined, 'user', 'drive', 'allDrives']) {
        assert.throws(() => drive.searchDreamerFiles(mode), e => e.code === 'invalid_corpora');
    }
    assert.equal(host.requests.length, 0);
    for (const mode of ['domain', 'sharedWithMe']) {
        host.reset();
        host.response(({ url, method }) => {
            const { origin, pathname, searchParams } = new URL(url);
            assert.equal(origin, 'https://www.googleapis.com');
            assert.equal(method, 'get');
            if (pathname === about) return { data: { user: { emailAddress: agent } } };
            if (pathname === files) {
                assert.equal(searchParams.get('corpora'), mode === 'domain' ? 'domain' : 'user');
                assert.equal(searchParams.get('q'), mode === 'domain' ? 'trashed = false' : 'trashed = false and sharedWithMe = true');
                assert.equal(searchParams.get('driveId'), null);
                if (searchParams.has('pageToken')) {
                    assert.equal(searchParams.get('pageToken'), 'next/+');
                    return { data: { files: Object.keys(candidates).map(id => metadata(id)) } };
                }
                return { data: { files: [metadata('private')], nextPageToken: 'next/+' } };
            }
            const [, id] = pathname.match(/^\/drive\/v3\/files\/(\w+)\/permissions$/);
            assert.equal(searchParams.get('supportsAllDrives'), 'true');
            assert.match(searchParams.get('fields'), /permissionDetails\(permissionType,inherited,role\)/);
            return { data: { permissions: candidates[id] } };
        });
        const first = drive.searchDreamerFiles(mode);
        assert.deepEqual(first, { items: [], nextPageToken: 'next/+' });
        const page = drive.searchDreamerFiles(mode, { pageToken: first.nextPageToken, rawQuery: 'trashed = true', driveId: 'team' });
        assert.deepEqual(page.items.map(f => f.id), ['direct', 'domain']);
        assert.equal(page.nextPageToken, '');
    }
});

test('known IDs with no qualifying grant yield neither metadata nor content', async () => {
    const { host, drive } = await setup();
    for (const reply of [
        ...Object.values(ineligible).map(permissions => ({ data: { permissions } })),
        { data: {} }, { status: 403, data: { error: { errors: [{ reason: 'insufficientFilePermissions' }] } } },
        { status: 404, data: {} },
    ]) {
        host.reset();
        host.response(({ url }) => {
            const path = new URL(url).pathname;
            if (path === about) return { data: { user: { emailAddress: agent } } };
            assert.equal(path, files + '/known/permissions');
            return reply;
        });
        assert.equal(drive.getDreamerFile('known'), null);
        assert.throws(() => drive.readDreamerText('known'), e => e instanceof drive.DriveError && e.status === 404);
        assert.equal(host.requests.length, 4);
    }
});

test('a direct grant on a later permission page permits Doc export, revocation denies a later read', async () => {
    const { host, drive } = await setup();
    let granted = true;
    let exports = 0;
    host.response(({ url }) => {
        const { pathname, searchParams } = new URL(url);
        if (pathname === about) return { data: { user: { emailAddress: agent } } };
        if (pathname.endsWith('/permissions')) {
            if (!searchParams.has('pageToken')) return { data: { nextPageToken: 'acl/+' } };
            assert.equal(searchParams.get('pageToken'), 'acl/+');
            return { data: { permissions: granted ? [direct] : [] } };
        }
        if (pathname === files + '/doc') return { data: metadata('doc', 'application/vnd.google-apps.document') };
        assert.equal(pathname, files + '/doc/export');
        assert.equal(searchParams.get('mimeType'), 'text/plain');
        assert.equal(searchParams.has('supportsAllDrives'), false);
        exports++;
        return { body: 'Doc text' };
    });
    assert.equal(drive.getDreamerFile('doc').id, 'doc');
    assert.equal(drive.readDreamerText('doc'), 'Doc text');
    granted = false;
    assert.throws(() => drive.readDreamerText('doc'), e => e.code === 'not_found');
    assert.equal(exports, 1);
});

test('quota and malformed permission errors preserve their HTTP status and stop before content', async () => {
    const { host, drive } = await setup();
    for (const [reply, code] of [
        [{ status: 403, data: { error: { errors: [{ reason: 'userRateLimitExceeded' }] } } }, 'userRateLimitExceeded'],
        [{ status: 403, body: '{"error":' }, 'http_error'],
    ]) {
        host.reset();
        host.response(({ url }) => {
            const path = new URL(url).pathname;
            if (path === about) return { data: { user: { emailAddress: agent } } };
            if (path === files) return { data: { files: [metadata('doc')] } };
            assert.equal(path, files + '/doc/permissions');
            return reply;
        });
        const expected = e => e instanceof drive.DriveError && e.status === 403 && e.code === code;
        assert.throws(() => drive.searchDreamerFiles('domain'), expected);
        assert.throws(() => drive.readDreamerText('doc'), expected);
    }
});
