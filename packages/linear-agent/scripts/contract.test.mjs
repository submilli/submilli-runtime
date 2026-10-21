// Exercise both real packages. Only Submilli host functions are replaced.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import { test } from 'node:test';

let token = 'oauth-test-token';
let response;
let failure;
let failureOnOperation = false;
let denied;
let team = 'team';
let commentSession = false;
const requests = [];
const checks = [];
const queries = new Set();
const session = { id: 'session', status: 'active', url: null, summary: null,
    issue: { id: 'issue' }, comment: null, externalUrls: [], plan: null };
const host = {
    secrets: { get: () => token },
    check(capability, context) {
        checks.push({ capability, context });
        if (capability === denied) throw new Error('Capability denied: ' + capability);
    },
    parse(value) {
        const url = new URL(value);
        return { protocol: url.protocol.slice(0, -1), host: url.hostname };
    },
    post(url, body, headers) {
        requests.push({ url, body: JSON.parse(JSON.stringify(body)), headers });
        queries.add(body.query);
        const activeFailure = !failureOnOperation || !body.query.includes('team { id }') ? failure : undefined;
        if (activeFailure === 'transport') throw new Error('transport failed');
        if (activeFailure === 'http') return { ok: false, status: 403, statusText: 'Forbidden', body: '{"errors":[{"message":"not the owning app"}]}' };
        if (activeFailure === 'graphql') return { ok: true, json: () => ({ errors: [{ message: 'not the owning app' }] }) };
        if (activeFailure === 'missing-data') return { ok: true, json: () => ({ data: null }) };
        if (body.query.includes('team { id }')) {
            const issue = { team: { id: team } };
            const agentSession = team === null ? { issue: null, comment: null }
                : commentSession ? { issue: null, comment: { issue } } : { issue, comment: null };
            return { ok: true, json: () => ({ data: { agentSession } }) };
        }
        return { ok: true, json: () => ({ data: response }) };
    },
};
globalThis.__linearAgentHost = host;
const moduleUrl = source => `data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(source)).toString('base64')}`;
const linearSource = (await readFile(new URL('../../linear/src/lib.ts', import.meta.url), 'utf8'))
    .replace(/^import .*from "submilli:.*";\n/gm, '')
    .replace('const ENDPOINT', 'const { post, secrets, check } = globalThis.__linearAgentHost;\nconst ENDPOINT');
const linearUrl = moduleUrl(linearSource);
const source = (await readFile(new URL('../src/lib.ts', import.meta.url), 'utf8'))
    .replace(/^import .*from "submilli:.*";\n/gm, '')
    .replace(/import \{[\s\S]*?\} from "@submilli\/linear";/, `import { createAgentActivity, getAgentSession, listAgentActivities, updateAgentSession } from "${linearUrl}";`);
const agent = await import(moduleUrl('const { check, parse } = globalThis.__linearAgentHost;\n' + source));

function reset() {
    requests.length = checks.length = 0;
    failure = denied = undefined;
    failureOnOperation = false;
    team = 'team';
    commentSession = false;
}
function activity(content) {
    return { id: 'activity', createdAt: '2026-01-01T00:00:00Z', ephemeral: false,
        content, signal: null, signalMetadata: null };
}
const emissions = [
    ['thought', () => agent.thought('session', 'Working'), { type: 'thought', body: 'Working' }],
    ['action', () => agent.action('session', 'Searching', 'Related issues'), { type: 'action', action: 'Searching', parameter: 'Related issues' }],
    ['ask', () => agent.ask('session', 'Which repo?'), { type: 'elicitation', body: 'Which repo?' }],
    ['respond', () => agent.respond('session', 'Done'), { type: 'response', body: 'Done' }],
    ['fail', () => agent.fail('session', 'Failed'), { type: 'error', body: 'Failed' }],
    ['askChoice', () => agent.askChoice('session', 'Choose', [{ label: 'Frontend', value: 'frontend' }]), { type: 'elicitation', body: 'Choose' }],
    ['askAccountConnection', () => agent.askAccountConnection('session', 'Connect', 'https://auth.example.com'), { type: 'elicitation', body: 'Connect' }],
];
const readsAndLinks = [
    ['getSession', () => agent.getSession('session'), 'getAgentSession'],
    ['listActivities', () => agent.listActivities('session'), 'listAgentActivities'],
    ['setExternalLinks', () => agent.setExternalLinks('session', []), 'updateAgentSession'],
    ['addExternalLinks', () => agent.addExternalLinks('session', []), 'updateAgentSession'],
    ['removeExternalLinks', () => agent.removeExternalLinks('session', []), 'updateAgentSession'],
];

