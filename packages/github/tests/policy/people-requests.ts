// Run through `submilli run --blueprint people-requests.yaml`, not the allow-all build test runner.
import {
    getUser, listForks, listRepositoryEvents, listStargazers, listUserOrganizations, listUserSocialAccounts,
} from "@submilli/github";

// The reason of the blueprint's `ask-human` rule, which matches only the exact paths it lists.
const APPROVED_REQUEST = "@submilli/github: policy requires human approval for http.get (caller @submilli/github); "
    + "ask-human is deferred and treated as deny";

function assertApproved(what: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + ": " + error.reason;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === APPROVED_REQUEST, what + ": expected the approved request, got " + outcome);
}

function main(): string {
    // Mixed case: the request carries the lowercased values the policy saw.
    assertApproved("events", () => { listRepositoryEvents("Allowed-Org", "Repo.js", { limit: 2 }); });
    assertApproved("stargazers", () => { listStargazers("Allowed-Org", "Repo.js", { limit: 2 }); });
    assertApproved("forks", () => { listForks("Allowed-Org", "Repo.js", { limit: 2 }); });
    assertApproved("profile", () => { getUser("OctoCat"); });
    assertApproved("organizations", () => { listUserOrganizations("OctoCat", { limit: 2 }); });
    assertApproved("social accounts", () => { listUserSocialAccounts("OctoCat", { limit: 2 }); });
    assertApproved("bot profile", () => { getUser("Dependabot[bot]"); });
    return "people request checks passed";
}
