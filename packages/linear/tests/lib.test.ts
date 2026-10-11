// Pure unit tests for the GraphQL variable builders — no network, no secret.
// Object keys serialize in alphabetical order.

import { label } from "submilli:test";
import {
    buildIssueFilter,
    buildPageVars,
    buildListIssuesVars,
    graphqlErrorMessage,
    httpFailureMessage,
    GraphQlResponse,
    IssueCreateInput,
    IssueUpdateInput,
    Page,
    Issue,
} from "@submilli/linear";

function main(): void {
    label("issue filter: empty cases");
    assert(JSON.stringify(buildIssueFilter()) === "{}", "omitted filter is empty");
    assert(JSON.stringify(buildIssueFilter({})) === "{}", "filter with no fields is empty");

    label("issue filter: single clause, others omitted");
    assert(
        JSON.stringify(buildIssueFilter({ teamId: "T1" })) === "{\"team\":{\"id\":{\"eq\":\"T1\"}}}",
        "team-only clause; absent clauses omitted, not null",
    );

    label("issue filter: all clauses");
    const full = buildIssueFilter({ teamId: "T1", assigneeId: "U1", stateType: "started" });
    assert(
        JSON.stringify(full) ===
            "{\"assignee\":{\"id\":{\"eq\":\"U1\"}},\"state\":{\"type\":{\"eq\":\"started\"}},\"team\":{\"id\":{\"eq\":\"T1\"}}}",
        "all three nested clauses",
    );

    label("issue filter: date clauses");
    const closedThisWeek = buildIssueFilter({
        stateType: "completed",
        completedAtAfter: "2026-06-08T00:00:00.000Z",
        completedAtBefore: "2026-06-15T00:00:00.000Z",
    });
    assert(
        JSON.stringify(closedThisWeek) ===
            "{\"completedAt\":{\"gte\":\"2026-06-08T00:00:00.000Z\",\"lt\":\"2026-06-15T00:00:00.000Z\"},\"state\":{\"type\":{\"eq\":\"completed\"}}}",
        "completed-at range plus completed state",
    );
    assert(
        JSON.stringify(buildIssueFilter({ createdAtAfter: "2026-06-01T00:00:00.000Z" })) ===
            "{\"createdAt\":{\"gte\":\"2026-06-01T00:00:00.000Z\"}}",
        "created-at lower bound",
    );
    assert(
        JSON.stringify(buildIssueFilter({ updatedAtAfter: "2026-06-01T00:00:00.000Z" })) ===
            "{\"updatedAt\":{\"gte\":\"2026-06-01T00:00:00.000Z\"}}",
        "updated-at lower bound",
    );

    label("date filters strip a Temporal bracketed zone annotation (Linear rejects it)");
    assert(
        JSON.stringify(
            buildIssueFilter({ completedAtBefore: "2026-07-15T13:36:00.178322174+00:00[UTC]" }),
        ) === "{\"completedAt\":{\"lt\":\"2026-07-15T13:36:00.178322174+00:00\"}}",
        "ZonedDateTime.toString() form is normalized to the plain offset form",
    );
    assert(
        JSON.stringify(buildIssueFilter({ updatedAtAfter: "2026-07-08T00:00:00Z" })) ===
            "{\"updatedAt\":{\"gte\":\"2026-07-08T00:00:00Z\"}}",
        "annotation-free timestamps pass through unchanged",
    );

    label("page vars: a default `first` is always sent (Linear rejects `after` without it)");
    assert(
        JSON.stringify(buildPageVars()) === "{\"first\":50}",
        "omitted page still sends a default first",
    );
    assert(JSON.stringify(buildPageVars({ first: 25 })) === "{\"first\":25}", "explicit first wins");
    assert(
        JSON.stringify(buildPageVars({ first: 10, after: "cur" })) === "{\"after\":\"cur\",\"first\":10}",
        "first and after",
    );
    assert(
        JSON.stringify(buildPageVars({ after: "cur" })) === "{\"after\":\"cur\",\"first\":50}",
        "cursor-only page gets a default first so Linear accepts `after`",
    );

    label("list-issues vars combine page + filter");
    assert(
        JSON.stringify(buildListIssuesVars({ teamId: "T1" }, { first: 5 })) ===
            "{\"filter\":{\"team\":{\"id\":{\"eq\":\"T1\"}}},\"first\":5}",
        "filter and first present, after omitted",
    );
    assert(
        JSON.stringify(buildListIssuesVars()) === "{\"first\":50}",
        "no filter, no page still sends a default first",
    );
    assert(
        JSON.stringify(buildListIssuesVars(undefined, { after: "cur" })) === "{\"after\":\"cur\",\"first\":50}",
        "cursor-only list-issues page gets a default first",
    );

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

    label("a null endCursor feeds straight back as the `after` cursor (omitted, not sent)");
    assert(
        JSON.stringify(buildPageVars({ after: lastPage.pageInfo.endCursor })) === "{\"first\":50}",
        "null after omitted; default first still sent",
    );
    assert(
        JSON.stringify(buildPageVars({ first: 5, after: "cur" })) === "{\"after\":\"cur\",\"first\":5}",
        "non-null after sent",
    );

    label("graphql error message lifts the actionable extensions detail");
    const validation = graphqlErrorMessage([
        {
            message: "Argument Validation Error",
            path: ["issues"],
            extensions: {
                code: "INVALID_INPUT",
                validationErrors: [{ property: "after" }],
            },
        },
    ]);
    assert(validation.includes("INVALID_INPUT"), "error message surfaces the extensions code");
    assert(validation.includes("after"), "error message names the invalid argument");
    assert(validation.includes("at issues"), "error message includes the field path");

    label("graphql error message degrades to the bare message when no extensions");
    const bare = graphqlErrorMessage([{ message: "Something went wrong" }]);
    assert(
        bare === "Linear GraphQL error: Something went wrong",
        "plain errors render unchanged",
    );

    label("non-2xx failures lift the GraphQL errors body over the status line");
    const validation400 = httpFailureMessage(
        400,
        "Bad Request",
        "{\"errors\":[{\"message\":\"Argument Validation Error\",\"path\":[\"issues\"],\"extensions\":{\"code\":\"INVALID_INPUT\",\"validationErrors\":[{\"property\":\"filter\"}]}}]}",
    );
    assert(validation400.includes("INVALID_INPUT"), "400 body's extensions code surfaces");
    assert(validation400.includes("filter"), "400 body's invalid argument surfaces");

    label("a real Linear 400 body renders despite undeclared extra fields");
    // Captured verbatim from api.linear.app for a bracketed-timestamp filter;
    // `locations` / `type` / `userError` are not modelled and must be tolerated.
    const real400 = httpFailureMessage(
        400,
        "Bad Request",
        "{\"errors\":[{\"message\":\"Variable \\\"$filter\\\" got invalid value \\\"2026-07-15T13:36:00.178322174+00:00[UTC]\\\" at \\\"filter.completedAt.lt\\\"; Expected type \\\"DateTimeOrDuration\\\". Unable to parse value '2026-07-15T13:36:00.178322174+00:00[UTC]' into a valid date\",\"locations\":[{\"line\":1,\"column\":7}],\"extensions\":{\"code\":\"BAD_USER_INPUT\",\"type\":\"graphql error\",\"userError\":true}}]}",
    );
    assert(real400.includes("BAD_USER_INPUT"), "real 400 surfaces the extensions code");
    assert(
        real400.includes("Unable to parse value"),
        "real 400 surfaces the actionable parse detail",
    );

    label("non-GraphQL failure bodies degrade to the status line");
    assert(
        httpFailureMessage(502, "Bad Gateway", "<html>upstream error</html>") ===
            "Linear request failed: HTTP 502 Bad Gateway",
        "HTML body falls back to the status line",
    );
    assert(
        httpFailureMessage(400, "Bad Request", "") ===
            "Linear request failed: HTTP 400 Bad Request",
        "empty body falls back to the status line",
    );

    label("graphql error message lifts a user-presentable detail when present");
    const presentable = graphqlErrorMessage([
        { message: "Argument Validation Error", extensions: { userPresentableMessage: "after needs first" } },
    ]);
    assert(presentable.includes("after needs first"), "user-presentable message is surfaced");

    label("an errors-only 200 body casts, so the GraphQL error is reported instead of a cast failure");
    const errorsOnly = JSON.parse(
        "{\"errors\":[{\"message\":\"Entity not found\",\"path\":null,\"extensions\":null}]}",
    ) as GraphQlResponse<Issue>;
    assert(errorsOnly.data === undefined, "omitted data reads as undefined");
    const errorsOnlyErrors = errorsOnly.errors;
    assert(
        errorsOnlyErrors !== undefined && errorsOnlyErrors !== null &&
            graphqlErrorMessage(errorsOnlyErrors) === "Linear GraphQL error: Entity not found",
        "null path and extensions are tolerated",
    );
    const nullErrors = JSON.parse("{\"data\":null,\"errors\":null}") as GraphQlResponse<Issue>;
    assert(nullErrors.data === null && nullErrors.errors === null, "explicit nulls cast");

    label("null extension fields are skipped when rendering");
    assert(
        httpFailureMessage(
            400,
            "Bad Request",
            "{\"errors\":[{\"message\":\"Bad\",\"extensions\":{\"code\":null,\"userPresentableMessage\":null,\"validationErrors\":[{\"property\":null},{\"property\":\"first\"}]}}]}",
        ) === "Linear GraphQL error: Bad (invalid arguments: first)",
        "null code, message, and property are skipped",
    );

    label("an issue in an unnamed cycle parses");
    const page = JSON.parse(
        "{\"nodes\":[{\"id\":\"i\",\"identifier\":\"ENG-1\",\"number\":1,\"title\":\"T\",\"description\":null,\"priority\":0,\"priorityLabel\":\"No priority\",\"url\":\"https://linear.app/x\",\"createdAt\":\"2026-01-01T00:00:00Z\",\"updatedAt\":\"2026-01-01T00:00:00Z\",\"completedAt\":null,\"canceledAt\":null,\"startedAt\":null,\"triagedAt\":null,\"archivedAt\":null,\"autoClosedAt\":null,\"dueDate\":null,\"estimate\":null,\"labelIds\":[],\"assignee\":null,\"creator\":null,\"team\":null,\"state\":null,\"project\":null,\"cycle\":{\"id\":\"c\",\"number\":3,\"name\":null,\"startsAt\":null,\"endsAt\":null}}],\"pageInfo\":{\"hasNextPage\":false,\"endCursor\":null}}",
    ) as Page<Issue>;
    const cycle = page.nodes[0].cycle;
    assert(cycle !== null && cycle.name === null, "unnamed cycle has a null name");
}
