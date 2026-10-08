import { save } from "@acme/records";
function main(): string {
    let leaked = "";
    try { save("/restricted"); } catch (error: PermissionDeniedError) { return "denied"; }
    return "completed";
}
