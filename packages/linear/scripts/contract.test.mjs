// Exercise the actual package wrappers with a mocked Submilli host, without
// network access. Native build tests separately verify Submilli compatibility.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import { test } from 'node:test';

let token = 'oauth-test-token';
let response;
// When set, the whole response envelope instead of `{ data: response }`.
let envelope;
let request;
let denied = false;
const capabilities = [];
const contexts = [];
// Every request since a test last reset this, in order; `request` is the last of them.
let requests = [];
let lookupResponse = null;
// When non-empty, successive requests take their data from here, in order.
let queued = [];
const resolvedTeam = { team: { id: 'team' }, labels: { nodes: [{ id: 'feedback' }] }, project: { id: 'project' } };
globalThis.__linearHost = {
    secrets: { get: () => token },
    check: (capability, context) => {
        capabilities.push(capability);
        contexts.push(context);
        if (denied) throw new Error('Capability denied');
    },
    post: (url, body, headers) => {
        request = { url, body, headers };
        requests.push(request);
        queries.add(body.query);
        let data = queued.length > 0 ? queued.shift() : response;
        if (queued.length === 0 && data === response && body.query.includes('team { id }')) {
            data = lookupResponse ?? (response === null ? null : {
                issue: Object.hasOwn(response, 'issue') && response.issue === null ? null : resolvedTeam,
                comment: { issue: resolvedTeam },
                agentSession: { issue: resolvedTeam, comment: null },
            });
        }
        return { ok: true, json: () => envelope ?? { data } };
    },
};
// lib.ts imports its helpers from internal.ts; inline that module ahead of it,
// since a data: URL module cannot resolve the relative import.
const internal = await readFile(new URL('../src/internal.ts', import.meta.url), 'utf8');
const source = internal + (await readFile(new URL('../src/lib.ts', import.meta.url), 'utf8'))
    .replace(/^import .*from "submilli:.*";\n/gm, '')
    .replace(/^(import|export) \{[^}]*\} from "\.\/internal";\n/gm, '')
    .replace('const ENDPOINT', 'const { post, secrets, check } = globalThis.__linearHost;\nconst ENDPOINT');
const linear = await import(`data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(source)).toString('base64')}`);
const session = { id: 'session', status: 'active', issue: { id: 'issue' }, comment: null,
    url: null, summary: null, externalUrls: [], plan: null };
const page = { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } };

// Also emits the captured operations for optional validation against Linear's SDL.
const queries = new Set();
function sent(capability, inputType, input) {
    assert.equal(capabilities.at(-1), `linear.app/${capability}`);
    assert.equal(request.url, 'https://api.linear.app/graphql');
    assert.ok(request.body.query.includes(inputType));
    assert.deepEqual(request.body.variables.input, input);
    queries.add(request.body.query);
}

test('create sessions on issues and comments, preserving external URLs', () => {
    for (const [method, mutation, type, target] of [
        ['createAgentSessionOnIssue', 'agentSessionCreateOnIssue', 'AgentSessionCreateOnIssue!', { issueId: 'issue' }],
        ['createAgentSessionOnComment', 'agentSessionCreateOnComment', 'AgentSessionCreateOnComment!', { commentId: 'comment' }],
    ]) {
        const input = { ...target, externalUrls: [{ label: 'Console', url: 'https://example.com/session' }] };
        response = { [mutation]: { success: true, agentSession: session } };
        assert.deepEqual(linear[method](input), session);
        sent(method, type, input);
        for (const payload of [{ success: false, agentSession: session }, { success: true, agentSession: null }]) {
            response = { [mutation]: payload };
            assert.throws(() => linear[method](input), /did not succeed/);
        }
    }
});

