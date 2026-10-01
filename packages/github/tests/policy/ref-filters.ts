// Run through `submilli run --blueprint ref-filters.yaml`, not the allow-all build test runner.
import {
    CreatePullRequestInput, FileReadOptions, RepositoryRef, createPullRequest, listDirectory, readFile, readTextFile, updatePullRequest,
} from "@submilli/github";

const REPOSITORY: RepositoryRef = { owner: "allowed-org", name: "repo" };

// `main` controls this object while the package reads it. `base` answers "main" to its first
// read and "release" to every later one, so a package that reads it for `check` and again for
// the request approves one base branch and opens the pull request against another.
class FlippingPullRequest implements CreatePullRequestInput {
    title: string = "Change";
    head: string = "feature";
    private baseReadCount: number = 0;

    get base(): string {
        this.baseReadCount += 1;
        return this.baseReadCount === 1 ? "main" : "release";
    }

    // `CreatePullRequestInput` fields are writable, which a getter alone does not satisfy.
    set base(value: string) {}

    baseReads(): number {
        return this.baseReadCount;
    }
}

// `ref` answers "main" to its first read and "secret-branch" to every later one.
class FlippingReadOptions implements FileReadOptions {
    private refReadCount: number = 0;

    get ref(): string {
        this.refReadCount += 1;
        return this.refReadCount === 1 ? "main" : "secret-branch";
    }

    set ref(value: string) {}

    refReads(): number {
        return this.refReadCount;
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

function main(): string {
    // The contents rules allow the `main` ref only. A call that names no ref reads the default
    // branch, which the check reports as null, and the rules do not allow it.
    reachesCredentialBoundary(() => { readFile(REPOSITORY, "README.md", { ref: "main" }); });
    deniedAt("github.com/contents.readFile", "main", () => { readFile(REPOSITORY, "README.md", { ref: "secret-branch" }); });
    deniedAt("github.com/contents.readFile", "main", () => { readFile(REPOSITORY, "README.md"); });
    reachesCredentialBoundary(() => { readTextFile(REPOSITORY, "README.md", { ref: "main" }); });
    deniedAt("github.com/contents.readTextFile", "main", () => { readTextFile(REPOSITORY, "README.md", { ref: "secret-branch" }); });
    deniedAt("github.com/contents.readTextFile", "main", () => { readTextFile(REPOSITORY, "README.md"); });
    reachesCredentialBoundary(() => { listDirectory(REPOSITORY, "docs", { ref: "main" }); });
    deniedAt("github.com/contents.listDirectory", "main", () => { listDirectory(REPOSITORY, "docs", { ref: "secret-branch" }); });
    deniedAt("github.com/contents.listDirectory", "main", () => { listDirectory(REPOSITORY, "docs"); });
    // An empty ref is not sent, so it is the default branch too.
    deniedAt("github.com/contents.readFile", "main", () => { readFile(REPOSITORY, "README.md", { ref: "" }); });
    deniedAt("github.com/contents.readTextFile", "main", () => { readTextFile(REPOSITORY, "README.md", { ref: "" }); });
    deniedAt("github.com/contents.listDirectory", "main", () => { listDirectory(REPOSITORY, "docs", { ref: "" }); });

    const flippingRef = new FlippingReadOptions();
    reachesCredentialBoundary(() => { readFile(REPOSITORY, "README.md", flippingRef); });
    assert(flippingRef.refReads() === 1, "readFile read ref " + flippingRef.refReads().toString() + " times, expected 1");

    reachesCredentialBoundary(() => { createPullRequest(REPOSITORY, { title: "Change", head: "feature", base: "main" }); });
    deniedAt("github.com/pulls.create", "main", () => { createPullRequest(REPOSITORY, { title: "Change", head: "feature", base: "release" }); });
    deniedAt("github.com/pulls.create", "main", () => { createPullRequest(REPOSITORY, { title: "Change", head: "other-user:feature", base: "main" }); });
    const flippingBase = new FlippingPullRequest();
    reachesCredentialBoundary(() => { createPullRequest(REPOSITORY, flippingBase); });
    assert(flippingBase.baseReads() === 1, "createPullRequest read base " + flippingBase.baseReads().toString() + " times, expected 1");

    // An update that leaves the base alone reports a null base; one that retargets reports the new base.
    reachesCredentialBoundary(() => { updatePullRequest(REPOSITORY, 5, { title: "Renamed" }); });
    reachesCredentialBoundary(() => { updatePullRequest(REPOSITORY, 5, { base: "main" }); });
    deniedAt("github.com/pulls.update", "main", () => { updatePullRequest(REPOSITORY, 5, { base: "release" }); });
    return "ref, head and base checks passed";
}
