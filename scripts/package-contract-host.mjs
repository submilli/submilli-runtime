// Host adapter for offline tests of package request contracts.
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';

// `modules` lists the package's other `src/` files. A generated module cannot resolve relative
// imports, so they are merged in after the root, which uses them only from function bodies.
export async function loadPackage(sourceUrl, host, modules = []) {
    const bindings = 'get, post, put, patch, delete: remove, download, exists, read, readBytes, stat, write, encodeComponent, decodeComponent, encodeQuery, parse, secrets, check';
    const files = await Promise.all([sourceUrl, ...modules].map((url) => readFile(url, 'utf8')));
    const source = files.join('\n')
        .replace(/import\s+[\s\S]*?from "submilli:[^"]+";\n/g, '')
        .replace(/^(import|export) [^;]*? from "\.\/[^"]+";\n/gm, '')
        .replace(/\bdelete\(/g, 'remove(');
    // Each module captures its own host so separate package tests cannot share state.
    const key = `__packageContractHost${loadPackage.count++}`;
    globalThis[key] = host;
    const prelude = `const { ${bindings} } = globalThis[${JSON.stringify(key)}];\n`;
    return import(`data:text/javascript;base64,${Buffer.from(prelude + stripTypeScriptTypes(source)).toString('base64')}`);
}
loadPackage.count = 0;

export function contractHost() {
    const requests = [];
    const checks = [];
    let respond = () => ({ data: {} });
    let deny = () => false;
    const request = (method) => (url, body, headers) => {
        if (method === 'get' || method === 'delete') { headers = body; body = null; }
        // An object body is sent as JSON, which drops `undefined` fields.
        const wire = body !== null && typeof body === 'object' && !(body instanceof Uint8Array)
            ? JSON.parse(JSON.stringify(body))
            : body;
        const captured = { method, url, body: wire, headers };
        requests.push(captured);
        const result = respond(captured);
        return {
            status: result.status ?? 200,
            ok: (result.status ?? 200) < 400,
            statusText: result.statusText ?? '',
            body: JSON.stringify(result.data),
            headers: { get: (name) => result.headers?.[name] },
            json: () => result.data,
            text: () => JSON.stringify(result.data),
        };
    };
    return {
        requests, checks,
        response: (handler) => { respond = handler; },
        denial: (predicate) => { deny = predicate; },
        reset: () => { requests.length = 0; checks.length = 0; deny = () => false; },
        get: request('get'), post: request('post'), put: request('put'), patch: request('patch'), delete: request('delete'),
        encodeComponent: encodeURIComponent, decodeComponent: decodeURIComponent,
        encodeQuery: (query) => new URLSearchParams(query).toString(),
        secrets: { get: () => 'test-token' },
        check: (capability, context) => {
            checks.push({ capability, context });
            if (deny(capability, context)) throw new Error('Capability denied');
        },
        exists: () => true,
        stat: () => ({ kind: 'file', size: 0 }),
        parse: (url) => {
            const value = new URL(url);
            return { protocol: value.protocol.slice(0, -1), host: value.hostname, port: value.port === '' ? undefined : Number(value.port), path: value.pathname, query: value.search.slice(1), fragment: value.hash.slice(1), username: value.username, password: value.password };
        },
    };
}
