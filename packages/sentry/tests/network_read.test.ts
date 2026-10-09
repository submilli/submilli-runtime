import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { getIssue, getIssueEvent, listIssueEvents, listIssues, listOrganizations, listProjects } from "@submilli/sentry";

function main(): void {
    if (secrets.get("SENTRY_AUTH_TOKEN") === undefined) {
        label("skip: SENTRY_AUTH_TOKEN is not bound");
        return;
    }
    const organization = secrets.get("SENTRY_TEST_ORGANIZATION");
    if (organization === undefined) {
        label("skip: SENTRY_TEST_ORGANIZATION is not bound");
        return;
    }

    label("list organizations and projects");
    const organizations = listOrganizations({ limit: 10 });
    assert(organizations.items.length > 0, "token can see at least one organization");
    const projects = listProjects(organization, { limit: 10 });
    assert(projects.items.length >= 0, "project list succeeds");

    label("list unresolved issues and inspect one event");
    const issues = listIssues(organization, { limit: 2 });
    if (issues.items.length === 0) return;
    const issue = issues.items[0];
    const fetched = getIssue(organization, issue.project.slug, issue.shortId.length > 0 ? issue.shortId : issue.id);
    assert(fetched !== null && fetched.id === issue.id, "issue resolves by public reference");
    const events = listIssueEvents(organization, issue.project.slug, issue.id, { limit: 1 });
    if (events.items.length > 0) {
        const event = getIssueEvent(organization, issue.project.slug, issue.id, events.items[0].eventId);
        assert(event !== null, "listed event can be fetched");
    }
}
