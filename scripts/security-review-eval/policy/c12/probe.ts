import { save } from "@pied-piper/records";
function main(): string {
    let leaked = "";
    try { save("/public/../restricted"); } catch (error: PermissionDeniedError) { return "denied"; }
    return "completed";
}
