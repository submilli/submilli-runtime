// Run through `submilli run --blueprint owner-filter.yaml`, not the allow-all build test runner.
import { RepositoryRef, getPullRequest, getRepository, listBranches, readFile, searchIssues } from "@submilli/github";

// `main` controls this object while the package reads it. `owner` answers
// "allowed-org" to its first read and "victim-org" to every later one, so a
// package that reads it for `check` and again for the request approves one
// repository and calls another.
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
        return "repo";
    }

    set name(value: string) {}

    ownerReads(): number {
        return this.ownerReadCount;
    }

    nameReads(): number {
        return this.nameReadCount;
    }
}

function deniedAt(capability: string, caller: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + " denied at " + error.capability;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    const expected = caller + " denied at " + capability;
    assert(outcome === expected, "expected " + expected + ", got " + outcome);
}

function reachesCredentialBoundary(action: () => void): void {
    // This denial proves the business check passed, without using a token or HTTP.
    deniedAt("secrets.get", "@submilli/github", action);
}

function ownerFilterHolds(capability: string, call: (repository: RepositoryRef) => void): void {
    deniedAt(capability, "main", () => { call({ owner: "victim-org", name: "repo" }); });
    deniedAt(capability, "main", () => { call({ owner: "Victim-Org", name: "REPO" }); });
    reachesCredentialBoundary(() => { call({ owner: "ALLOWED-ORG", name: "Repo" }); });
    reachesCredentialBoundary(() => { call({ owner: "allowed-org", name: "repo" }); });

    const flipping = new FlippingRepository();
    reachesCredentialBoundary(() => { call(flipping); });
    assert(flipping.ownerReads() === 1, capability + " read owner " + flipping.ownerReads().toString() + " times, expected 1");
    assert(flipping.nameReads() === 1, capability + " read name " + flipping.nameReads().toString() + " times, expected 1");
    // The request is built before the credential boundary and never sent, so its
    // path cannot be inspected. One read is the proof instead: the package holds a
    // single owner, `check` approved it, and any further read answers "victim-org".
    const secondRead = flipping.owner;
    assert(secondRead === "victim-org", "second read of owner answered " + secondRead + ", expected victim-org");
}

function main(): string {
    // Each operation reaches the repository through a different private helper chain.
    ownerFilterHolds("github.com/repositories.get", (repository: RepositoryRef): void => { getRepository(repository); });
    ownerFilterHolds("github.com/branches.list", (repository: RepositoryRef): void => { listBranches(repository, { limit: 2 }); });
    ownerFilterHolds("github.com/issues.search", (repository: RepositoryRef): void => { searchIssues(repository, "bug"); });
    ownerFilterHolds("github.com/pulls.get", (repository: RepositoryRef): void => { getPullRequest(repository, 1); });
    ownerFilterHolds("github.com/contents.readFile", (repository: RepositoryRef): void => { readFile(repository, "README.md"); });
    return "owner filter checks passed";
}
