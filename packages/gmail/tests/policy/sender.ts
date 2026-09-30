// Run through `submilli run --blueprint sender.yaml`, not the allow-all build test runner.
import { EmailInput, GmailError, sendEmail, createDraft } from "@submilli/gmail";

// `main` controls this object while the package reads it. `from` answers the allowed alias to its
// first read and another address to every later one, so a package that reads it for `check` and
// again for the header approves one sender and writes another.
class FlippingSender implements EmailInput {
    to: string[] = ["dana@example.com"];
    subject: string = "s";
    text: string = "t";
    private fromReadCount: number = 0;

    get from(): string {
        this.fromReadCount += 1;
        return this.fromReadCount === 1 ? "alias@example.com" : "other@example.com";
    }

    // `EmailInput` fields are writable, which a getter alone does not satisfy.
    set from(value: string) {}

    fromReads(): number {
        return this.fromReadCount;
    }
}

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
    // This denial proves the sender check passed, without reading a token or sending mail.
    deniedAt("secrets.get", "@submilli/gmail", action);
}

function rejectedSender(action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: GmailError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === "invalid_sender", "expected invalid_sender, got " + outcome);
}

function from(sender: string): EmailInput {
    return { to: ["dana@example.com"], subject: "s", text: "t", from: sender };
}

function senderRuleHolds(capability: string, call: (input: EmailInput) => void): void {
    reachesCredentialBoundary(() => { call(from("alias@example.com")); });
    // Policy reads the address the header will carry, not the value as typed.
    reachesCredentialBoundary(() => { call(from(" Alias@Example.com. ")); });
    deniedAt(capability, "main", () => { call(from("other@example.com")); });
    // Without `from` the message is sent as the account itself, which this rule does not allow.
    deniedAt(capability, "main", () => { call({ to: ["dana@example.com"], subject: "s", text: "t" }); });
    // One value is one address. Each of these would put a second sender behind the allowed one.
    rejectedSender(() => { call(from("alias@example.com, other@example.com")); });
    rejectedSender(() => { call(from("alias@example.com <other@example.com>")); });
    rejectedSender(() => { call(from("alias@example.com\r\nFrom: other@example.com")); });

    const flipping = new FlippingSender();
    reachesCredentialBoundary(() => { call(flipping); });
    assert(flipping.fromReads() === 1, capability + " read from " + flipping.fromReads().toString() + " times, expected 1");
}

function main(): string {
    senderRuleHolds("submilli/gmail.sendEmail", (input: EmailInput): void => { sendEmail(input); });
    senderRuleHolds("submilli/gmail.createDraft", (input: EmailInput): void => { createDraft(input); });
    return "sender capability checks passed";
}
