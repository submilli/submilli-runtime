// Run through `submilli run --blueprint people-requests.yaml`, not the allow-all build test runner.
import {
    getUser, listForks, listRepositoryEvents, listStargazers, listUserOrganizations, listUserSocialAccounts,
} from "@submilli/github";

// The blueprint denies the package every request, so one that passes main's check stops
// at its HTTP call, and nothing is sent. The exact paths are in scripts/contract.test.mjs.
const SENT_REQUEST = "@submilli/github: policy denied http.get for @submilli/github";

function assertSent(what: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + ": " + error.reason;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === SENT_REQUEST, what + ": expected the sent request, got " + outcome);
}

function main(): string {
    // Mixed case and a bot login: each passes main's check and the package builds its request.
    assertSent("events", () => { listRepositoryEvents("Allowed-Org", "Repo.js", { limit: 2 }); });
    assertSent("stargazers", () => { listStargazers("Allowed-Org", "Repo.js", { limit: 2 }); });
    assertSent("forks", () => { listForks("Allowed-Org", "Repo.js", { limit: 2 }); });
    assertSent("profile", () => { getUser("OctoCat"); });
    assertSent("organizations", () => { listUserOrganizations("OctoCat", { limit: 2 }); });
    assertSent("social accounts", () => { listUserSocialAccounts("OctoCat", { limit: 2 }); });
    assertSent("bot profile", () => { getUser("Dependabot[bot]"); });
    return "people request checks passed";
}
