// Run with the restricted download-path.yaml blueprint, without credentials or network.
import { downloadFile } from "@submilli/slack-user";

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

function main(): string {
    for (const path of ["/elsewhere/file", "/downloads/../elsewhere/file", "/downloads/nested/../../elsewhere/file"]) {
        deniedAt("fs.write", "main", () => { downloadFile("file", path); });
    }
    for (const path of ["/downloads/file", "/downloads/nested/../file", "downloads/file"]) {
        deniedAt("secrets.get", "@submilli/slack-user", () => { downloadFile("file", path); });
    }
    deniedAt("slack.com/user/downloadFile", "main", () => { downloadFile("file", "/downloads/business-denied"); });
    return "download destination checks passed";
}
