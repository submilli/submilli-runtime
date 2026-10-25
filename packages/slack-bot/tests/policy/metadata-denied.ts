// Run through `submilli run --blueprint metadata-denied.yaml`, not the allow-all build test runner.
import { sendDirectMessage, sendGroupDirectMessage, sendMessage, updateMessage } from "@submilli/slack-bot";

function deniedAt(capability: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + " denied at " + error.capability;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    const expected = "@submilli/slack-bot denied at " + capability;
    assert(outcome === expected, "expected " + expected + ", got " + outcome);
}

function main(): string {
    // A missing grant is not an absent variable: the post fails before Slack.
    deniedAt("variables.get", () => { sendMessage({ channelId: "C0123456789", text: "hi" }); });
    deniedAt("variables.get", () => { sendMessage({ channelId: "C0123456789", text: "hi", threadTs: "123.456" }); });
    deniedAt("variables.get", () => { sendDirectMessage("U012ABCDEF", "hi"); });
    deniedAt("variables.get", () => { sendGroupDirectMessage(["U012ABCDEF", "WSECOND"], "hi"); });
    deniedAt("secrets.get", () => { updateMessage({ channelId: "C0123456789", ts: "123.456" }, "hi"); });
    return "missing metadata grant denies the post";
}
