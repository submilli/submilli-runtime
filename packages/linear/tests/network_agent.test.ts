// Explicitly opt-in: creates two completed sessions and a comment thread on a
// disposable issue. These records remain in Linear for inspection.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    createAgentSessionOnIssue, createAgentSessionOnComment, createAgentActivity,
    getAgentSession, updateAgentSession, listAgentActivities, createComment, listComments,
} from "@submilli/linear";

function main(): void {
    if (secrets.get("LINEAR_LIVE_MUTATIONS") !== "true") {
        label("skip: Linear live mutations are disabled");
        return;
    }
    const issueId = secrets.get("LINEAR_TEST_ISSUE_ID");
    assert(secrets.get("LINEAR_API_KEY") !== undefined, "bind an app OAuth token as LINEAR_API_KEY");
    assert(issueId !== undefined, "bind a disposable LINEAR_TEST_ISSUE_ID");
    if (issueId === undefined) return;

    label("create a session on the test issue and acknowledge it");
    const session = createAgentSessionOnIssue({ issueId: issueId });
    try {
        const thought = createAgentActivity({
            agentSessionId: session.id,
            content: { type: "thought", body: "Submilli integration smoke test started." },
            ephemeral: true,
        });
        assert(thought.content.type === "thought", "thought was recorded");
        const active = getAgentSession(session.id);
        assert(active.status === "active", "thought activates the session");
        const updated = updateAgentSession(session.id, {
            plan: [{ content: "Check session operations", status: "completed" }],
            externalUrls: [{ label: "Submilli", url: "https://github.com/submilli/submilli-runtime" }],
        });
        assert(updated.externalUrls.length === 1, "session external link saved");
        assert(updated.plan !== null, "plan saved");
        createAgentActivity({
            agentSessionId: session.id,
            content: { type: "elicitation", body: "Test question; no reply is needed." },
            signal: "select",
            signalMetadata: { options: [{ label: "Continue", value: "continue" }] },
        });
        assert(getAgentSession(session.id).status === "awaitingInput", "elicitation waits for input");
    } finally {
        createAgentActivity({
            agentSessionId: session.id,
            content: { type: "response", body: "Submilli smoke-test session closed; see local test output for results." },
        });
    }
    assert(getAgentSession(session.id).status === "complete", "response completes the session");
    const activities = listAgentActivities(session.id, { first: 20 });
    assert(activities.nodes.length > 0, "activities can be read back");

    label("create a comment thread and a session on its root");
    const root = createComment({ issueId: issueId, body: "Submilli agent API smoke test." });
    const reply = createComment({ issueId: issueId, parentId: root.id, body: "Threaded reply smoke test." });
    assert(reply.parent?.id === root.id, "reply belongs to the correct thread");
    assert(listComments(issueId, { first: 10 }).nodes.length > 0, "comments can be listed");
    const commentSession = createAgentSessionOnComment({ commentId: root.id });
    createAgentActivity({
        agentSessionId: commentSession.id,
        content: { type: "response", body: "Comment-session smoke test complete." },
    });
    assert(getAgentSession(commentSession.id).status === "complete", "comment session completed");
}