test('session reads and partial updates use the correct GraphQL operation', () => {
    response = { agentSession: session };
    assert.deepEqual(linear.getAgentSession('session'), session);
    assert.deepEqual(request.body.variables, { id: 'session' });
    queries.add(request.body.query);
    const input = { plan: [{ content: 'Inspect', status: 'completed' }], addedExternalUrls: [{ label: 'PR', url: 'https://example.com/pr' }] };
    response = { agentSessionUpdate: { success: true, agentSession: session } };
    assert.deepEqual(linear.updateAgentSession('session', input), session);
    sent('updateAgentSession', 'AgentSessionUpdateInput!', input);
    assert.equal(request.body.variables.id, 'session');
    assert.ok(!('externalUrls' in request.body.variables.input));
    response = { agentSessionUpdate: { success: false, agentSession: null } };
    assert.throws(() => linear.updateAgentSession('session', input), /did not succeed/);
});

test('all five activity types and signal metadata are sent intact', () => {
    for (const content of [
        { type: 'thought', body: 'Working' },
        { type: 'action', action: 'Search', parameter: 'code', result: 'Found' },
        { type: 'elicitation', body: 'Which repository?' },
        { type: 'response', body: 'Done' },
        { type: 'error', body: 'Failed' },
    ]) {
        const input = { agentSessionId: 'session', content };
        const activity = { id: 'activity', content, signal: null, signalMetadata: null, ephemeral: false };
        response = { agentActivityCreate: { success: true, agentActivity: activity } };
        assert.deepEqual(linear.createAgentActivity(input), activity);
        sent('createAgentActivity', 'AgentActivityCreateInput!', input);
    }
    for (const input of [
        { agentSessionId: 'session', content: { type: 'thought', body: 'Working' }, ephemeral: true },
        { agentSessionId: 'session', content: { type: 'elicitation', body: 'Connect' }, signal: 'auth', signalMetadata: { url: 'https://example.com/auth', providerName: 'GitHub' } },
        { agentSessionId: 'session', content: { type: 'elicitation', body: 'Choose' }, signal: 'select', signalMetadata: { options: [{ label: 'Runtime', value: 'runtime' }] } },
    ]) {
        linear.createAgentActivity(input);
        sent('createAgentActivity', 'AgentActivityCreateInput!', input);
    }
    response = { agentActivityCreate: { success: false, agentActivity: null } };
    assert.throws(() => linear.createAgentActivity({ agentSessionId: 'session', content: { type: 'response', body: 'Done' } }), /did not succeed/);
});

test('comments support threaded replies; comment and activity lists preserve pagination', () => {
    const input = { issueId: 'issue', parentId: 'parent', body: 'Reply' };
    response = { issue: { team: { id: 'team' } }, commentCreate: { success: true, comment: { id: 'reply', ...input, parent: { id: 'parent' } } } };
    assert.equal(linear.createComment(input).parent.id, 'parent');
    sent('createComment', 'CommentCreateInput!', input);
    for (const [method, field, connection] of [
        ['listComments', 'issue', 'comments'],
        ['listAgentActivities', 'agentSession', 'activities'],
    ]) {
        response = { [field]: { [connection]: page } };
        assert.deepEqual(linear[method]('target', { first: 10, after: 'cursor' }), page);
        assert.deepEqual(request.body.variables, { id: 'target', first: 10, after: 'cursor' });
        assert.equal(capabilities.at(-1), `linear.app/${method}`);
        queries.add(request.body.query);
    }
});

test('an update and a comment are checked against the team of their issue', () => {
    const issue = { id: 'issue', title: 'Renamed' };
    for (const [capability, call, mutation, context] of [
        ['updateIssue', () => linear.updateIssue('issue', { title: 'Renamed' }), 'issueUpdate', { teamId: 'team', projectId: null }],
        ['updateIssue', () => linear.updateIssue('issue', { projectId: 'project' }), 'issueUpdate', { teamId: 'team', projectId: 'project' }],
        ['createComment', () => linear.createComment({ issueId: 'issue', body: 'Note' }), 'commentCreate', { teamId: 'team', labelIds: ['feedback'], projectId: 'project' }],
    ]) {
        response = {
            issue: { team: { id: 'team' } },
            issueUpdate: { success: true, issue },
            commentCreate: { success: true, comment: { id: 'comment' } },
        };
        requests = [];
        call();
        assert.equal(capabilities.at(-1), `linear.app/${capability}`);
        // The team comes from Linear's answer about the issue, not from an argument.
        assert.deepEqual({ ...contexts.at(-1) }, context);
        assert.equal(requests.length, 2);
        assert.ok(requests[0].body.query.includes('team { id }'));
        assert.deepEqual(requests[0].body.variables, { id: 'issue' });
        assert.ok(requests[1].body.query.includes(mutation));
        queries.add(requests[0].body.query);

        // A denial follows the read and precedes the mutation.
        denied = true;
        requests = [];
        assert.throws(call, /Capability denied/);
        assert.equal(requests.length, 1);
        assert.ok(!requests[0].body.query.includes('mutation'));
        denied = false;

        response = { issue: null };
        requests = [];
        assert.throws(call, /issue was not found/);
        assert.equal(requests.length, 1);
    }
});

