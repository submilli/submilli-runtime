import { registerFinalizer, unregisterFinalizer } from "submilli:session";

function main(): void {
    let denials = 0;
    try {
        registerFinalizer("resource", { id: "resource" });
    } catch (error: PermissionDeniedError) {
        denials++;
    }
    try {
        unregisterFinalizer("resource");
    } catch (error: PermissionDeniedError) {
        denials++;
    }
    assert(denials === 2, "finalizer registrations are package owned");
}
