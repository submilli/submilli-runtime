// Opt-in writes, independent of app OAuth: personal keys can test comment APIs.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { createComment, listComments } from "@submilli/linear";

function main(): void {
    if (secrets.get("LINEAR_LIVE_MUTATIONS") !== "true") {
        label("skip: Linear live mutations are disabled");
        return;
    }
    const issueId = secrets.get("LINEAR_TEST_ISSUE_ID");
    assert(issueId !== undefined, "bind LINEAR_TEST_ISSUE_ID");
    assert(secrets.get("LINEAR_API_KEY") !== undefined, "bind LINEAR_API_KEY");
    if (issueId === undefined) return;

    label("create a comment and threaded reply without changing the issue");
    const root = createComment({ issueId: issueId, body: "Submilli comment API smoke test. This test does not change the issue status." });
    const reply = createComment({ issueId: issueId, parentId: root.id, body: "Threaded reply smoke test." });
    assert(reply.parent?.id === root.id, "reply belongs to the correct thread");
    const comments = listComments(issueId, { first: 10 });
    assert(comments.nodes.length > 0, "comments can be listed");
}