test('every activity helper maps to the correct content and returns its identity', () => {
    for (const [name, call, content] of emissions) {
        reset();
        const result = activity(content);
        response = { agentActivityCreate: { success: true, agentActivity: result } };
        assert.deepEqual(call(), result);
        assert.equal(checks[0].capability, `linear-agent.${name}`);
        assert.deepEqual(checks[0].context, { sessionId: 'session' });
        assert.equal(checks[1].capability, 'linear.app/createAgentActivity');
        assert.deepEqual(checks[1].context, { teamId: 'team' });
        assert.equal(requests.length, 2);
        const input = requests.at(-1).body.variables.input;
        assert.deepEqual(input.content, content);
        assert.equal(input.agentSessionId, 'session');
        assert.ok(!Object.hasOwn(input, 'id'));
        assert.ok(!Object.hasOwn(input, 'ephemeral'));
        if (name === 'askChoice') {
            assert.equal(input.signal, 'select');
            assert.deepEqual(input.signalMetadata, { options: [{ label: 'Frontend', value: 'frontend' }] });
        } else if (name === 'askAccountConnection') {
            assert.equal(input.signal, 'auth');
            assert.deepEqual(input.signalMetadata, { url: 'https://auth.example.com' });
        } else {
            assert.ok(!Object.hasOwn(input, 'signal'));
            assert.ok(!Object.hasOwn(input, 'signalMetadata'));
        }
    }
});

test('all helpers forward caller UUIDs and only their own optional fields', () => {
    reset();
    response = { agentActivityCreate: { success: true, agentActivity: activity({ type: 'thought', body: 'b' }) } };
    const id = '5bbdee82-423f-46cf-b68e-459ca5c5c7ef';
    for (const call of [
        () => agent.thought('session', 'b', { id, ephemeral: false }),
        () => agent.action('session', 'a', 'p', { id, ephemeral: true, result: '' }),
        () => agent.ask('session', 'b', { id }),
        () => agent.respond('session', 'b', { id }),
        () => agent.fail('session', 'b', { id }),
        () => agent.askChoice('session', 'b', [{ label: 'L', value: 'v' }], { id }),
        () => agent.askAccountConnection('session', 'b', 'https://example.com/auth', { id, providerName: 'GitHub', userId: 'user' }),
    ]) {
        call();
        assert.equal(requests.at(-1).body.variables.input.id, id);
    }
    agent.thought('session', 'b', { ephemeral: false });
    assert.equal(requests.at(-1).body.variables.input.ephemeral, false);
    agent.action('session', 'a', 'p', { ephemeral: true, result: '' });
    assert.equal(requests.at(-1).body.variables.input.ephemeral, true);
    assert.equal(requests.at(-1).body.variables.input.content.result, '');
    agent.action('session', 'a', 'p');
    assert.ok(!Object.hasOwn(requests.at(-1).body.variables.input.content, 'result'));
    agent.askAccountConnection('session', 'b', 'https://example.com/auth', { providerName: 'GitHub', userId: 'user' });
    assert.deepEqual(requests.at(-1).body.variables.input.signalMetadata, { url: 'https://example.com/auth', providerName: 'GitHub', userId: 'user' });
    // Extra properties arriving through JavaScript/structural typing cannot inject invalid fields.
    agent.respond('session', 'b', { ephemeral: true, signal: 'auth' });
    assert.deepEqual(requests.at(-1).body.variables.input, { agentSessionId: 'session', content: { type: 'response', body: 'b' } });
});

test('account links reject non-HTTPS and malformed URLs before dependency calls', () => {
    reset();
    for (const url of ['http://example.com', 'javascript:alert(1)', '', '/relative', 'https://', 'https:///']) {
        assert.throws(() => agent.askAccountConnection('session', 'Connect', url));
    }
    assert.equal(requests.length, 0);
});

test('session and paginated activity reads preserve prompts, signals, and cursors', () => {
    reset();
    response = { agentSession: session };
    assert.deepEqual(agent.getSession('session'), session);
    const prompt = { ...activity({ type: 'prompt', body: 'Stop' }), signal: 'stop' };
    const first = { nodes: [prompt], pageInfo: { hasNextPage: true, endCursor: 'cursor' } };
    response = { agentSession: { activities: first } };
    assert.deepEqual(agent.listActivities('session'), first);
    assert.deepEqual(requests.at(-1).body.variables, { id: 'session', first: 50 });
    const last = { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } };
    response = { agentSession: { activities: last } };
    assert.deepEqual(agent.listActivities('session', { first: 25, after: first.pageInfo.endCursor }), last);
    assert.deepEqual(requests.at(-1).body.variables, { id: 'session', first: 25, after: 'cursor' });
    agent.listActivities('session', { after: null });
    assert.deepEqual(requests.at(-1).body.variables, { id: 'session', first: 50 });
    agent.listActivities('session', { first: 250 });
    assert.equal(requests.at(-1).body.variables.first, 250);
});

