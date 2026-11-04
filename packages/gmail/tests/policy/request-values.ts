// Run through `submilli run --blueprint request-values.yaml`, not the allow-all build test runner.
import { EmailInput, sendEmail, createDraft } from "@submilli/gmail";

// The blueprint denies the package every request, so a message that passes main's check
// stops at its HTTP call, and nothing is sent.
const SENT_MESSAGE = "@submilli/gmail: policy denied http.post for @submilli/gmail";

const ALLOWED: string[] = ["allowed@example.com"];
const BLOCKED: string[] = ["blocked@example.com", "blocked.archive@example.com", "blocked.audit@example.com"];

// `main` controls this object while the package reads it. `to` answers the allowed list to its
// first read and the blocked list to every later one. The package here holds a token, so it
// builds the whole message, and the blueprint reports which list that message was built from.
class FlippingEmail implements EmailInput {
    subject: string = "s";
    text: string = "t";
    private toReadCount: number = 0;

    get to(): string[] {
        this.toReadCount += 1;
        return this.toReadCount === 1 ? ALLOWED : BLOCKED;
    }

    // `EmailInput` fields are writable, which a getter alone does not satisfy.
    set to(value: string[]) {}

    toReads(): number {
        return this.toReadCount;
    }
}

function requestValuesHold(capability: string, call: (input: EmailInput) => void): void {
    assertOutcome("main: policy denied " + capability + " for main", () => { call({ to: BLOCKED, subject: "s", text: "t" }); });
    assertOutcome(SENT_MESSAGE, () => { call({ to: ALLOWED, subject: "s", text: "t" }); });

    // Main's check saw the allowed list, and the package read it once, so the message it
    // built is for the recipients main's check allowed.
    const flipping = new FlippingEmail();
    assertOutcome(SENT_MESSAGE, () => { call(flipping); });
    assert(flipping.toReads() === 1, capability + " read to " + flipping.toReads().toString() + " times, expected 1");
}

function assertOutcome(expected: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + ": " + error.reason;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === expected, "expected " + expected + ", got " + outcome);
}

function main(): string {
    requestValuesHold("submilli/gmail.sendEmail", (input: EmailInput): void => { sendEmail(input); });
    requestValuesHold("submilli/gmail.createDraft", (input: EmailInput): void => { createDraft(input); });
    return "request value checks passed";
}
