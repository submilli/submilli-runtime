// Run through `submilli run --blueprint metadata-malformed.yaml`, not the allow-all build test runner.
import { SlackError, sendDirectMessage, sendGroupDirectMessage, sendMessage, updateMessage } from "@submilli/slack-bot";

function metadataRejected(action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: SlackError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === "invalid_metadata", "expected invalid_metadata, got " + outcome);
}

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
    metadataRejected(() => { sendMessage({ channelId: "C0123456789", text: "hi" }); });
    metadataRejected(() => { sendMessage({ channelId: "C0123456789", text: "hi", threadTs: "123.456" }); });
    metadataRejected(() => { sendDirectMessage("U012ABCDEF", "hi"); });
    metadataRejected(() => { sendGroupDirectMessage(["U012ABCDEF", "WSECOND"], "hi"); });
    // An update must not read the variable, or this malformed default would reject it.
    reachesCredentialBoundary(() => { updateMessage({ channelId: "C0123456789", ts: "123.456" }, "hi"); });
    return "malformed metadata rejected before send";
}
