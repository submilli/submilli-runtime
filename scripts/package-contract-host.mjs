// Host adapter for offline tests of package request contracts. Missing wire and
// option fields read as null in Submilli, unlike JavaScript's undefined.
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';

export function nullableFields(value) {
    if (value === null || typeof value !== 'object') return value;
    if (!Array.isArray(value) && Object.getPrototypeOf(value) !== Object.prototype) return value;
    return new Proxy(value, {
        get(target, property, receiver) {
            const field = Reflect.get(target, property, receiver);
            return field === undefined ? null : nullableFields(field);
        },
    });
}

export async function loadPackage(sourceUrl, host) {
    const bindings = 'get, post, put, patch, delete: remove, download, read, readBytes, stat, write, encodeComponent, decodeComponent, encodeQuery, parse, secrets, check';
    const source = (await readFile(sourceUrl, 'utf8'))
        .replace(/import\s+[\s\S]*?from "submilli:[^"]+";\n/g, '')
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
        const captured = { method, url, body, headers };
        requests.push(captured);
        const result = respond(captured);
        return {
            status: result.status ?? 200,
            ok: (result.status ?? 200) < 400,
            statusText: result.statusText ?? '',
            body: JSON.stringify(result.data),
            headers: { get: (name) => result.headers?.[name] ?? null },
            json: () => nullableFields(result.data),
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
        stat: () => ({ kind: 'file', size: 0 }),
        parse: (url) => {
            const value = new URL(url);
            return nullableFields({ protocol: value.protocol.slice(0, -1), host: value.hostname, port: value.port === '' ? null : Number(value.port), path: value.pathname, query: value.search.slice(1), fragment: value.hash.slice(1), username: value.username, password: value.password });
        },
    };
}
