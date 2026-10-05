// Run with the restricted download-path.yaml blueprint, without credentials or network.
import { downloadFile } from "@submilli/google-drive";

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
        deniedAt("secrets.get", "@submilli/google-drive", () => { downloadFile("file", path); });
    }
    deniedAt("submilli/google-drive.downloadFile", "main", () => { downloadFile("file", "/downloads/business-denied"); });
    deniedAt("fs.write", "main", () => { downloadFile("file", "/downloads/file", { maxBytes: 20000001 }); });
    deniedAt("secrets.get", "@submilli/google-drive", () => { downloadFile("file", "/downloads/file", { maxBytes: 0 }); });
    for (const maxBytes of [-1, 0.5, NaN, Infinity, 9007199254740992]) {
        let rejected = false;
        try { downloadFile("file", "/downloads/file", { maxBytes: maxBytes }); }
        catch (error: RangeError) { rejected = true; }
        assert(rejected, "invalid maxBytes must fail before credentials");
    }
    return "download destination checks passed";
}
