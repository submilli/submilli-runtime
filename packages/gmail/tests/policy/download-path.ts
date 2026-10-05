// Run with the restricted download-path.yaml blueprint, without credentials or network.
import { downloadAttachment } from "@submilli/gmail";

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
        deniedAt("fs.write", "main", () => { downloadAttachment("message", "attachment", path); });
    }
    for (const path of ["/downloads/file", "/downloads/nested/../file", "downloads/file"]) {
        deniedAt("secrets.get", "@submilli/gmail", () => { downloadAttachment("message", "attachment", path); });
    }
    deniedAt("submilli/gmail.downloadAttachment", "main", () => { downloadAttachment("message", "attachment", "/downloads/business-denied"); });
    return "download destination checks passed";
}
