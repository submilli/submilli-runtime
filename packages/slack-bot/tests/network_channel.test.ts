import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    addReaction,
    deleteMessage,
    getConversation,
    getMessage,
    getThread,
    listConversations,
    listMembers,
    listMessages,
    removeReaction,
    sendMessage,
    SlackConversation,
    updateMessage,
} from "@submilli/slack-bot";

function findConversation(name: string): SlackConversation {
    const page = listConversations({ limit: 200 });
    for (const conversation of page.conversations) {
        if (conversation.name === name) return conversation;
    }
    throw new Error(`Slack test conversation not found: ${name}`);
}

function main(): void {
    const channelName = secrets.get("SLACK_TEST_CHANNEL");
    if (secrets.get("SLACK_BOT_TOKEN") === undefined || channelName === undefined) return;

    label("bot channel reads");
    const channel = findConversation(channelName);
    assert(getConversation(channel.id).id === channel.id, "conversation lookup returns the test channel");
    listMembers(channel.id, { limit: 1 });
    listMessages(channel.id, { limit: 1 });

    label("bot message lifecycle");
    let rootTs = "";
    let replyTs = "";
    try {
        const root = sendMessage({
            channelId: channel.id,
            text: "[submilli integration test] bot message",
        });
        rootTs = root.ts;

        const fetched = getMessage({ channelId: channel.id, ts: rootTs });
        assert(fetched !== null && fetched.ts === rootTs, "posted message can be read");

        const updated = updateMessage(
            { channelId: channel.id, ts: rootTs },
            "[submilli integration test] bot message updated",
        );
        assert(updated.text.includes("updated"), "posted message can be updated");

        addReaction({ channelId: channel.id, ts: rootTs }, "eyes");
        removeReaction({ channelId: channel.id, ts: rootTs }, "eyes");

        const reply = sendMessage({
            channelId: channel.id,
            text: "[submilli integration test] thread reply",
            threadTs: rootTs,
        });
        replyTs = reply.ts;

        const thread = getThread(channel.id, rootTs, { limit: 100 });
        let foundReply = false;
        for (const message of thread.messages) {
            if (message.ts === replyTs) foundReply = true;
        }
        assert(foundReply, "thread reply can be read");
    } finally {
        if (replyTs !== "") deleteMessage({ channelId: channel.id, ts: replyTs, threadTs: rootTs });
        if (rootTs !== "") deleteMessage({ channelId: channel.id, ts: rootTs });
    }
}
