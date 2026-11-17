// Exercise the real submitFeedback with a mocked Submilli host: no network, no credential.
// Native build tests separately verify Submilli compatibility.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import { test } from 'node:test';

const secretValues = new Map();
let requests = [];
let checks = [];
let teams = [];
let failStatus = 0;
let createErrors;
globalThis.__feedbackHost = {
    secrets: { get: (name) => secretValues.get(name) },
    check: (capability, context) => { checks.push({ capability, context }); },
    post: (url, body, headers) => {
        requests.push({ url, body, headers: Object.fromEntries(headers) });
        if (failStatus) return { ok: false, status: failStatus, statusText: 'Unauthorized', body: '{"secret":"do not echo"}' };
        let data;
        if (body.query.includes('FeedbackTeamByKey')) data = { teams: { nodes: teams } };
        else if (body.query.includes('FeedbackTeamById')) data = { team: teams[0] ?? null };
        else data = { issueCreate: { success: true, issue: { id: 'issue-uuid', identifier: 'ENG-7', url: 'https://linear.app/acme/issue/ENG-7',
            title: body.variables.input.title, state: { name: 'Triage' } } } };
        return { ok: true, json: () => (createErrors && body.query.includes('issueCreate') ? { errors: createErrors } : { data }) };
    },
};
const source = (await readFile(new URL('../src/lib.ts', import.meta.url), 'utf8'))
    .replace(/^import .*from "submilli:.*";\n/gm, '')
    .replace('const ENDPOINT', 'const { post, secrets, check } = globalThis.__feedbackHost;\nconst ENDPOINT');
const feedback = await import(`data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(source)).toString('base64')}`);

const team = { id: 'team-uuid', key: 'ENG', states: { nodes: [
    { id: 'backlog', name: 'Backlog', type: 'backlog', position: 1 },
    { id: 'triage', name: 'Triage', type: 'triage', position: 0 },
] } };
const confirmed = { original: 'export button does nothing', summary: 'Export button does nothing', confirmed: true, kind: 'bug',
    user: 'dana@example.com', workspace: 'acme', page: '/agents' };

function reset({ key = 'lin_api_testkey', teamSetting = 'ENG', found = [team] } = {}) {
    secretValues.clear();
    if (key !== null) secretValues.set('FEEDBACK_LINEAR_API_KEY', key);
    if (teamSetting !== null) secretValues.set('FEEDBACK_LINEAR_TEAM', teamSetting);
    requests = []; checks = []; teams = found; failStatus = 0; createErrors = undefined;
}

test('files into the triage state of the team named by key, after one capability check', () => {
    reset();
    const filed = feedback.submitFeedback(confirmed);
    assert.deepEqual(checks, [{ capability: 'linear.app/submitFeedback', context: { kind: 'bug', test: false } }]);
    assert.equal(requests.length, 2);
    for (const request of requests) {
        assert.equal(request.url, 'https://api.linear.app/graphql');
        assert.equal(request.headers.Authorization, 'lin_api_testkey');
    }
    assert.deepEqual(requests[0].body.variables, { key: 'ENG' });
    const input = requests[1].body.variables.input;
    assert.equal(input.teamId, 'team-uuid');
    assert.equal(input.stateId, 'triage');
    assert.equal(input.title, 'Feedback (bug): Export button does nothing');
    assert.ok(input.description.includes('```text\nexport button does nothing\n```'));
    assert.deepEqual(filed, { id: 'issue-uuid', identifier: 'ENG-7', url: 'https://linear.app/acme/issue/ENG-7',
        title: 'Feedback (bug): Export button does nothing', state: 'Triage' });
    assert.ok(!JSON.stringify(filed).includes('lin_api_'), 'the key never appears in the result');
});

test('a team UUID is looked up by id; OAuth tokens get the Bearer prefix; no triage means backlog', () => {
    reset({ key: 'oauth-token', teamSetting: '2b508a4d-dabf-4ccd-b3af-79f066c981c0',
        found: [{ ...team, states: { nodes: [team.states.nodes[0]] } }] });
    feedback.submitFeedback({ ...confirmed, test: true });
    assert.ok(requests[0].body.query.includes('FeedbackTeamById'));
    assert.deepEqual(requests[0].body.variables, { id: '2b508a4d-dabf-4ccd-b3af-79f066c981c0' });
    assert.equal(requests[0].headers.Authorization, 'Bearer oauth-token');
    assert.equal(requests[1].body.variables.input.stateId, 'backlog');
    assert.ok(requests[1].body.variables.input.title.startsWith('[TEST] '));
    assert.deepEqual(checks[0].context, { kind: 'bug', test: true });
});

test('unconfirmed feedback is refused before any check, secret read, or request', () => {
    reset();
    assert.throws(() => feedback.submitFeedback({ ...confirmed, confirmed: false }), /confirms/);
    assert.equal(requests.length, 0);
    assert.equal(checks.length, 0);
});

test('missing setup, unknown teams, and failures are reported without leaking bodies', () => {
    reset({ key: null });
    assert.throws(() => feedback.submitFeedback(confirmed), /FEEDBACK_LINEAR_API_KEY secret is not bound/);
    reset({ teamSetting: null });
    assert.throws(() => feedback.submitFeedback(confirmed), /FEEDBACK_LINEAR_TEAM secret is not bound/);
    reset({ found: [] });
    assert.throws(() => feedback.submitFeedback(confirmed), /was not found/);
    reset();
    failStatus = 401;
    assert.throws(() => feedback.submitFeedback(confirmed), (error) => error.message === 'Linear request failed: HTTP 401 Unauthorized');
    reset();
    createErrors = [{ message: 'Entity not found' }];
    assert.throws(() => feedback.submitFeedback(confirmed), /Linear GraphQL error: Entity not found/);
});
