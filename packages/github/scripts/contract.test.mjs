import assert from 'node:assert/strict';
import { test } from 'node:test';
import { contractHost, loadPackage } from '../../../scripts/package-contract-host.mjs';
const host = contractHost();
const github = await loadPackage(new URL('../src/lib.ts', import.meta.url), host, [new URL('../src/people.ts', import.meta.url)]);
const options = (value = {}) => value;
const repo = { owner: 'Victim-Org', name: 'Mixed-Repo' };
const reaction = { total_count: 3, '+1': 2, '-1': 0, laugh: 0, hooray: 0, confused: 0, heart: 1, rocket: 0, eyes: 0 };
const hit = { number: 7, title: 'Compiler work', repository_url: 'https://api.github.com/repos/Rust-Lang/Rust', html_url: 'https://github.com/rust-lang/rust/issues/7', created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-02T00:00:00Z', reactions: reaction };

test('repository identity is canonical in checks and paths; commit refs retain case', () => {
    host.reset(); host.response(() => ({ data: [] }));
    github.listCommits(repo, options({ sha: 'Main', path: 'src/Lib.ts' }));
    assert.deepEqual(host.checks[0], { capability: 'github.com/commits.list', context: { owner: 'victim-org', repo: 'mixed-repo', branch: null, path: 'src/Lib.ts', ref: 'Main', treeSha: null, head: null, base: null } });
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

const pageLink = (path) => ({ link: `<https://api.github.com${path}?per_page=2&page=3>; rel="next", <https://api.github.com${path}?per_page=2&page=9>; rel="last"` });
const repositoryContext = { owner: 'victim-org', repo: 'mixed-repo', branch: null, path: null, ref: null, treeSha: null, head: null, base: null };

for (const [method, capability, suffix, accept, extraQuery, data, expected] of [
    ['listRepositoryEvents', 'github.com/events.list', '/events', 'application/vnd.github+json', {},
        { id: '1', type: 'ForkEvent', actor: { login: 'hubot' }, created_at: '2026-10-02T00:00:00Z', payload: { action: 'forked', forkee: { full_name: 'hubot/Hello-World' } } },
        { id: '1', type: 'ForkEvent', actorLogin: 'hubot', createdAt: '2026-10-02T00:00:00Z', action: 'forked', forkFullName: 'hubot/Hello-World' }],
    ['listStargazers', 'github.com/stargazers.list', '/stargazers', 'application/vnd.github.star+json', {},
        { starred_at: '2026-10-01T00:00:00Z', user: { login: 'octocat', type: 'User' } },
        { login: 'octocat', starredAt: '2026-10-01T00:00:00Z', accountType: 'User' }],
    ['listForks', 'github.com/forks.list', '/forks', 'application/vnd.github+json', { sort: 'newest' },
        { full_name: 'hubot/Hello-World', html_url: 'https://github.com/hubot/Hello-World', created_at: '2026-10-02T00:00:00Z', owner: { login: 'hubot' } },
        { fullName: 'hubot/Hello-World', ownerLogin: 'hubot', createdAt: '2026-10-02T00:00:00Z', url: 'https://github.com/hubot/Hello-World' }],
]) {
    test(`${method} is one GET of the owner's repository, checked with the lowercased identity`, () => {
        const path = `/repos/victim-org/mixed-repo${suffix}`;
        host.reset(); host.response(() => ({ data: [data], headers: pageLink(path) }));
        const page = github[method]('Victim-Org', 'Mixed-Repo', options({ limit: 2, pageToken: '2' }));
        assert.equal(host.requests.length, 1);
        assert.equal(host.requests[0].method, 'get');
        assert.deepEqual(host.checks, [{ capability, context: repositoryContext }]);
        const url = new URL(host.requests[0].url);
        assert.equal(url.host, 'api.github.com'); assert.equal(url.pathname, path);
        assert.deepEqual(Object.fromEntries(url.searchParams), { per_page: '2', page: '2', ...extraQuery });
        assert.equal(host.requests[0].headers.get('Accept'), accept);
        assert.equal(host.requests[0].headers.get('X-GitHub-Api-Version'), '2026-03-10');
        assert.deepEqual(page, { items: [expected], nextPageToken: '3', isComplete: false });

        host.reset(); host.response(() => ({ data: [] }));
        assert.deepEqual(github[method]('octocat', 'repo'), { items: [], nextPageToken: '', isComplete: true });
        assert.equal(new URL(host.requests[0].url).searchParams.get('per_page'), '30');
    });

    test(`${method} surfaces GitHub's refusal as a GitHubError and checks before any request`, () => {
        for (const [status, code, message] of [[404, 'not_found', 'Not Found'], [403, 'forbidden', 'Must have push access'], [401, 'unauthorized', 'Bad credentials']]) {
            host.reset(); host.response(() => ({ status, data: { message, documentation_url: 'https://docs.github.com/rest' }, headers: { 'x-github-request-id': 'request-1' } }));
            assert.throws(() => github[method]('octocat', 'repo'), (e) => e instanceof github.GitHubError && e.code === code && e.status === status && e.message === message && e.requestId === 'request-1');
        }
        host.reset(); host.response(() => ({ status: 403, data: { message: 'API rate limit exceeded' }, headers: { 'x-ratelimit-remaining': '0', 'retry-after': '5' } }));
        assert.throws(() => github[method]('octocat', 'repo'), (e) => e.code === 'rate_limited' && e.retryAfterSeconds === 5);
        host.reset(); host.denial((cap) => cap === capability);
        assert.throws(() => github[method]('octocat', 'repo'), /Capability denied/);
        assert.equal(host.requests.length, 0);
    });
}

const profile = {
    login: 'octocat', id: 583231, type: 'User', url: 'https://api.github.com/users/octocat', html_url: 'https://github.com/octocat', name: 'The Octocat', company: null, blog: '', location: 'San Francisco',
    email: 'octocat@example.invalid', notification_email: 'octocat@example.invalid', bio: null, twitter_username: null, public_repos: 8, followers: 9000, following: 9, created_at: '2011-01-25T18:44:36Z',
};

test('getUser checks and sends one lowercased login and leaves the email address behind', () => {
    host.reset(); host.response(() => ({ data: profile }));
    const user = github.getUser('OctoCat');
    assert.deepEqual(host.checks, [{ capability: 'github.com/users.get', context: { login: 'octocat' } }]);
    assert.equal(host.requests.length, 1); assert.equal(host.requests[0].method, 'get');
    assert.equal(new URL(host.requests[0].url).pathname, '/users/octocat');
    assert.equal(new URL(host.requests[0].url).search, '');
    assert.deepEqual(user, {
        login: 'octocat', name: 'The Octocat', company: '', bio: '', blog: '', twitterUsername: '', location: 'San Francisco', url: 'https://github.com/octocat',
        accountType: 'User', followers: 9000, following: 9, publicRepos: 8, createdAt: '2011-01-25T18:44:36Z', hasPublicEmail: true,
    });
    assert.ok(!JSON.stringify(user).includes('example.invalid'));
    host.response(() => ({ data: { ...profile, email: null } }));
    assert.equal(github.getUser('octocat').hasPublicEmail, false);
});

test('a bot login keeps its suffix in the check and is escaped in the path', () => {
    host.reset(); host.response(() => ({ data: [] }));
    github.listUserSocialAccounts('Dependabot[bot]');
    assert.deepEqual(host.checks[0].context, { login: 'dependabot[bot]' });
    assert.equal(new URL(host.requests[0].url).pathname, '/users/dependabot%5Bbot%5D/social_accounts');
});

for (const [method, capability, suffix, data, expected] of [
    ['listUserOrganizations', 'github.com/userOrganizations.list', '/orgs', { login: 'github', url: 'https://api.github.com/orgs/github' }, { login: 'github', url: 'https://github.com/github' }],
    ['listUserSocialAccounts', 'github.com/userSocialAccounts.list', '/social_accounts', { provider: 'twitter', url: 'https://twitter.com/octocat' }, { provider: 'twitter', url: 'https://twitter.com/octocat' }],
]) {
    test(`${method} pages one lowercased login`, () => {
        const path = `/users/octocat${suffix}`;
        host.reset(); host.response(() => ({ data: [data], headers: pageLink(path) }));
        const page = github[method]('OctoCat', options({ limit: 2, pageToken: '2' }));
        assert.deepEqual(host.checks, [{ capability, context: { login: 'octocat' } }]);
        assert.equal(host.requests.length, 1); assert.equal(host.requests[0].method, 'get');
        const url = new URL(host.requests[0].url);
        assert.equal(url.pathname, path);
        assert.deepEqual(Object.fromEntries(url.searchParams), { per_page: '2', page: '2' });
        assert.deepEqual(page, { items: [expected], nextPageToken: '3', isComplete: false });
    });
}

test('an unknown login throws, and a crafted login reaches neither the check nor HTTP', () => {
    for (const method of ['getUser', 'listUserOrganizations', 'listUserSocialAccounts']) {
        host.reset(); host.response(() => ({ status: 404, data: { message: 'Not Found' } }));
        assert.throws(() => github[method]('octocat'), (e) => e instanceof github.GitHubError && e.code === 'not_found' && e.status === 404);
        for (const login of ['..', 'a/b', '../octocat', '%2e%2e', 'octocat?x=1', 'octocat#y', 'octocat\n', 'octo--cat', '-a', 'a-', '\u212Aelvin', 'a'.repeat(40)]) {
            host.reset();
            assert.throws(() => github[method](login), (e) => e instanceof github.GitHubError && e.code === 'invalid_input' && e.status === 0);
            assert.equal(host.checks.length, 0); assert.equal(host.requests.length, 0);
        }
    }
});
