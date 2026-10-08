import { check } from "submilli:security";
import { writeText } from "submilli:fs";
/** Save a record.
 * @capability acme.write { path: string }
 */
export function save(path: string): void {
    const destination = canonical(path);
    check("acme.write", { path: path });
    writeText(destination, "record");
}
function canonical(path: string): string {
    if (!path.startsWith("/")) throw new Error("absolute path required");
    const parts: string[] = [];
    for (const part of path.split("/")) {
        if (part === "" || part === ".") continue;
        if (part === "..") { parts.pop(); } else { parts.push(part); }
    }
    return "/" + parts.join("/");
}
