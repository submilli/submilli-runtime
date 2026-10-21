import { deliver } from "@pied-piper/records";
function main(): string {
    let leaked = "";
    try { deliver("/secret", "/untrusted"); } catch (error: PermissionDeniedError) { return "denied"; }
    return "completed";
}
