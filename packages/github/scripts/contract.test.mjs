import assert from 'node:assert/strict';
import { test } from 'node:test';
import { contractHost, loadPackage, nullableFields } from '../../../scripts/package-contract-host.mjs';
const host = contractHost();
const github = await loadPackage(new URL('../src/lib.ts', import.meta.url), host);
const options = (value = {}) => nullableFields(value);
const repo = { owner: 'Victim-Org', name: 'Mixed-Repo' };
const reaction = { total_count: 3, '+1': 2, '-1': 0, laugh: 0, hooray: 0, confused: 0, heart: 1, rocket: 0, eyes: 0 };
const hit = { number: 7, title: 'Compiler work', repository_url: 'https://api.github.com/repos/Rust-Lang/Rust', html_url: 'https://github.com/rust-lang/rust/issues/7', created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-02T00:00:00Z', reactions: reaction };

test('repository identity is canonical in checks and paths; commit refs retain case', () => {
    host.reset(); host.response(() => ({ data: [] }));
    github.listCommits(repo, options({ sha: 'Main', path: 'src/Lib.ts' }));
    assert.deepEqual(host.checks[0], { capability: 'github.com/commits.list', context: { owner: 'victim-org', repo: 'mixed-repo', ref: 'Main', path: 'src/Lib.ts' } });
    const url = new URL(host.requests[0].url);
    assert.equal(url.pathname, '/repos/victim-org/mixed-repo/commits');
    assert.equal(url.searchParams.get('sha'), 'Main'); assert.equal(url.searchParams.get('path'), 'src/Lib.ts');
    host.denial((cap, ctx) => ctx.owner === 'victim-org');
    assert.throws(() => github.getRepository(repo), /Capability denied/);
    assert.equal(host.requests.length, 1);
});

test('branch mutations canonicalize request identities and preserve branch case', () => {
    for (const method of ['createBranch', 'deleteBranch']) {
        host.reset(); host.response(() => ({ data: { ref: 'refs/heads/Feature', object: { sha: 'sha' } } }));
        if (method === 'createBranch') github.createBranch(repo, { name: 'Feature', fromSha: 'a'.repeat(40) });
        else github.deleteBranch(repo, 'Feature');
        assert.equal(host.checks[0].context.owner, 'victim-org');
        assert.equal(host.checks[0].context.repo, 'mixed-repo');
        assert.ok(new URL(host.requests[0].url).pathname.startsWith('/repos/victim-org/mixed-repo/'));
        if (method === 'createBranch') assert.equal(host.requests[0].body.ref, 'refs/heads/Feature');
        else assert.ok(new URL(host.requests[0].url).pathname.endsWith('/heads/Feature'));
    }
});

test('cross-repository issue search sends structured author and inclusive dates in one request', () => {
    host.reset(); host.response(() => ({ data: { total_count: 4, incomplete_results: true, items: [hit] }, headers: { link: '<https://api.github.com/search/issues?page=2>; rel="next"' } }));
    const page = github.searchIssuesAcrossRepositories('compiler org:rust-lang', options({ author: 'Niko', createdSince: '2026-01-01', createdUntil: '2026-01-31', limit: 2 }));
    assert.equal(host.requests.length, 1);
    assert.equal(new URL(host.requests[0].url).searchParams.get('q'), 'compiler org:rust-lang is:issue author:niko created:2026-01-01..2026-01-31');
    assert.equal(host.checks[0].capability, 'github.com/issues.searchAcrossRepositories');
    assert.equal(host.checks[0].context.author, 'niko');
    assert.deepEqual(page.items[0].repository, { owner: 'rust-lang', name: 'rust' });
    assert.equal(page.items[0].issue.reactions.totalCount, 3);
    assert.equal(page.items[0].issue.createdAt, hit.created_at);
    assert.equal(page.nextPageToken, '2'); assert.equal(page.totalCount, 4); assert.equal(page.incompleteResults, true); assert.equal(page.isCapped, false);
});

test('PR summaries do not fetch details; existing PR search retains detail calls', () => {
    host.reset(); host.response(({ url }) => ({ data: url.includes('/search/issues') ? { total_count: 1, incomplete_results: false, items: [{ ...hit, pull_request: {}, draft: false }] } : { number: 7, additions: 12 } }));
    const light = github.searchPullRequestsAcrossRepositories('compiler', options());
    assert.equal(host.requests.length, 1); assert.equal(light.items[0].pullRequest.number, 7); assert.equal(light.items[0].pullRequest.draft, false);
    host.reset();
    const full = github.searchPullRequests({ owner: 'rust-lang', name: 'rust' }, 'compiler');
    assert.equal(host.requests.length, 2); assert.equal(full.items[0].additions, 12);
    host.reset();
    github.searchPullRequestSummaries(repo, 'compiler', options());
    assert.equal(host.requests.length, 1); assert.ok(new URL(host.requests[0].url).searchParams.get('q').includes('repo:victim-org/mixed-repo'));
});

test('unavailable reactions and draft metadata differ from confirmed zero and false', () => {
    for (const [reactions, expected] of [[null, null], [{ total_count: 0 }, 0]]) {
        host.reset(); host.response(() => ({ data: { total_count: 1, incomplete_results: false, items: [{ ...hit, reactions, pull_request: {} }] } }));
        const value = github.searchPullRequestsAcrossRepositories('compiler', options()).items[0].pullRequest;
        assert.equal(value.reactions === null ? null : value.reactions.totalCount, expected);
        if (value.reactions !== null) assert.equal(value.reactions.heart, null);
        assert.equal(value.draft, null);
    }
    host.response(() => ({ data: { ...hit, reactions: reaction } }));
    assert.equal(github.getIssue(repo, 7).reactions.thumbsUp, 2);
    host.response(() => ({ data: [{ ...hit, reactions: reaction }] }));
    assert.equal(github.listIssueComments(repo, 7).items[0].reactions.heart, 1);
});

test('selected PR reactions use an explicit grant without loosening getIssue', () => {
    host.reset(); host.response(() => ({ data: { ...hit, pull_request: {}, reactions: reaction } }));
    assert.equal(github.getPullRequestReactions(repo, 7).heart, 1);
    assert.equal(host.checks[0].capability, 'github.com/pulls.getReactions');
    assert.equal(new URL(host.requests[0].url).pathname, '/repos/victim-org/mixed-repo/issues/7');
    assert.throws(() => github.getIssue(repo, 7), (e) => e.code === 'wrong_resource_type');
    host.response(() => ({ data: hit }));
    assert.throws(() => github.getPullRequestReactions(repo, 7), (e) => e.code === 'wrong_resource_type');
    host.response(() => ({ status: 404, data: {} }));
    assert.equal(github.getPullRequestReactions(repo, 7), null);
});

test('pagination reports the search ceiling and timeout independently', () => {
    host.reset(); host.response(() => ({ data: { total_count: 1200, incomplete_results: true, items: [] }, headers: { link: '<https://api.github.com/search/issues?page=11>; rel="next"' } }));
    const page = github.searchIssuesAcrossRepositories('compiler', options({ limit: 100, pageToken: '10' }));
    assert.equal(page.nextPageToken, ''); assert.equal(page.isComplete, true); assert.equal(page.isCapped, true); assert.equal(page.incompleteResults, true); assert.equal(page.totalCount, 1200);
    assert.throws(() => github.searchIssuesAcrossRepositories('compiler', options({ limit: 100, pageToken: '11' })), /1,000/);
    assert.equal(host.requests.length, 1);
});

test('query, date, pagination and separate capability boundaries fail before HTTP', () => {
    for (const query of ['foo OR bar', 'is:pr', 'is:"pr"', 'type:issue', 'author:niko', 'created:>2020-01-01', 'foo ＯＲ bar']) {
        host.reset(); assert.throws(() => github.searchIssuesAcrossRepositories(query, options()), (e) => e instanceof github.GitHubError && e.code === 'unsafe_search_query'); assert.equal(host.requests.length, 0);
    }
    for (const value of [{ author: 'niko OR author:other' }, { createdSince: '2026-02-30' }, { createdSince: '2026-02-01', createdUntil: '2026-01-01' }, { limit: 1.5 }, { pageToken: '0' }]) {
        host.reset(); assert.throws(() => github.searchIssuesAcrossRepositories('compiler', options(value)), github.GitHubError); assert.equal(host.requests.length, 0);
    }
    host.reset(); host.denial((cap) => cap.endsWith('AcrossRepositories'));
    assert.throws(() => github.searchPullRequestsAcrossRepositories('compiler', options()), /Capability denied/); assert.equal(host.requests.length, 0);
    for (const query of ['repo:other/repo', '""org:other', 'owner:other']) assert.throws(() => github.searchPullRequestSummaries(repo, query, options()), github.GitHubError);
});

test('invalid search responses and rate-limit errors remain typed', () => {
    host.reset(); host.response(() => ({ data: { items: [] } }));
    assert.throws(() => github.searchIssuesAcrossRepositories('compiler', options()), (e) => e.code === 'invalid_response' && e.message.includes('total_count'));
    host.reset(); host.response(() => ({ data: { total_count: 1, incomplete_results: false, items: [{ ...hit, repository_url: null }] } }));
    assert.throws(() => github.searchIssuesAcrossRepositories('compiler', options()), (e) => e instanceof github.GitHubError && e.code === 'invalid_response');
    host.response(() => ({ data: { total_count: 1, incomplete_results: false, items: [{ ...hit, reactions: { total_count: -1 } }] } }));
    assert.throws(() => github.searchIssuesAcrossRepositories('compiler', options()), (e) => e.code === 'invalid_response');
    host.response(() => ({ status: 403, data: { message: 'API rate limit exceeded' }, headers: { 'x-ratelimit-remaining': '0', 'retry-after': '5' } }));
    assert.throws(() => github.searchIssuesAcrossRepositories('compiler', options()), (e) => e instanceof github.GitHubError && e.status === 403 && e.retryAfterSeconds === 5 && e.rateLimitRemaining === 0);
});
