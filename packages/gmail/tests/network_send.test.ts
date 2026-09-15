import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { sendEmail } from "@submilli/gmail";

function main(): void {
    label("live Gmail self-addressed send");
    if (secrets.get("GOOGLE_ACCESS_TOKEN") === null) return;
    if (secrets.get("GOOGLE_LIVE_MUTATIONS") !== "true") return;
    const recipient = secrets.get("GOOGLE_TEST_EMAIL_RECIPIENT");
    if (recipient === null) return;

    const stamp = Temporal.Now.instant().toString();
    const message = sendEmail({
        to: [recipient],
        subject: "Submilli Gmail integration test " + stamp,
        text: "This is an authorized self-addressed test from @submilli/gmail.\n\nTimestamp: " + stamp,
    });
    assert(message.id.length > 0, "sent message has an ID");
    assert(message.threadId.length > 0, "sent message has a thread ID");
}
