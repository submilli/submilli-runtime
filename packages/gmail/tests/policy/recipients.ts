// Run through `submilli run --blueprint recipients.yaml`, not the allow-all build test runner.
import { EmailInput, sendEmail, createDraft, GmailError } from "@submilli/gmail";

// `main` controls this object while the package reads it. `to` answers an allowed list to its
// first read and the blocked address to every later one, so a package that reads it for `check`
// and again for the message approves one list and writes another.
class FlippingEmail implements EmailInput {
    subject: string = "s";
    text: string = "t";
    private toReadCount: number = 0;

    get to(): string[] {
        this.toReadCount += 1;
        return this.toReadCount === 1 ? ["allowed@example.com"] : ["blocked@example.com"];
    }

    // `EmailInput` fields are writable, which a getter alone does not satisfy.
    set to(value: string[]) {}

    toReads(): number {
        return this.toReadCount;
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
    // This denial proves the recipient check passed, without reading a token or sending mail.
    deniedAt("secrets.get", "@submilli/gmail", action);
}

function rejectedRecipient(action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: GmailError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === "invalid_recipient", "expected invalid_recipient, got " + outcome);
}

function readsRecipientsOnce(operation: string, call: (input: EmailInput) => void): void {
    const flipping = new FlippingEmail();
    reachesCredentialBoundary(() => { call(flipping); });
    // The message is built before the credential boundary and never sent, so its headers cannot
    // be inspected. One read is the proof instead: the package holds a single list, `check`
    // approved it, and any further read answers the blocked address.
    assert(flipping.toReads() === 1, operation + " read to " + flipping.toReads().toString() + " times, expected 1");
    const secondRead = flipping.to.join(" ");
    assert(secondRead === "blocked@example.com", "second read of to answered " + secondRead);
}

function main(): string {
    // Checking To alone fails the Cc and Bcc assertions.
    deniedAt("submilli/gmail.sendEmail", "main", () => { sendEmail({ to: ["blocked@example.com"], subject: "s", text: "t" }); });
    deniedAt("submilli/gmail.sendEmail", "main", () => { sendEmail({ to: ["allowed@example.com"], cc: ["blocked@example.com"], subject: "s", text: "t" }); });
    deniedAt("submilli/gmail.sendEmail", "main", () => { sendEmail({ to: ["allowed@example.com"], bcc: ["blocked@example.com"], subject: "s", text: "t" }); });
    // Policy reads the address the header will carry, not the entry as typed.
    deniedAt("submilli/gmail.sendEmail", "main", () => { sendEmail({ to: ["allowed@example.com"], bcc: [" blocked@example.com "], subject: "s", text: "t" }); });
    deniedAt("submilli/gmail.sendEmail", "main", () => { sendEmail({ to: ["allowed@example.com"], cc: ["\tblocked@example.com\t"], subject: "s", text: "t" }); });
    reachesCredentialBoundary(() => { sendEmail({ to: ["allowed@example.com"], cc: ["second@example.com"], bcc: ["third@example.com"], subject: "s", text: "t" }); });

    deniedAt("submilli/gmail.createDraft", "main", () => { createDraft({ to: ["blocked@example.com"], subject: "s", text: "t" }); });
    deniedAt("submilli/gmail.createDraft", "main", () => { createDraft({ to: ["allowed@example.com"], cc: ["blocked@example.com"], subject: "s", text: "t" }); });
    deniedAt("submilli/gmail.createDraft", "main", () => { createDraft({ to: ["allowed@example.com"], bcc: ["blocked@example.com"], subject: "s", text: "t" }); });
    deniedAt("submilli/gmail.createDraft", "main", () => { createDraft({ to: ["allowed@example.com"], bcc: [" blocked@example.com "], subject: "s", text: "t" }); });
    reachesCredentialBoundary(() => { createDraft({ to: ["allowed@example.com"], cc: ["second@example.com"], bcc: ["third@example.com"], subject: "s", text: "t" }); });

    // One entry is one address. Each of these would reach blocked@example.com behind an entry policy allows.
    rejectedRecipient(() => { sendEmail({ to: ["allowed@example.com, blocked@example.com"], subject: "s", text: "t" }); });
    rejectedRecipient(() => { sendEmail({ to: ["allowed@example.com"], cc: ["allowed@example.com\r\nBcc: blocked@example.com"], subject: "s", text: "t" }); });
    rejectedRecipient(() => { sendEmail({ to: ["allowed@example.com"], bcc: ["Allowed <blocked@example.com>"], subject: "s", text: "t" }); });
    rejectedRecipient(() => { createDraft({ to: ["allowed@example.com, blocked@example.com"], subject: "s", text: "t" }); });
    rejectedRecipient(() => { createDraft({ to: ["allowed@example.com"], cc: ["allowed@example.com\r\nBcc: blocked@example.com"], subject: "s", text: "t" }); });
    rejectedRecipient(() => { createDraft({ to: ["allowed@example.com"], bcc: ["Allowed <blocked@example.com>"], subject: "s", text: "t" }); });

    // Each of these is drawn like, or is delivered as, blocked@example.com, and none equals it.
    for (const entry of [
        "blocked@example\uff0ecom",
        "blocked@exam\u200bple.com",
        "blocked@exam\u00adple.com",
        "blocked@exam\ufeffple.com",
        // A string literal cannot hold a lone surrogate.
        "blocked@example.com" + String.fromCharCode(0xd800),
    ]) {
        rejectedRecipient(() => { sendEmail({ to: [entry], subject: "s", text: "t" }); });
        rejectedRecipient(() => { sendEmail({ to: ["allowed@example.com"], bcc: [entry], subject: "s", text: "t" }); });
        rejectedRecipient(() => { createDraft({ to: ["allowed@example.com"], cc: [entry], subject: "s", text: "t" }); });
    }

    readsRecipientsOnce("sendEmail", (input: EmailInput): void => { sendEmail(input); });
    readsRecipientsOnce("createDraft", (input: EmailInput): void => { createDraft(input); });
    return "recipient capability checks passed";
}
