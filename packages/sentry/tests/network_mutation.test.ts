import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { IssuePriority, getIssue, updateIssue } from "@submilli/sentry";

function main(): void {
    if (secrets.get("SENTRY_AUTH_TOKEN") === null || secrets.get("SENTRY_LIVE_MUTATIONS") !== "true") {
        label("skip: Sentry live mutations are disabled");
        return;
    }
    const organization = secrets.get("SENTRY_TEST_ORGANIZATION");
    const project = secrets.get("SENTRY_TEST_PROJECT");
    const issueId = secrets.get("SENTRY_TEST_ISSUE_ID");
    if (organization === null || project === null || issueId === null) {
        label("skip: SENTRY_TEST_ORGANIZATION, SENTRY_TEST_PROJECT, or SENTRY_TEST_ISSUE_ID is not bound");
        return;
    }
    const issue = getIssue(organization, project, issueId);
    assert(issue !== null, "disposable test issue exists");
    if (issue === null) return;
    if (issue.priority !== "low" && issue.priority !== "medium" && issue.priority !== "high") {
        label("skip: disposable issue has an unsupported priority");
        return;
    }
    let original: IssuePriority = "medium";
    if (issue.priority === "low") original = "low";
    if (issue.priority === "high") original = "high";
    const temporary: IssuePriority = original === "high" ? "low" : "high";
    try {
        const changed = updateIssue(organization, project, issue.id, { priority: temporary });
        assert(changed.priority === temporary, "priority changed");
    } finally {
        const restored = updateIssue(organization, project, issue.id, { priority: original });
        assert(restored.priority === original, "priority restored");
    }
}
