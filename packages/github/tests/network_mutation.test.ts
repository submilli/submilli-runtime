import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    RepositoryRef,
    addIssueComment,
    addPullRequestComment,
    commitFiles,
    createBranch,
    createIssue,
    createPullRequest,
    createPullRequestReview,
    deleteBranch,
    getBranch,
    getRepository,
    readTextFile,
    updateIssue,
    updatePullRequest,
} from "@submilli/github";

function main(): void {
    if (secrets.get("GITHUB_TOKEN") === null || secrets.get("GITHUB_LIVE_MUTATIONS") !== "true") {
        label("skip: GitHub live mutations are disabled");
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
    const metadata = getRepository(repository);
    assert(metadata !== null, "test repository exists");
    if (metadata === null) return;
    const issue = createIssue(repository, { title: "Submilli GitHub package live test", body: "Created by the opt-in live suite." });
    addIssueComment(repository, issue.number, "Comment created by the opt-in live suite.");
    const closed = updateIssue(repository, issue.number, { state: "closed", stateReason: "completed" });
    assert(closed.state === "closed", "live issue was closed");

    const base = getBranch(repository, metadata.defaultBranch);
    assert(base !== null, "default branch exists");
    if (base === null) return;
    const branchName = "submilli-live-" + issue.number.toString();
    createBranch(repository, { name: branchName, fromSha: base.sha });
    try {
        const path = "submilli-live-" + issue.number.toString() + ".txt";
        commitFiles(repository, {
            branch: branchName,
            expectedHeadSha: base.sha,
            message: "Exercise @submilli/github live mutations",
            changes: [{ type: "write", path: path, content: "Submilli GitHub package live test\n" }],
        });
        const file = readTextFile(repository, path, { ref: branchName });
        assert(file !== null && file.text.startsWith("Submilli"), "committed file can be read");
        const pull = createPullRequest(repository, {
            title: "Submilli GitHub package live test",
            head: branchName,
            base: metadata.defaultBranch,
            body: "Created by the opt-in live suite.",
        });
        addPullRequestComment(repository, pull.number, "Comment created by the opt-in live suite.");
        createPullRequestReview(repository, pull.number, { event: "COMMENT", body: "Review created by the opt-in live suite." });
        const closedPull = updatePullRequest(repository, pull.number, { state: "closed" });
        assert(closedPull.state === "closed", "live pull request was closed");
    } finally {
        deleteBranch(repository, branchName);
    }
}