test('issue reads and all session operations check the resolved team before sensitive requests', () => {
    const calls = [
        ['getIssue', () => linear.getIssue('issue')],
        ['listComments', () => linear.listComments('issue')],
        ['getAgentSession', () => linear.getAgentSession('session')],
        ['listAgentActivities', () => linear.listAgentActivities('session')],
        ['createAgentActivity', () => linear.createAgentActivity({ agentSessionId: 'session', content: { type: 'thought', body: 'Working' } })],
        ['updateAgentSession', () => linear.updateAgentSession('session', { summary: 'Working' })],
        ['createAgentSessionOnIssue', () => linear.createAgentSessionOnIssue({ issueId: 'issue' })],
        ['createAgentSessionOnComment', () => linear.createAgentSessionOnComment({ commentId: 'comment' })],
    ];
    lookupResponse = { issue: { team: { id: 'blocked-team' } }, comment: { issue: { team: { id: 'blocked-team' } } }, agentSession: { issue: null, comment: { issue: { team: { id: 'blocked-team' } } } } };
    denied = true;
    for (const [method, call] of calls) {
        requests = [];
        assert.throws(call, /Capability denied/);
        assert.deepEqual(contexts.at(-1), { teamId: 'blocked-team' });
        assert.equal(capabilities.at(-1), `linear.app/${method}`);
        assert.equal(requests.length, 1);
        assert.ok(requests[0].body.query.includes('team { id }'));
        assert.ok(!requests[0].body.query.includes('mutation'));
    }
    denied = false;
    lookupResponse = { issue: null };
    requests = [];
    assert.equal(linear.getIssue('absent'), null);
    assert.deepEqual(contexts.at(-1), { teamId: null });
    assert.equal(requests.length, 1);
    lookupResponse = { agentSession: { issue: null, comment: null } };
    assert.throws(() => linear.getAgentSession('session'), /no issue or comment team/);
    lookupResponse = { comment: null };
    assert.throws(() => linear.createAgentSessionOnComment({ commentId: 'missing' }), /comment was not found/);
    lookupResponse = { comment: { issue: null } };
    assert.throws(() => linear.createAgentSessionOnComment({ commentId: 'project-comment' }), /comment has no issue team/);
    lookupResponse = null;
});

test('issue creation is checked with its team and the labels and project it is created with', () => {
    const issue = { id: 'new', title: 'Hello' };
    response = { issueCreate: { success: true, issue } };
    const input = { teamId: 'team', title: 'Hello', labelIds: ['feedback'], projectId: 'project' };
    assert.deepEqual(linear.createIssue(input), issue);
    sent('createIssue', 'IssueCreateInput!', input);
    assert.deepEqual({ ...contexts.at(-1) }, { teamId: 'team', labelIds: ['feedback'], projectId: 'project' });
    linear.createIssue({ teamId: 'team', title: 'Plain' });
    assert.deepEqual({ ...contexts.at(-1) }, { teamId: 'team', labelIds: [], projectId: null });
    queries.add(request.body.query);
});

