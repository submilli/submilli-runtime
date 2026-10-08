import { save } from "@acme/records";
function main(): string {
    let leaked = "";
    try { save("alpha", "beta"); } catch (error: PermissionDeniedError) { return "denied"; }
    return "completed";
}
