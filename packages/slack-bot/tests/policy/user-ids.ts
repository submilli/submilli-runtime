// Run through `submilli run --blueprint user-ids.yaml`, not the allow-all build test runner.
import { SlackError, openDirectMessage, openGroupDirectMessage, sendDirectMessage, sendGroupDirectMessage } from "@submilli/slack-bot";

// Slack reads `users` as a comma-separated list. Each of these is one argument that policy allows
// as written and that would also address UBLOCKED, or is not a user ID at all.
const CRAFTED_IDS: string[] = ["UALLOWED,UBLOCKED", "UALLOWED, UBLOCKED", "UALLOWED\nUBLOCKED", " UALLOWED", "ublocked", "C0123456789", ""];

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
    // This denial proves the user ID check passed, without reading a token or calling Slack.
    deniedAt("secrets.get", "@submilli/slack-bot", action);
}

function rejectedUserId(action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: SlackError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === "invalid_user_id", "expected invalid_user_id, got " + outcome);
}

function singleUserHolds(capability: string, call: (userId: string) => void): void {
    deniedAt(capability, "main", () => { call("UBLOCKED"); });
    reachesCredentialBoundary(() => { call("UALLOWED"); });
    for (const crafted of CRAFTED_IDS) rejectedUserId(() => { call(crafted); });
}

function userListHolds(capability: string, call: (userIds: string[]) => void): void {
    // A blocked ID is denied wherever it sits in the list.
    deniedAt(capability, "main", () => { call(["UALLOWED", "UBLOCKED"]); });
    deniedAt(capability, "main", () => { call(["UBLOCKED", "UALLOWED"]); });
    reachesCredentialBoundary(() => { call(["UALLOWED", "WSECOND"]); });
    for (const crafted of CRAFTED_IDS) rejectedUserId(() => { call(["UALLOWED", crafted]); });
}

function main(): string {
    singleUserHolds("slack.com/bot/sendDirectMessage", (userId: string): void => { sendDirectMessage(userId, "hi"); });
    singleUserHolds("slack.com/bot/openDirectMessage", (userId: string): void => { openDirectMessage(userId); });
    userListHolds("slack.com/bot/sendGroupDirectMessage", (userIds: string[]): void => { sendGroupDirectMessage(userIds, "hi"); });
    userListHolds("slack.com/bot/openGroupDirectMessage", (userIds: string[]): void => { openGroupDirectMessage(userIds); });
    return "user ID checks passed";
}