test('a comment is checked against the labels and project its issue has in Linear, not the caller\'s word', () => {
    lookupResponse = { issue: { team: { id: 'team' }, labels: { nodes: [] }, project: null } };
    response = { commentCreate: { success: true, comment: { id: 'comment' } } };
    requests = [];
    linear.createComment({ issueId: 'issue', body: 'Note' });
    assert.deepEqual({ ...contexts.at(-1) }, { teamId: 'team', labelIds: [], projectId: null });
    assert.ok(requests[0].body.query.includes('labels(first: 250) { nodes { id } } project { id }'));
    queries.add(requests[0].body.query);
    denied = true;
    requests = [];
    assert.throws(() => linear.createComment({ issueId: 'issue', body: 'Note' }), /Capability denied/);
    assert.equal(requests.length, 1, 'a denied comment makes only the lookup');
    denied = false;
    lookupResponse = null;
});

test('findSimilarIssues checks first, then returns only ids and scores of open labelled lookalikes', () => {
    const title = (id, text, closed) => ({ id, title: text, completedAt: closed === 'done' ? '2026-01-01T00:00:00Z' : null, canceledAt: closed === 'canceled' ? '2026-01-01T00:00:00Z' : null });
    const pages = [
        { nodes: [
            title('same', 'Feedback (bug): Package-not-granted error on save does not say how to fix it'),
            title('other', 'Feedback (idea): Dark mode for the console'),
            title('closed', 'Feedback (bug): Package-not-granted error on save does not say how to fix it', 'done'),
        ], pageInfo: { hasNextPage: true, endCursor: 'c1' } },
        { nodes: [
            title('close', 'Feedback (bug): Save error says package not granted'),
            title('cancelled', 'Package not granted error on save', 'canceled'),
        ], pageInfo: { hasNextPage: false, endCursor: null } },
    ];
    const serve = () => { queued = pages.map((issues) => ({ issues })); };
    envelope = undefined;
    requests = [];
    serve();
    try {
        const found = linear.findSimilarIssues({ teamId: 'team', labelId: 'feedback', projectId: 'project' }, 'Package-not-granted error on save gives no way to fix it');
        assert.equal(capabilities.at(-1), 'linear.app/findSimilarIssues');
        assert.deepEqual({ ...contexts.at(-1) }, { teamId: 'team', labelId: 'feedback', projectId: 'project' });
        // Only ids and scores, best first; closed, canceled and unrelated issues are left out.
        assert.deepEqual(found.map((r) => Object.keys(r).sort()), found.map(() => ['id', 'score']));
        assert.deepEqual(found.map((r) => r.id), ['same', 'close']);
        assert.ok(found[0].score > found[1].score && found[1].score >= 0.5);
        assert.ok(!JSON.stringify(found).includes('Package'), 'no title text is returned');
        // Both pages were read with the team and (label or project) filter, and only ids, titles and close dates were asked for.
        assert.equal(requests.length, 2);
        assert.deepEqual(JSON.parse(JSON.stringify(requests[0].body.variables)), { first: 250, filter: { team: { id: { eq: 'team' } }, or: [{ labels: { some: { id: { eq: 'feedback' } } } }, { project: { id: { eq: 'project' } } }] } });
        assert.equal(requests[1].body.variables.after, 'c1');
        assert.ok(requests[0].body.query.includes('nodes { id title completedAt canceledAt }'));
        assert.ok(!requests[0].body.query.includes('description'));
        queries.add(requests[0].body.query);
        // limit and minScore bound the result.
        serve();
        assert.deepEqual(linear.findSimilarIssues({ teamId: 'team', labelId: 'feedback' }, 'Package-not-granted error on save gives no way to fix it', { limit: 1 }).map((r) => r.id), ['same']);
        serve();
        assert.deepEqual(linear.findSimilarIssues({ teamId: 'team', projectId: 'project' }, 'Package-not-granted error on save gives no way to fix it', { minScore: 1 }), []);
        assert.deepEqual({ ...contexts.at(-1) }, { teamId: 'team', labelId: null, projectId: 'project' });
        assert.deepEqual(JSON.parse(JSON.stringify(requests.at(-1).body.variables)).filter, { team: { id: { eq: 'team' } }, project: { id: { eq: 'project' } } });
        queued = [];
        // Text with no significant words makes no request.
        requests = [];
        assert.deepEqual(linear.findSimilarIssues({ teamId: 'team', labelId: 'feedback' }, 'the and of'), []);
        assert.equal(requests.length, 0);
        // A denied check makes no request at all.
        denied = true;
        assert.throws(() => linear.findSimilarIssues({ teamId: 'team', labelId: 'other-label' }, 'anything here'), /Capability denied/);
        assert.equal(requests.length, 0);
    } finally {
        denied = false;
        queued = [];
    }
});

