// Run through `submilli run --blueprint metadata-valid.yaml`, not the allow-all build test runner.
import { sendDirectMessage, sendGroupDirectMessage, sendMessage, updateMessage } from "@submilli/slack-bot";

function reachesCredentialBoundary(action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + " denied at " + error.capability;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    const expected = "@submilli/slack-bot denied at secrets.get";
    assert(outcome === expected, "expected " + expected + ", got " + outcome);
}

function main(): string {
    reachesCredentialBoundary(() => { sendMessage({ channelId: "C0123456789", text: "hi" }); });
    reachesCredentialBoundary(() => { sendMessage({ channelId: "C0123456789", text: "hi", threadTs: "123.456" }); });
    reachesCredentialBoundary(() => { sendDirectMessage("U012ABCDEF", "hi"); });
    reachesCredentialBoundary(() => { sendGroupDirectMessage(["U012ABCDEF", "WSECOND"], "hi"); });
    reachesCredentialBoundary(() => { updateMessage({ channelId: "C0123456789", ts: "123.456" }, "hi"); });
    return "valid metadata accepted";
}
