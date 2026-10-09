// Real network integration for read operations, gated on a token: Linear has no
// anonymous tier, so this runs only when LINEAR_API_KEY is available (build test
// bridges env vars / a .env to secrets.get). Without it, the test skips + passes.
// Reads only — writes are covered by the pure unit tests in lib.test.ts.

import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { getViewer, listTeams, getTeam, getIssue, listIssues } from "@submilli/linear";

function main(): void {
    if (secrets.get("LINEAR_API_KEY") === undefined) {
        return;
    }

    label("viewer fetch returns the token owner");
    const me = getViewer();
    assert(me.id.length > 0, "viewer has an id");
    assert(me.name.length > 0, "viewer has a name");

    label("list teams");
    const teams = listTeams({ first: 10 });
    assert(teams.nodes.length > 0, "workspace has at least one team");

    label("get a team by id");
    const firstTeam = teams.nodes[0];
    const team = getTeam(firstTeam.id);
    assert(team !== null, "team fetched by id");
    if (team !== null) {
        assert(team.id === firstTeam.id, "fetched team matches");
        assert(team.key.length > 0, "team has a key");
    }

    label("list issues for that team");
    const issues = listIssues({ teamId: firstTeam.id }, { first: 5 });
    if (issues.nodes.length > 0) {
        const issue = issues.nodes[0];
        assert(issue.identifier.length > 0, "issue has an identifier");
        assert(issue.createdAt.length > 0, "issue has createdAt");
        assert(issue.updatedAt.length > 0, "issue has updatedAt");
        assert(issue.priorityLabel.length > 0, "issue has priorityLabel");
        assert(issue.team !== null, "issue carries its embedded team");

        label("get issue by id");
        const fetched = getIssue(issue.id);
        assert(fetched !== null, "issue fetched by id");
        if (fetched !== null) {
            assert(fetched.id === issue.id, "fetched issue matches");
            assert(fetched.createdAt.length > 0, "fetched issue has createdAt");
            assert(fetched.updatedAt.length > 0, "fetched issue has updatedAt");
            assert(fetched.priorityLabel.length > 0, "fetched issue has priorityLabel");
        }
    }

    label("pagination: a second page fetched with the returned cursor succeeds");
    const page1 = listIssues({ teamId: firstTeam.id }, { first: 1 });
    if (page1.pageInfo.hasNextPage) {
        const cursor = page1.pageInfo.endCursor;
        assert(cursor !== null, "a next page advertises a cursor");
        if (cursor !== null) {
            // A cursor-only page used to send `{ after }` with no `first`, which
            // Linear rejects; a default `first` now rides along so page 2 loads.
            const page2 = listIssues({ teamId: firstTeam.id }, { after: cursor });
            assert(page2.nodes.length > 0, "second page returns issues");
            if (page1.nodes.length > 0) {
                assert(page2.nodes[0].id !== page1.nodes[0].id, "second page differs from the first");
            }
        }
    }

    label("list completed issues by completedAt");
    const completed = listIssues(
        { stateType: "completed", completedAtAfter: "1970-01-01T00:00:00.000Z" },
        { first: 1 },
    );
    if (completed.nodes.length > 0) {
        assert(completed.nodes[0].completedAt !== null, "completed issue has completedAt");
    }

    label("a bracketed Temporal zone annotation in a date filter is accepted");
    // Temporal.Now.zonedDateTimeISO(...).toString() produces this form; Linear
    // rejects it with a 400 unless the annotation is stripped before sending.
    const bracketed = listIssues(
        {
            stateType: "completed",
            completedAtAfter: "1970-01-01T00:00:00+00:00[UTC]",
            completedAtBefore: "2100-01-01T00:00:00+00:00[Etc/UTC]",
        },
        { first: 1 },
    );
    assert(bracketed.nodes.length === completed.nodes.length, "bracketed range matches the plain one");
}
