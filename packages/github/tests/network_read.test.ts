import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    RepositoryRef,
    getBranch,
    getRepository,
    getViewer,
    listBranches,
    listCommits,
    listIssues,
    listPullRequests,
    readTextFile,
} from "@submilli/github";

function main(): void {
    if (secrets.get("GITHUB_TOKEN") === null) {
        label("skip: GITHUB_TOKEN is not bound");
        return;
    }
    const configured = secrets.get("GITHUB_TEST_REPOSITORY");
    if (configured === null) {
        label("skip: GITHUB_TEST_REPOSITORY is not bound");
        return;
    }
    const parts = configured.split("/");
    assert(parts.length === 2, "GITHUB_TEST_REPOSITORY must be owner/name");
    const repository: RepositoryRef = { owner: parts[0], name: parts[1] };
    const viewer = getViewer();
    assert(viewer.login.length > 0, "viewer has a login");
    const metadata = getRepository(repository);
    assert(metadata !== null, "test repository exists");
    if (metadata === null) return;
    const branch = getBranch(repository, metadata.defaultBranch);
    assert(branch !== null, "default branch exists");
    assert(listBranches(repository, { limit: 2 }).items.length > 0, "branches can be listed");
    assert(listCommits(repository, { limit: 2 }).items.length > 0, "commits can be listed");
    listIssues(repository, { limit: 2 });
    listPullRequests(repository, { limit: 2 });
    readTextFile(repository, "README.md", { ref: metadata.defaultBranch });
}
