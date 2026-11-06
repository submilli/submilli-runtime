// Run through `submilli run --blueprint repository-search-query.yaml`, not the allow-all build test runner.
import { GitHubError, searchRepositories } from "@submilli/github";

const PINNED = "org:submilli is:public";

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

function assertOutcome(expected: string, query: string, action: () => void): void {
    const outcome = outcomeOf(action);
    assert(outcome === expected, JSON.stringify(query) + ": expected " + expected + ", got " + outcome);
}

function main(): string {
    // This denial proves the business check passed, without using a token or HTTP.
    const allowed = "@submilli/github denied at secrets.get";
    assertOutcome(allowed, PINNED, () => { searchRepositories(PINNED); });
    assertOutcome(allowed, PINNED, () => { searchRepositories(PINNED, { sort: "stars", limit: 5 }); });

    const denied = "main denied at github.com/repositories.search";
    const others = [
        "org:victim-org is:private", "org:submilli is:private", "org:submilli", "org:submilli is:public org:victim-org",
        // The filter sees the text as sent, so a spelling that GitHub may read the same is another query.
        " org:submilli is:public", "org:submilli is:public ", "org:submilli  is:public", "ORG:submilli is:public",
    ];
    for (const query of others) assertOutcome(denied, query, () => { searchRepositories(query); });

    // A blank query is never sent, so it is refused before the check.
    assertOutcome("invalid_input", " ", () => { searchRepositories(" "); });
    return "repository search query checks passed";
}
