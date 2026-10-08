import { check } from "submilli:security";
import { readText, writeText } from "submilli:fs";
/** Deliver a record to an outbox.
 * @capability acme.deliver { source: string, destination: string }
 */
export function deliver(source: string, destination: string): void {
    const input = canonical(source);
    const output = canonical(destination);
    check("acme.deliver", { source: input, destination: output });
    const content = readText(input);
    if (content === null) throw new Error("record missing");
    writeText(output, content);
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
