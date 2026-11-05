import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    CommitFileChange,
    GitHubError,
    RepositoryRef,
    UpdateIssueInput,
    createBranch,
    createIssue,
    getBranch,
    getCommit,
    getTree,
    getUser,
    getViewer,
    listBranches,
    listCommits,
    listForks,
    listIssues,
    listRepositoryEvents,
    listStargazers,
    listUserOrganizations,
    listUserSocialAccounts,
    readFile,
    searchCode,
    updateIssue,
} from "@submilli/github";
import {
    ApiEvent,
    ApiFork,
    ApiStargazer,
    ApiUserOrganization,
    ApiUserProfile,
    ApiUserSocialAccount,
    repositoryEventFrom,
    repositoryForkFrom,
    stargazerFrom,
    userOrganizationFrom,
    userProfileFrom,
    userSocialAccountFrom,
} from "../src/people";

// Shaped like GitHub's `GET /users/{login}`, with the fields the package must not pass on.
const PROFILE_JSON = `{
    "login": "octocat", "id": 583231, "node_id": "MDQ6VXNlcjU4MzIzMQ==", "type": "User", "site_admin": false,
    "url": "https://api.github.com/users/octocat", "html_url": "https://github.com/octocat",
    "name": "The Octocat", "company": "@github", "blog": "https://github.blog", "location": "San Francisco",
    "email": "octocat@example.invalid", "notification_email": "octocat@example.invalid",
    "hireable": null, "bio": "A mascot", "twitter_username": "octocat",
    "public_repos": 8, "public_gists": 8, "followers": 9000, "following": 9,
    "created_at": "2011-01-25T18:44:36Z", "updated_at": "2026-01-01T00:00:00Z"
}`;
const PROFILE_KEYS = "accountType,bio,blog,company,createdAt,followers,following,hasPublicEmail,location,login,name,publicRepos,twitterUsername,url";