test('session creation snapshots target getters once', () => {
    let reads = 0;
    lookupResponse = { issue: resolvedTeam };
    response = { agentSessionCreateOnIssue: { success: true, agentSession: session } };
    linear.createAgentSessionOnIssue({ get issueId() { reads++; return reads === 1 ? 'allowed-issue' : 'blocked-issue'; } });
    assert.equal(reads, 1);
    assert.equal(requests.at(-2).body.variables.id, 'allowed-issue');
    assert.equal(requests.at(-1).body.variables.input.issueId, 'allowed-issue');
    lookupResponse = null;
});

test('OAuth uses Bearer, personal keys remain raw, existing Bearer is not doubled', () => {
    response = { agentSession: session };
    for (const [credential, expected] of [['oauth-token', 'Bearer oauth-token'], ['lin_api_test', 'lin_api_test'], ['Bearer existing', 'Bearer existing']]) {
        token = credential;
        linear.getAgentSession('session');
        assert.equal(request.headers.get('Authorization'), expected);
    }
});

test('denied session reads make only the team lookup', () => {
    denied = true;
    request = null;
    assert.throws(() => linear.getAgentSession('session'), /Capability denied/);
    assert.ok(request.body.query.includes('team { id }'));
    assert.ok(!request.body.query.includes('status'));
    denied = false;
});

test('GraphQL errors remain actionable', () => {
    response = null;
    assert.throws(() => linear.getAgentSession('session'), /no data/);
    envelope = { errors: [{ message: 'Entity not found', path: null, extensions: null }] };
    assert.throws(() => linear.getAgentSession('session'), /^Error: Linear GraphQL error: Entity not found$/);
    envelope = { data: null, errors: null };
    assert.throws(() => linear.getAgentSession('session'), /no data/);
    envelope = undefined;
});

test('an unfiltered issue list is checked with a null team and sends only the default page', () => {
    response = { issues: page };
    assert.deepEqual(linear.listIssues(), page);
    assert.deepEqual({ ...contexts.at(-1) }, { teamId: null, labelId: null });
    assert.deepEqual(JSON.parse(JSON.stringify(request.body.variables)), { first: 50 });
    linear.listIssues({ teamId: 'team', stateType: 'started' }, { after: 'cursor' });
    assert.deepEqual({ ...contexts.at(-1) }, { teamId: 'team', labelId: null });
    assert.deepEqual(JSON.parse(JSON.stringify(request.body.variables)), {
        first: 50, after: 'cursor', filter: { team: { id: { eq: 'team' } }, state: { type: { eq: 'started' } } },
    });
    linear.listIssues({ teamId: 'team', labelId: 'feedback' });
    assert.deepEqual({ ...contexts.at(-1) }, { teamId: 'team', labelId: 'feedback' });
    assert.deepEqual(JSON.parse(JSON.stringify(request.body.variables)).filter, {
        team: { id: { eq: 'team' } }, labels: { some: { id: { eq: 'feedback' } } },
    });
    queries.add(request.body.query);
});

// Supply a local SDL and an installed graphql module for an additional schema check.
test('queries validate against the public Linear schema when supplied', async (t) => {
    if (!process.env.LINEAR_SCHEMA_PATH || !process.env.GRAPHQL_MODULE_PATH) {
        t.skip('Set LINEAR_SCHEMA_PATH and GRAPHQL_MODULE_PATH to validate against Linear SDL');
        return;
    }
    const { buildSchema, parse, validate } = await import(process.env.GRAPHQL_MODULE_PATH);
    const schema = buildSchema(await readFile(process.env.LINEAR_SCHEMA_PATH, 'utf8'));
    for (const query of queries) assert.deepEqual(validate(schema, parse(query)), []);
});
