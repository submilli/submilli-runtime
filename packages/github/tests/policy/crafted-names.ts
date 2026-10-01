// Run through `submilli run --blueprint crafted-names.yaml`, not the allow-all build test runner.
import {
    GitHubError, RepositoryRef, getBranch, getCommit, getPullRequest, getRepository, getTree, listTeamMembers, readFile,
    searchCode, searchIssues, searchPullRequests,
} from "@submilli/github";

const ALLOWED: RepositoryRef = { owner: "allowed-org", name: "repo.js" };

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

function reachesCredentialBoundary(what: string, action: () => void): void {
    // This denial proves the business check passed, without using a token or HTTP.
    const outcome = outcomeOf(action);
    assert(outcome === "@submilli/github denied at secrets.get", what + ": expected the credential boundary, got " + outcome);
}

function rejectedAs(code: string, what: string, action: () => void): void {
    const outcome = outcomeOf(action);
    assert(outcome === code, what + ": expected " + code + ", got " + outcome);
}

// Every operation that names a repository validates it the same way, before its check.
function repositoryOperations(repository: RepositoryRef): (() => void)[] {
    return [
        (): void => { getRepository(repository); },
        (): void => { getPullRequest(repository, 5); },
        (): void => { readFile(repository, "README.md"); },
        (): void => { searchCode(repository, "needle"); },
        (): void => { searchIssues(repository, "bug"); },
        (): void => { searchPullRequests(repository, "fix"); },
    ];
}

function main(): string {
    for (const operation of repositoryOperations(ALLOWED)) reachesCredentialBoundary("allowed repository", operation);
    // Every character GitHub allows in a name is accepted.
    for (const operation of repositoryOperations({ owner: "allowed-org", name: ".github_v2-final.js" })) reachesCredentialBoundary("name with allowed punctuation", operation);

    // The owner is the allowed one in each of these, so an `owner` filter alone lets them past.
    // The name would add a second `repo:` qualifier to a search, or step out of the owner's path.
    const craftedNames: string[] = [
        "x repo:victim-org/private", "x\trepo:victim-org/private", "x\rrepo:victim-org/private", "x\nrepo:victim-org/private",
        "repo:victim-org/private", "x/../../victim-org/private", "victim-org/private", ".", "..", "%2E%2E", "a b", "",
    ];
    for (const name of craftedNames) {
        for (const operation of repositoryOperations({ owner: "allowed-org", name: name })) rejectedAs("invalid_input", "name " + JSON.stringify(name), operation);
    }
    const craftedOwners: string[] = ["allowed-org/x", "allowed-org x", "..", ".", "allowed-org\r", "allowed.org", ""];
    for (const owner of craftedOwners) {
        for (const operation of repositoryOperations({ owner: owner, name: "repo.js" })) rejectedAs("invalid_input", "owner " + JSON.stringify(owner), operation);
    }

    // A path segment of `.` or `..` would send the request to another path of the API.
    for (const dots of [".", ".."]) {
        rejectedAs("invalid_input", "branch " + dots, () => { getBranch(ALLOWED, dots); });
        rejectedAs("invalid_input", "commit ref " + dots, () => { getCommit(ALLOWED, dots); });
        rejectedAs("invalid_input", "tree SHA " + dots, () => { getTree(ALLOWED, dots); });
        rejectedAs("invalid_input", "organization " + dots, () => { listTeamMembers(dots, "team"); });
        rejectedAs("invalid_input", "team slug " + dots, () => { listTeamMembers("allowed-org", dots); });
    }
    reachesCredentialBoundary("branch with dots inside", () => { getBranch(ALLOWED, "release/v1.2..3"); });

    // A scope qualifier in the query is refused whatever whitespace separates it from the rest.
    for (const separator of [" ", "\t", "\n", "\r", "\u000b", "\u000c", "\u0085", "\u00a0", "\u1680", "\u2003", "\u2028", "\u2029", "\u202f", "\u205f", "\u3000", "\ufeff", "\u0000", "\u001c", "\u001f", "\u007f", "\u180e", "\u200b", "\u2060"]) {
        const query = "needle" + separator + "repo:victim-org/private";
        const what = "separator U+" + separator.charCodeAt(0).toString(16);
        rejectedAs("unsafe_search_query", what, () => { searchCode(ALLOWED, query); });
        rejectedAs("unsafe_search_query", what, () => { searchIssues(ALLOWED, query); });
    }
    return "crafted name checks passed";
}
