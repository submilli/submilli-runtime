// Run through `submilli run --blueprint repository-search-open.yaml`, not the allow-all build test runner.
import { searchRepositories } from "@submilli/github";

function main(): string {
    for (const query of ["org:submilli is:public", "language:rust stars:>100"]) {
        let outcome = "no error";
        try {
            searchRepositories(query);
        } catch (error: PermissionDeniedError) {
            outcome = error.caller + " denied at " + error.capability;
        }
        // This denial proves the business check passed, without using a token or HTTP.
        assert(outcome === "@submilli/github denied at secrets.get", query + ": got " + outcome);
    }
    return "unfiltered repository search checks passed";
}
