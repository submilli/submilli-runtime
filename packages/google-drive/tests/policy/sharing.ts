// Run through `submilli run --blueprint sharing.yaml`, not the allow-all build test runner.
import { DriveError, ShareFileInput, copyFile, createFolder, moveFile, shareFile, uploadFile } from "@submilli/google-drive";
import { write } from "submilli:fs";

// `main` controls this object while the package reads it. `emailAddress` answers the allowed
// address to its first read and another to every later one, so a package that reads it for
// `check` and again for the request approves one grantee and shares with another.
class FlippingShare implements ShareFileInput {
    type: string = "user";
    role: string = "reader";
    private emailReadCount: number = 0;

    get emailAddress(): string {
        this.emailReadCount += 1;
        return this.emailReadCount === 1 ? "dana@example.com" : "other@example.com";
    }

    // `ShareFileInput` fields are writable, which a getter alone does not satisfy.
    set emailAddress(value: string) {}

    emailReads(): number {
        return this.emailReadCount;
    }
}

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

function reachesCredentialBoundary(action: () => void): void {
    // This denial proves the business check passed, without reading a token or calling Drive.
    deniedAt("secrets.get", "@submilli/google-drive", action);
}

function rejectedAs(code: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: DriveError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === code, "expected " + code + ", got " + outcome);
}

function share(input: ShareFileInput): void {
    shareFile("file1", input);
}

function main(): string {
    reachesCredentialBoundary(() => { share({ type: "user", role: "reader", emailAddress: "dana@example.com" }); });
    // Policy reads the address in lowercase, whichever way the caller wrote it.
    reachesCredentialBoundary(() => { share({ type: "user", role: "reader", emailAddress: "Dana@Example.com" }); });
    reachesCredentialBoundary(() => { share({ type: "user", role: "reader", emailAddress: "Dana@Example.com." }); });
    deniedAt("submilli/google-drive.shareFile", "main", () => { share({ type: "user", role: "reader", emailAddress: "other@example.com" }); });
    // A role and a type are policy-visible: readers are allowed, writers and groups are not.
    deniedAt("submilli/google-drive.shareFile", "main", () => { share({ type: "user", role: "writer", emailAddress: "dana@example.com" }); });
    deniedAt("submilli/google-drive.shareFile", "main", () => { share({ type: "group", role: "reader", emailAddress: "dana@example.com" }); });
    deniedAt("submilli/google-drive.shareFile", "main", () => { share({ type: "anyone", role: "reader" }); });

    // Whether the grantee is emailed is policy-visible: this blueprint allows a share only with the email.
    deniedAt("submilli/google-drive.shareFile", "main", () => { share({ type: "user", role: "reader", emailAddress: "dana@example.com", sendNotificationEmail: false }); });
    // Drive emails nobody about a domain share, so it is reported as not notifying whatever the input says.
    reachesCredentialBoundary(() => { share({ type: "domain", role: "reader", domain: "example.com", sendNotificationEmail: true }); });
    // A domain is read in one spelling too, and whether the file can be found by search is policy-visible.
    reachesCredentialBoundary(() => { share({ type: "domain", role: "reader", domain: "Example.COM." }); });
    deniedAt("submilli/google-drive.shareFile", "main", () => { share({ type: "domain", role: "reader", domain: "other.example" }); });
    deniedAt("submilli/google-drive.shareFile", "main", () => { share({ type: "domain", role: "reader", domain: "example.com", allowFileDiscovery: true }); });

    const flipping = new FlippingShare();
    reachesCredentialBoundary(() => { share(flipping); });
    assert(flipping.emailReads() === 1, "shareFile read emailAddress " + flipping.emailReads().toString() + " times, expected 1");

    // A second principal beside the one policy reads is refused before the check.
    rejectedAs("invalid_permission_principal", () => { share({ type: "user", role: "reader", emailAddress: "dana@example.com", domain: "other.example" }); });
    rejectedAs("invalid_permission_principal", () => { share({ type: "domain", role: "reader", domain: "example.com", emailAddress: "dana@example.com" }); });
    rejectedAs("invalid_permission_principal", () => { share({ type: "anyone", role: "reader", emailAddress: "dana@example.com" }); });
    rejectedAs("invalid_permission_principal", () => { share({ type: "anyone", role: "reader", domain: "example.com" }); });
    rejectedAs("invalid_permission_principal", () => { share({ type: "user", role: "reader" }); });
    rejectedAs("invalid_permission_principal", () => { share({ type: "domain", role: "reader" }); });
    rejectedAs("invalid_permission_principal", () => { share({ type: "user", role: "reader", emailAddress: "dana@example.com", allowFileDiscovery: true }); });
    // One value is one account or one domain, in one spelling.
    for (const entry of [" dana@example.com", "dana@example.com,other@example.com", "Other <dana@example.com>", "dana@example.com\n", "dana", "dana@."]) {
        rejectedAs("invalid_permission_principal", () => { share({ type: "user", role: "reader", emailAddress: entry }); });
    }
    for (const entry of ["example.com/x", "example.com,other.example", " example.com", "."]) {
        rejectedAs("invalid_permission_principal", () => { share({ type: "domain", role: "reader", domain: entry }); });
    }
    rejectedAs("invalid_permission_type", () => { share({ type: "everyone", role: "reader", emailAddress: "dana@example.com" }); });
    rejectedAs("invalid_permission_role", () => { share({ type: "user", role: "owner", emailAddress: "dana@example.com" }); });

    write("/report.txt", new TextEncoder().encode("report"));
    reachesCredentialBoundary(() => { uploadFile("/report.txt", { name: "report.txt", mimeType: "text/plain", parentId: "allowedFolder" }); });
    reachesCredentialBoundary(() => { uploadFile("/report.txt", { name: "report.txt", mimeType: "text/plain", parentId: "otherFolder" }); });
    // Uploads resolve the actual drive before checking the parent/drive policy.
    // The denied metadata read is the sentinel here; contract tests cover the check.
    reachesCredentialBoundary(() => { uploadFile("/report.txt", { name: "report.txt", mimeType: "text/plain" }); });

    reachesCredentialBoundary(() => { copyFile("file1", { parentId: "allowedFolder" }); });
    deniedAt("submilli/google-drive.copyFile", "main", () => { copyFile("file1", { parentId: "otherFolder" }); });
    deniedAt("submilli/google-drive.copyFile", "main", () => { copyFile("file1"); });

    // Drive reads `addParents` as a comma-separated list, so a parent is one ID.
    rejectedAs("invalid_parent", () => { moveFile("file1", "allowedFolder,otherFolder"); });
    rejectedAs("invalid_parent", () => { moveFile("file1", ""); });
    rejectedAs("invalid_parent", () => { moveFile("file1", "allowed.Folder"); });
    rejectedAs("invalid_parent", () => { copyFile("file1", { parentId: "allowedFolder,otherFolder" }); });
    rejectedAs("invalid_parent", () => { uploadFile("/report.txt", { name: "report.txt", mimeType: "text/plain", parentId: "allowedFolder/../x" }); });
    rejectedAs("invalid_parent", () => { createFolder("reports", "allowedFolder,otherFolder"); });
    // An alias such as `root` is one ID.
    reachesCredentialBoundary(() => { createFolder("reports", "root"); });
    reachesCredentialBoundary(() => { moveFile("file1", "allowedFolder"); });
    deniedAt("submilli/google-drive.moveFile", "main", () => { moveFile("file1", "otherFolder"); });
    return "sharing and destination checks passed";
}