function main(): void {
    const repository: RepositoryRef = { owner: "octocat", name: "Hello-World" };

    label("public inputs are compact and typed");
    const changes: CommitFileChange[] = [
        { type: "write", path: "notes/plan.md", content: "# Plan" },
        { type: "delete", path: "notes/old.md" },
    ];
    assert(changes.length === 2, "commit changes are represented");
    const update: UpdateIssueInput = { state: "closed", stateReason: "completed" };
    assert(update.state === "closed", "issue state is typed");

    label("validation errors happen before credentials or network access");
    assertError("invalid_input", () => getBranch({ owner: "", name: "repo" }, "main"));
    assertError("invalid_branch", () => createBranch(repository, { name: "refs/heads/main", fromSha: "a".repeat(40) }));
    assertError("invalid_path", () => readFile(repository, "../secret"));
    assertError("unsafe_search_query", () => searchCode(repository, "token OR repo:other/repo"));
    assertError("invalid_number", () => updateIssue(repository, 0, update));
    assertError("invalid_input", () => createIssue(repository, { title: " " }));
    assertError("invalid_timestamp", () => listCommits(repository, { since: "2026-08-11" }));
    assertError("invalid_timestamp", () => listCommits(repository, { until: "not a timestamp" }));

    label("an owner and a repository name are each exactly one name");
    for (const name of ["a b", "x repo:other/repo", "other/repo", ".", "..", "a\rb", "a\nb", "a\tb", "a%2Fb", "na\u00efve"]) {
        assertError("invalid_input", () => getBranch({ owner: "pied-piper", name: name }, "main"));
        assertError("invalid_input", () => searchCode({ owner: "pied-piper", name: name }, "needle"));
    }
    for (const owner of ["a b", "pied-piper/other", ".", "..", "pied-piper.inc", "pied-piper\r", "repo:pied-piper"]) {
        assertError("invalid_input", () => getBranch({ owner: owner, name: "repo" }, "main"));
        assertError("invalid_input", () => searchCode({ owner: owner, name: "repo" }, "needle"));
    }

    label("a path segment cannot be . or ..");
    for (const dots of [".", ".."]) {
        assertError("invalid_input", () => getBranch(repository, dots));
        assertError("invalid_input", () => getCommit(repository, dots));
        assertError("invalid_input", () => getTree(repository, dots));
    }

    label("a search query is split on every kind of whitespace");
    for (const separator of [" ", "\t", "\n", "\r", "\u000b", "\u000c", "\u0085", "\u00a0", "\u2003", "\u2028", "\u3000", "\ufeff"]) {
        assertError("unsafe_search_query", () => searchCode(repository, "needle" + separator + "repo:other/repo"));
        assertError("unsafe_search_query", () => searchCode(repository, "needle" + separator + "OR" + separator + "other"));
    }

    label("omitted options use the defaults and a bad page option is refused");
    assertError("invalid_page_size", () => listBranches(repository, { limit: 0 }));
    assertError("invalid_page_token", () => listBranches(repository, { pageToken: "next" }));
    assertError("invalid_page_size", () => listIssues(repository, { limit: 101 }));
    if (secrets.get("GITHUB_TOKEN") === undefined) {
        label("an unbound GITHUB_TOKEN is refused before any request");
        assertError("missing_token", () => getViewer());
        assertError("missing_token", () => listBranches(repository));
        assertError("missing_token", () => listBranches(repository, undefined));
        assertError("missing_token", () => getCommit(repository, "main"));
        assertError("missing_token", () => readFile(repository, "README.md"));
    }

    label("a login is exactly one GitHub account name");
    const craftedLogins = [
        "", " ", ".", "..", "/", "%", "?", "#", "a/b", "../octocat", "octocat/../hubot", "octocat/", "%2e%2e", "octocat%2fhubot",
        "octocat?per_page=1", "octocat#fragment", "octo cat", "octocat\n", "octocat\r", "\toctocat", "-octocat", "_octocat",
        "octo.cat", "[bot]", "octocat[bot]x", "octocat[BOT]", "octo[bot]cat", "na\u00efve", "\u212Aelvin", "a".repeat(40), "a".repeat(40) + "[bot]",
    ];
    for (const login of craftedLogins) {
        assertError("invalid_input", () => getUser(login));
        assertError("invalid_input", () => listUserOrganizations(login));
        assertError("invalid_input", () => listUserSocialAccounts(login));
    }

    label("repository audience reads hold the owner and name to the repository grammar");
    for (const owner of ["", "a b", "allowed-org/x", "..", "allowed-org\r"]) {
        assertError("invalid_input", () => listRepositoryEvents(owner, "repo"));
        assertError("invalid_input", () => listStargazers(owner, "repo"));
        assertError("invalid_input", () => listForks(owner, "repo"));
    }
    for (const name of ["", "a b", "other/repo", "..", "%2E%2E"]) {
        assertError("invalid_input", () => listRepositoryEvents("octocat", name));
        assertError("invalid_input", () => listStargazers("octocat", name));
        assertError("invalid_input", () => listForks("octocat", name));
    }
    assertError("invalid_page_size", () => listRepositoryEvents("octocat", "repo", { limit: 0 }));
    assertError("invalid_page_token", () => listStargazers("octocat", "repo", { pageToken: "next" }));
    assertError("invalid_page_size", () => listForks("octocat", "repo", { limit: 101 }));
    assertError("invalid_page_size", () => listUserOrganizations("octocat", { limit: 101 }));
    assertError("invalid_page_token", () => listUserSocialAccounts("octocat", { pageToken: "next" }));
    if (secrets.get("GITHUB_TOKEN") === undefined) {
        label("every valid login shape gets past validation to the unbound token");
        const accepted = [
            "octocat", "OctoCat", "a", "0", "a-b-c", "a".repeat(39), "dependabot[bot]", "a".repeat(39) + "[bot]",
            // Legacy logins with a trailing or doubled hyphen, and a managed user's underscore.
            "octocat-", "octo--cat", "octocat--", "octocat_acme", "Octo_Cat", "a" + "_".repeat(38), "octocat-[bot]",
        ];
        for (const login of accepted) {
            assertError("missing_token", () => getUser(login));
            assertError("missing_token", () => listUserOrganizations(login));
            assertError("missing_token", () => listUserSocialAccounts(login));
        }
    }

    label("a profile reports that a public email exists and never the address");
    const profile = userProfileFrom(JSON.parse(PROFILE_JSON) as ApiUserProfile);
    assert(profile.hasPublicEmail === true, "a public email is reported");
    assert(Object.keys(profile).sort().join(",") === PROFILE_KEYS, "only the curated keys are returned: " + Object.keys(profile).sort().join(","));
    assert(!JSON.stringify(profile).includes("example.invalid"), "the address is in no value");
    assert(!JSON.stringify(profile).toLowerCase().includes("\"email\""), "no key is named email");
    assert(profile.login === "octocat" && profile.name === "The Octocat" && profile.company === "@github", "identity is mapped");
    assert(profile.blog === "https://github.blog" && profile.bio === "A mascot" && profile.location === "San Francisco", "free text is mapped");
    assert(profile.twitterUsername === "octocat", "twitter_username is mapped");
    assert(profile.url === "https://github.com/octocat", "url is the web profile, not the API URL");
    assert(profile.accountType === "User" && profile.createdAt === "2011-01-25T18:44:36Z", "type and creation time are mapped");
    assert(profile.followers === 9000 && profile.following === 9 && profile.publicRepos === 8, "counts are mapped");

    label("nullable profile strings become empty and a null or empty email is no public email");
    const sparse = userProfileFrom(JSON.parse(`{ "login": "octocat", "name": null, "company": null, "blog": null, "location": null,
        "email": null, "bio": null, "twitter_username": null }`) as ApiUserProfile);
    assert(sparse.name === "" && sparse.company === "" && sparse.blog === "" && sparse.location === "", "null strings are empty");
    assert(sparse.bio === "" && sparse.twitterUsername === "" && sparse.url === "", "null and absent strings are empty");
    assert(sparse.followers === 0 && sparse.publicRepos === 0, "absent counts are zero");
    assert(sparse.hasPublicEmail === false, "null email");
    assert(userProfileFrom(JSON.parse(`{ "email": "" }`) as ApiUserProfile).hasPublicEmail === false, "empty email");
    assert(userProfileFrom(JSON.parse(`{}`) as ApiUserProfile).hasPublicEmail === false, "absent email");

    label("events keep every type, with the action and fork name only where they apply");
    const events = JSON.parse(`[
        { "id": "22249084947", "type": "WatchEvent", "actor": { "id": 1, "login": "octocat" }, "created_at": "2026-10-01T00:00:00Z", "payload": { "action": "started" } },
        { "id": "22249084948", "type": "ForkEvent", "actor": { "login": "hubot" }, "created_at": "2026-10-02T00:00:00Z", "payload": { "action": "forked", "forkee": { "full_name": "hubot/Hello-World" } } },
        { "id": 22249084949, "type": "PushEvent", "actor": { "login": "octocat" }, "created_at": "2026-10-03T00:00:00Z", "payload": { "ref": "refs/heads/main", "size": 1 } },
        { "id": "22249084950", "type": "PublicEvent", "payload": {} },
        { "id": null, "type": "IssuesEvent", "actor": null, "payload": { "action": null, "forkee": { "full_name": "not/a-fork" } } },
        { "type": "ForkEvent", "payload": null }
    ]`) as ApiEvent[];
    const star = repositoryEventFrom(events[0]);
    assert(star.id === "22249084947" && star.type === "WatchEvent" && star.actorLogin === "octocat", "a star is mapped");
    assert(star.createdAt === "2026-10-01T00:00:00Z" && star.action === "started" && star.forkFullName === "", "a star has an action and no fork");
    const fork = repositoryEventFrom(events[1]);
    assert(fork.action === "forked" && fork.forkFullName === "hubot/Hello-World", "a fork names its repository");
    const push = repositoryEventFrom(events[2]);
    assert(push.id === "22249084949" && push.type === "PushEvent" && push.action === "" && push.forkFullName === "", "other types are kept, a numeric id is text");
    const bare = repositoryEventFrom(events[3]);
    assert(bare.actorLogin === "" && bare.createdAt === "" && bare.action === "", "an empty payload and absent actor are empty");
    const other = repositoryEventFrom(events[4]);
    assert(other.id === "" && other.action === "" && other.forkFullName === "", "null fields are empty and forkee counts only on a ForkEvent");
    assert(repositoryEventFrom(events[5]).forkFullName === "", "a ForkEvent without a payload names no fork");

    label("stargazers read the star media type, including a star with no account");
    const stars = JSON.parse(`[
        { "starred_at": "2026-10-01T00:00:00Z", "user": { "login": "octocat", "type": "User" } },
        { "starred_at": "2026-10-02T00:00:00Z", "user": null },
        { "starred_at": null }
    ]`) as ApiStargazer[];
    const stargazer = stargazerFrom(stars[0]);
    assert(stargazer.login === "octocat" && stargazer.starredAt === "2026-10-01T00:00:00Z" && stargazer.accountType === "User", "a stargazer is mapped");
    const unlinked = stargazerFrom(stars[1]);
    assert(unlinked.login === "" && unlinked.accountType === "" && unlinked.starredAt === "2026-10-02T00:00:00Z", "a null user keeps the star");
    assert(stargazerFrom(stars[2]).starredAt === "", "a null time is empty");

    label("forks, organizations and social accounts map their fields and null values");
    const forks = JSON.parse(`[
        { "full_name": "hubot/Hello-World", "html_url": "https://github.com/hubot/Hello-World", "url": "https://api.github.com/repos/hubot/Hello-World",
          "created_at": "2026-10-02T00:00:00Z", "owner": { "login": "hubot" } },
        { "full_name": null, "owner": null }
    ]`) as ApiFork[];
    const mappedFork = repositoryForkFrom(forks[0]);
    assert(mappedFork.fullName === "hubot/Hello-World" && mappedFork.ownerLogin === "hubot", "a fork is mapped");
    assert(mappedFork.createdAt === "2026-10-02T00:00:00Z" && mappedFork.url === "https://github.com/hubot/Hello-World", "url is the web URL");
    const emptyFork = repositoryForkFrom(forks[1]);
    assert(emptyFork.fullName === "" && emptyFork.ownerLogin === "" && emptyFork.url === "" && emptyFork.createdAt === "", "null fork fields are empty");
    const organizations = JSON.parse(`[{ "login": "github", "url": "https://api.github.com/orgs/github", "description": null }, { "login": null }]`) as ApiUserOrganization[];
    const organization = userOrganizationFrom(organizations[0]);
    assert(organization.login === "github" && organization.url === "https://github.com/github", "organization url is the web URL");
    const noOrganization = userOrganizationFrom(organizations[1]);
    assert(noOrganization.login === "" && noOrganization.url === "", "a null login has no URL");
    const socials = JSON.parse(`[{ "provider": "twitter", "url": "https://twitter.com/octocat" }, { "provider": null, "url": null }]`) as ApiUserSocialAccount[];
    const social = userSocialAccountFrom(socials[0]);
    assert(social.provider === "twitter" && social.url === "https://twitter.com/octocat", "a social account is mapped");
    const noSocial = userSocialAccountFrom(socials[1]);
    assert(noSocial.provider === "" && noSocial.url === "", "null social fields are empty");

    label("GitHubError preserves operational metadata");
    const error = new GitHubError("rate_limited", "slow down", 429, "request-1", "docs", 2, 0, "1234", ["quota"]);
    assert(error.code === "rate_limited", "code is preserved");
    assert(error.requestId === "request-1", "request ID is preserved");
    assert(error.retryAfterSeconds === 2, "retry delay is preserved");
    assert(error.rateLimitRemaining === 0, "rate remaining is preserved");
}

function assertError(code: string, action: () => unknown): void {
    let matched = false;
    try {
        action();
    } catch (cause) {
        if (cause instanceof GitHubError) matched = cause.code === code && cause.status === 0;
    }
    assert(matched, "expected GitHubError " + code);
}
