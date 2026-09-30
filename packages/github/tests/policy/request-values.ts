// Run through `submilli run --blueprint request-values.yaml`, not the allow-all build test runner.
import { RepositoryRef, getPullRequest, getRepository, listBranches, readFile } from "@submilli/github";

// The reason of the blueprint's `ask-human` rule, which matches a request for allowed-org/repo.js only.
const APPROVED_REQUEST = "@submilli/github: policy requires human approval for http.get (caller @submilli/github); "
    + "ask-human is deferred and treated as deny";

// `owner` answers "allowed-org" to its first read and "victim-org" to every later
// one. The package here holds a token, so it builds the whole request, and the
// blueprint reports which repository that request names.
class FlippingRepository implements RepositoryRef {
    private ownerReadCount: number = 0;
    private nameReadCount: number = 0;

    get owner(): string {
        this.ownerReadCount += 1;
        return this.ownerReadCount === 1 ? "allowed-org" : "victim-org";
    }

    // `RepositoryRef` fields are writable, which a getter alone does not satisfy.
    set owner(value: string) {}

    get name(): string {
        this.nameReadCount += 1;
        return "repo.js";
    }

    set name(value: string) {}

    ownerReads(): number {
        return this.ownerReadCount;
    }

    nameReads(): number {
        return this.nameReadCount;
    }
}

function requestValuesHold(capability: string, call: (repository: RepositoryRef) => void): void {
    const refusedAtMain = "main: policy denied " + capability + " for main";
    assertOutcome(refusedAtMain, () => { call({ owner: "victim-org", name: "repo.js" }); });
    assertOutcome(APPROVED_REQUEST, () => { call({ owner: "allowed-org", name: "repo.js" }); });

    const flipping = new FlippingRepository();
    assertOutcome(APPROVED_REQUEST, () => { call(flipping); });
    assert(flipping.ownerReads() === 1, capability + " read owner " + flipping.ownerReads().toString() + " times, expected 1");
    assert(flipping.nameReads() === 1, capability + " read name " + flipping.nameReads().toString() + " times, expected 1");
}

function assertOutcome(expected: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + ": " + error.reason;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === expected, "expected " + expected + ", got " + outcome);
}

function main(): string {
    // Each operation reaches the repository through a different private helper chain.
    requestValuesHold("github.com/repositories.get", (repository: RepositoryRef): void => { getRepository(repository); });
    requestValuesHold("github.com/branches.list", (repository: RepositoryRef): void => { listBranches(repository, { limit: 2 }); });
    requestValuesHold("github.com/pulls.get", (repository: RepositoryRef): void => { getPullRequest(repository, 1); });
    requestValuesHold("github.com/contents.readFile", (repository: RepositoryRef): void => { readFile(repository, "README.md"); });
    return "request value checks passed";
}
