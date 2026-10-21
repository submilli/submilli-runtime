// Offline tests for the public input and result types — no network, no secret.
// The GraphQL variable builders and error renderers are private, and every
// public operation sends a request, so scripts/contract.test.mjs covers them
// through the public operations against a mocked host.
// Object keys serialize in alphabetical order.

import { label } from "submilli:test";
import {
    IssueCreateInput,
    IssueUpdateInput,
    Page,
    Issue,
} from "@submilli/linear";

function main(): void {
    label("create input omits absent optionals");
    const create: IssueCreateInput = { teamId: "T1", title: "Hello" };
    assert(
        JSON.stringify(create) === "{\"teamId\":\"T1\",\"title\":\"Hello\"}",
        "only provided create fields are sent",
    );

    label("update input sends only changed fields (no destructive nulls)");
    const update: IssueUpdateInput = { priority: 2 };
    assert(
        JSON.stringify(update) === "{\"priority\":2}",
        "absent update fields omitted, not null",
    );

    label("last/empty page parses: endCursor is null when there is no next page");
    const lastPage = JSON.parse(
        "{\"nodes\":[],\"pageInfo\":{\"hasNextPage\":false,\"endCursor\":null}}",
    ) as Page<Issue>;
    assert(lastPage.nodes.length === 0, "empty page has no nodes");
    assert(!lastPage.pageInfo.hasNextPage, "empty page has no next page");
    assert(lastPage.pageInfo.endCursor === null, "endCursor is null on the last page");

    label("an issue in an unnamed cycle parses");
    const page = JSON.parse(
        "{\"nodes\":[{\"id\":\"i\",\"identifier\":\"ENG-1\",\"number\":1,\"title\":\"T\",\"description\":null,\"priority\":0,\"priorityLabel\":\"No priority\",\"url\":\"https://linear.app/x\",\"createdAt\":\"2026-01-01T00:00:00Z\",\"updatedAt\":\"2026-01-01T00:00:00Z\",\"completedAt\":null,\"canceledAt\":null,\"startedAt\":null,\"triagedAt\":null,\"archivedAt\":null,\"autoClosedAt\":null,\"dueDate\":null,\"estimate\":null,\"labelIds\":[],\"assignee\":null,\"creator\":null,\"team\":null,\"state\":null,\"project\":null,\"cycle\":{\"id\":\"c\",\"number\":3,\"name\":null,\"startsAt\":null,\"endsAt\":null}}],\"pageInfo\":{\"hasNextPage\":false,\"endCursor\":null}}",
    ) as Page<Issue>;
    const cycle = page.nodes[0].cycle;
    assert(cycle !== null && cycle.name === null, "unnamed cycle has a null name");
}
