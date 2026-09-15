import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { getProfile, searchThreads } from "@submilli/gmail";

function main(): void {
    label("live Gmail reads when GOOGLE_ACCESS_TOKEN is available");
    if (secrets.get("GOOGLE_ACCESS_TOKEN") === null) return;
    const profile = getProfile();
    assert(profile.emailAddress.length > 0, "profile has an email address");
    const page = searchThreads("in:inbox", { limit: 1 });
    assert(page.items.length <= 1, "search respects the requested bound");
}