test('link helpers issue disjoint replace/add/remove updates, including explicit clears', () => {
    reset();
    const links = [{ label: 'PR', url: 'https://example.com/pr' }];
    response = { agentSessionUpdate: { success: true, agentSession: session } };
    for (const [name, arg, field] of [
        ['setExternalLinks', links, 'externalUrls'],
        ['setExternalLinks', [], 'externalUrls'],
        ['addExternalLinks', links, 'addedExternalUrls'],
        ['removeExternalLinks', [links[0].url], 'removedExternalUrls'],
    ]) {
        assert.deepEqual(agent[name]('session', arg), session);
        assert.deepEqual(requests.at(-1).body.variables, { id: 'session', input: { [field]: arg } });
    }
});

test('every main capability denial stops before metadata lookup or mutation', () => {
    for (const [name, call] of [...emissions, ...readsAndLinks]) {
        reset();
        denied = `linear-agent.${name}`;
        assert.throws(call, /Capability denied/);
        assert.equal(requests.length, 0);
    }
});

test('every dependency denial stops after metadata lookup and before the operation', () => {
    for (const [, call, capability] of [...emissions.map(([name, call]) => [name, call, 'createAgentActivity']), ...readsAndLinks]) {
        reset();
        denied = `linear.app/${capability}`;
        assert.throws(call, /Capability denied/);
        assert.equal(requests.length, 1);
        assert.deepEqual(checks.at(-1).context, { teamId: 'team' });
    }
});

test('team lookup supports comment sessions and fails closed without a team', () => {
    reset();
    response = { agentActivityCreate: { success: true, agentActivity: activity({ type: 'thought', body: 'b' }) } };
    commentSession = true;
    agent.thought('session', 'b');
    assert.deepEqual(checks.at(-1).context, { teamId: 'team' });
    team = null;
    const before = requests.length;
    assert.throws(() => agent.thought('session', 'b'), /no issue or comment team/);
    assert.equal(requests.length, before + 1);
});

test('every mutation rejects success:false and missing payload entities', () => {
    for (const [, call] of emissions) {
        for (const payload of [{ success: false, agentActivity: activity({ type: 'thought', body: 'b' }) }, { success: true, agentActivity: null }]) {
            reset();
            response = { agentActivityCreate: payload };
            assert.throws(call, /did not succeed/);
            assert.equal(requests.length, 2, 'no hidden retries');
        }
    }
    for (const [name, call] of readsAndLinks.filter(([, , cap]) => cap === 'updateAgentSession')) {
        for (const payload of [{ success: false, agentSession: session }, { success: true, agentSession: null }]) {
            reset();
            response = { agentSessionUpdate: payload };
            assert.throws(call, /did not succeed/, name);
        }
    }
});

test('all helpers propagate HTTP, GraphQL, transport, and missing-data failures', () => {
    for (const [, call] of [...emissions, ...readsAndLinks]) {
        for (const mode of ['http', 'graphql', 'transport', 'missing-data']) {
            for (const operationStage of [false, true]) {
                reset();
                failure = mode;
                failureOnOperation = operationStage;
                assert.throws(call, /not the owning app|transport failed|no data/);
                assert.equal(requests.length, operationStage ? 2 : 1, 'no retries at either failure stage');
            }
        }
    }
});

test('credentials continue to use the dependency Authorization handling', () => {
    reset();
    response = { agentSession: session };
    for (const [value, header] of [['oauth-token', 'Bearer oauth-token'], ['Bearer oauth-token', 'Bearer oauth-token'], ['lin_api_personal', 'lin_api_personal']]) {
        token = value;
        agent.getSession('session');
        assert.equal(requests.at(-1).headers.get('Authorization'), header);
    }
});

test('captured queries validate against the official SDL when supplied', async t => {
    if (!process.env.LINEAR_SCHEMA_PATH || !process.env.GRAPHQL_MODULE_PATH) {
        t.skip('Set LINEAR_SCHEMA_PATH and GRAPHQL_MODULE_PATH for SDL validation');
        return;
    }
    const { buildSchema, parse, validate } = await import(process.env.GRAPHQL_MODULE_PATH);
    const schema = buildSchema(await readFile(process.env.LINEAR_SCHEMA_PATH, 'utf8'));
    for (const query of queries) assert.deepEqual(validate(schema, parse(query)), []);
});
