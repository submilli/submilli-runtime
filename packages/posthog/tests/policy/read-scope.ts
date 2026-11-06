// Run through `submilli run --blueprint read-scope.yaml`, not the allow-all build test runner.
import { listEvents, getCohort, capture } from "@submilli/posthog";

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
    // This denial proves the business checks passed, without reading a key or calling PostHog.
    deniedAt("secrets.get", "@submilli/posthog", action);
}

function main(): string {
    const after = "2026-10-24T06:50:00Z";
    const before = "2026-10-24T08:50:00Z";
    reachesCredentialBoundary(() => {
        listEvents(602724, {
            events: ["$pageview"], after: after, before: before, properties: ["$host", "path"], fields: ["$virt_is_bot"],
            filters: [{ scope: "person", property: "first_touch", key: "utm_source", operator: "notIn", values: ["e2e"] }],
        });
    });
    // Another project is denied.
    deniedAt("posthog.com/events.list", "main", () => { listEvents(1, { events: ["$pageview"], after: after, before: before }); });
    // A property outside the allowlist is denied, selected or filtered, event or person.
    deniedAt("posthog.com/properties.read", "main", () => { listEvents(602724, { events: ["$identify"], after: after, before: before, properties: ["$set"] }); });
    deniedAt("posthog.com/properties.read", "main", () => { listEvents(602724, { events: ["$pageview"], after: after, before: before, personProperties: ["email"] }); });
    deniedAt("posthog.com/properties.read", "main", () => {
        listEvents(602724, { events: ["$pageview"], after: after, before: before, filters: [{ scope: "person", property: "email", operator: "in", values: ["dana@acme.com"] }] });
    });
    // Allowing one nested path allows neither the whole object nor another key in it.
    deniedAt("posthog.com/properties.read", "main", () => { listEvents(602724, { events: ["$pageview"], after: after, before: before, personProperties: ["first_touch"] }); });
    deniedAt("posthog.com/properties.read", "main", () => {
        listEvents(602724, { events: ["$pageview"], after: after, before: before, filters: [{ scope: "person", property: "first_touch", key: "landing_path", operator: "in", values: ["/x"] }] });
    });
    // Events-table fields are checked too.
    deniedAt("posthog.com/properties.read", "main", () => { listEvents(602724, { events: ["$pageview"], after: after, before: before, fields: ["$virt_bot_name"] }); });
    // An allowed name in the wrong scope is denied.
    deniedAt("posthog.com/properties.read", "main", () => { listEvents(602724, { events: ["$pageview"], after: after, before: before, personProperties: ["$host"] }); });
    reachesCredentialBoundary(() => { getCohort(602724, 561927); });
    deniedAt("posthog.com/cohorts.get", "main", () => { getCohort(602724, 1); });
    // Capture is a separate capability and is not granted.
    deniedAt("posthog.com/events.capture", "main", () => { capture({ event: "demo_booked", distinctId: "d" }); });
    return "read scope checks passed";
}
