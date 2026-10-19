import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { github } from "@submilli/scrapecreators";
function main(): void {
    const key = secrets.get("SCRAPECREATORS_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: SCRAPECREATORS_API_KEY is not bound");
        return;
    }
    label("live GitHub user retains public identity");
    const user = github.getUser({ handle: "octocat" }).data;
    assert(user.success && user.login === "octocat", "GitHub identity");
}
