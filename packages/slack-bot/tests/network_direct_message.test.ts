import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { sendDirectMessage, sendGroupDirectMessage } from "@submilli/slack-bot";

function main(): void {
    if (secrets.get("SLACK_BOT_TOKEN") === undefined) return;

    const recipientId = secrets.get("SLACK_TEST_DM_USER_ID");
    if (recipientId !== undefined) {
        label("bot direct message");
        const message = sendDirectMessage(
            recipientId,
            "[submilli integration test] Bot direct-message delivery passed.",
        );
        assert(message.ts.length > 0, "direct message has a Slack timestamp");
        assert(message.text.includes("delivery passed"), "direct message preserves its text");
    }

    const groupRecipientIds = secrets.get("SLACK_TEST_GROUP_DM_USER_IDS");
    if (groupRecipientIds !== undefined) {
        label("bot group direct message");
        const userIds = groupRecipientIds.split(",");
        assert(userIds.length >= 2, "group DM test requires at least two approved user IDs");
        const message = sendGroupDirectMessage(
            userIds,
            "[submilli integration test] Bot group-DM delivery passed.",
        );
        assert(message.ts.length > 0, "group direct message has a Slack timestamp");
        assert(message.text.includes("delivery passed"), "group direct message preserves its text");
    }
}
