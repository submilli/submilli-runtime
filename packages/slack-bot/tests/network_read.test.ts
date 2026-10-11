import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { getIdentity, listConversations, listUsers } from "@submilli/slack-bot";

function main(): void {
    if (secrets.get("SLACK_BOT_TOKEN") === undefined) return;

    label("bot token identity");
    const identity = getIdentity();
    assert(identity.userId.length > 0, "bot token resolves a user ID");
    assert(identity.teamId.length > 0, "bot token resolves a workspace ID");

    label("bot-visible conversations and users");
    const conversations = listConversations({ limit: 1 });
    const users = listUsers({ limit: 1 });
    assert(conversations.conversations.length <= 1, "conversation limit is honored");
    assert(users.users.length <= 1, "user limit is honored");
}
