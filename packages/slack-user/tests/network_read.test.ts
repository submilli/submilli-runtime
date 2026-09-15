import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { getIdentity, listChannels, listUsers } from "@submilli/slack-user";

function main(): void {
    if (secrets.get("SLACK_USER_TOKEN") === null) return;

    label("user token identity");
    const identity = getIdentity();
    assert(identity.userId.length > 0, "user token resolves a user ID");
    assert(identity.teamId.length > 0, "user token resolves a workspace ID");

    label("user-visible channels and users");
    const channels = listChannels({ limit: 1 });
    const users = listUsers({ limit: 1 });
    assert(channels.channels.length <= 1, "channel limit is honored");
    assert(users.users.length <= 1, "user limit is honored");
}
