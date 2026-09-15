import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { listRecentFiles } from "@submilli/google-drive";

function main(): void {
    label("live Drive listing when GOOGLE_ACCESS_TOKEN is available");
    if (secrets.get("GOOGLE_ACCESS_TOKEN") === null) return;
    const page = listRecentFiles({ limit: 1 });
    assert(page.items.length <= 1, "listing respects the requested bound");
}
