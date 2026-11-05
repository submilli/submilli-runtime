// Run through `submilli run --blueprint people-scope.yaml`, not the allow-all build test runner.
import {
    GitHubError, getUser, listForks, listRepositoryEvents, listStargazers, listUserOrganizations, listUserSocialAccounts,
} from "@submilli/github";

function outcomeOf(action: () => void): string {
    try {
        action();
    } catch (error: PermissionDeniedError) {
        return error.caller + " denied at " + error.capability;
    } catch (error: GitHubError) {
        return error.code;
    } catch (error) {
        return error.name + ": " + error.message;
    }
    return "no error";
}

function assertOutcome(expected: string, what: string, action: () => void): void {
    const outcome = outcomeOf(action);
    assert(outcome === expected, what + ": expected " + expected + ", got " + outcome);
}

function denied(capability: string, what: string, action: () => void): void {
    assertOutcome("main denied at " + capability, what, action);
}

// This denial proves the business check passed, without using a token or HTTP.
function reachesCredentialBoundary(what: string, action: () => void): void {
    assertOutcome("@submilli/github denied at secrets.get", what, action);
}

function ownerFilterHolds(capability: string, call: (owner: string, repo: string) => void): void {
    denied(capability, "another owner", () => { call("victim-org", "repo"); });
    denied(capability, "another owner in mixed case", () => { call("Victim-Org", "REPO"); });
    denied(capability, "another repository of the allowed owner", () => { call("allowed-org", "other"); });
    reachesCredentialBoundary("allowed repository", () => { call("allowed-org", "repo"); });
    reachesCredentialBoundary("allowed repository in mixed case", () => { call("ALLOWED-ORG", "Repo"); });
    for (const owner of ["allowed-org/../victim-org", "allowed-org/x", "..", ""]) {
        assertOutcome("invalid_input", "owner " + JSON.stringify(owner), () => { call(owner, "repo"); });
    }
    for (const name of ["repo/../../victim-org/private", "repo?x=1", "..", ""]) {
        assertOutcome("invalid_input", "name " + JSON.stringify(name), () => { call("allowed-org", name); });
    }
}

// Each rule here allows `octocat*`, so a crafted value that starts with the allowed name would pass
// it. Only validation before the check keeps these out.
function loginFilterHolds(capability: string, call: (login: string) => void): void {
    denied(capability, "another login", () => { call("hubot"); });
    denied(capability, "another login in mixed case", () => { call("HuBot"); });
    reachesCredentialBoundary("allowed login", () => { call("octocat"); });
    reachesCredentialBoundary("allowed login in mixed case", () => { call("OctoCat"); });
    for (const login of ["octocat/../hubot", "octocat/orgs", "octocat%2f..%2fhubot", "octocat?x=1", "octocat#x", "octocat\n", "octocat.x"]) {
        assertOutcome("invalid_input", "login " + JSON.stringify(login), () => { call(login); });
    }
    // Real logins outside GitHub's sign-up rules: legacy hyphens and a managed user's underscore.
    for (const login of ["octocat-", "octocat--x", "octocat_x", "OctoCat_X"]) {
        reachesCredentialBoundary("login " + login, () => { call(login); });
    }
}

function main(): string {
    ownerFilterHolds("github.com/events.list", (owner: string, repo: string): void => { listRepositoryEvents(owner, repo); });
    ownerFilterHolds("github.com/stargazers.list", (owner: string, repo: string): void => { listStargazers(owner, repo); });
    ownerFilterHolds("github.com/forks.list", (owner: string, repo: string): void => { listForks(owner, repo); });
    loginFilterHolds("github.com/users.get", (login: string): void => { getUser(login); });
    loginFilterHolds("github.com/userOrganizations.list", (login: string): void => { listUserOrganizations(login); });
    loginFilterHolds("github.com/userSocialAccounts.list", (login: string): void => { listUserSocialAccounts(login); });

    // The blueprint names this bot for `users.get` only, so each login grant stands alone.
    reachesCredentialBoundary("bot login", () => { getUser("Dependabot[bot]"); });
    denied("github.com/userOrganizations.list", "bot login without the grant", () => { listUserOrganizations("dependabot[bot]"); });
    denied("github.com/userSocialAccounts.list", "bot login without the grant", () => { listUserSocialAccounts("dependabot[bot]"); });
    return "people scope checks passed";
}
