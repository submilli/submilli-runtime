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
    // This denial proves the business check passed, without reading a key or calling PostHog.
    deniedAt("secrets.get", "@submilli/posthog", action);
}

function main(): string {
    const after = "2026-10-24T06:50:00Z";
    const before = "2026-10-24T08:50:00Z";
    reachesCredentialBoundary(() => { listEvents(602724, { events: ["$pageview"], after: after, before: before, properties: ["$host", "path"] }); });
    // Another project is denied.
    deniedAt("posthog.com/events.list", "main", () => { listEvents(1, { events: ["$pageview"], after: after, before: before }); });
    // Selecting a forbidden property is denied, for event and person properties.
    deniedAt("posthog.com/events.list", "main", () => { listEvents(602724, { events: ["$pageview"], after: after, before: before, properties: ["email"] }); });
    deniedAt("posthog.com/events.list", "main", () => { listEvents(602724, { events: ["$pageview"], after: after, before: before, personProperties: ["email"] }); });
    // Filtering on it would leak it one value at a time, so a filter counts as a read.
    deniedAt("posthog.com/events.list", "main", () => {
        listEvents(602724, { events: ["$pageview"], after: after, before: before, filters: [{ scope: "person", property: "email", operator: "in", values: ["dana@acme.com"] }] });
    });
    reachesCredentialBoundary(() => { getCohort(602724, 561927); });
    deniedAt("posthog.com/cohorts.get", "main", () => { getCohort(602724, 1); });
    // Capture is a separate capability and is not granted.
    deniedAt("posthog.com/events.capture", "main", () => { capture({ event: "demo_booked", distinctId: "d" }); });
    return "read scope checks passed";
}
