import { load, current } from "@pied-piper/records";
function main(): string {
    let leaked = "";
    try { load("/secret"); leaked = current(); } catch (error: PermissionDeniedError) { return "denied"; }
    return leaked;
}
