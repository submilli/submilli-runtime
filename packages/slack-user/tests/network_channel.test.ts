import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    findUserByEmail,
    getChannel,
    getMessage,
    getUser,
    listChannels,
    listMessages,
    listUsers,
    search,
    SlackChannel,
} from "@submilli/slack-user";

function findChannel(name: string): SlackChannel {
    const page = listChannels({ limit: 200 });
    for (const channel of page.channels) {
        if (channel.name === name) return channel;
    }
    throw new Error(`Slack test channel not found: ${name}`);
}

function main(): void {
    const channelName = secrets.get("SLACK_TEST_CHANNEL");
    if (secrets.get("SLACK_USER_TOKEN") === null || channelName === null) return;

    label("user channel and message reads");
    const channel = findChannel(channelName);
    assert(getChannel(channel.id).id === channel.id, "channel lookup returns the test channel");
    const history = listMessages(channel.id, { limit: 1 });
    if (history.messages.length > 0) {
        const message = history.messages[0];
        let fetched = getMessage({ channelId: channel.id, ts: message.ts });
        if (message.threadTs !== "") {
            fetched = getMessage({ channelId: channel.id, ts: message.ts, threadTs: message.threadTs });
        }
        assert(fetched !== null && fetched.ts === message.ts, "history message can be read directly");
    }

    label("user lookup");
    const users = listUsers({ limit: 1 });
    if (users.users.length > 0) {
        const user = getUser(users.users[0].id);
        assert(user.id === users.users[0].id, "user can be read directly");
        if (user.email !== "") {
            assert(findUserByEmail(user.email).id === user.id, "user can be found by email");
        }
    }

    label("user search");
    search("test", { limit: 1, contextChannelId: channel.id });
}
