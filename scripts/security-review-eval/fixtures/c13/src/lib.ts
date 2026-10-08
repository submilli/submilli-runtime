import { check } from "submilli:security";
import { readText } from "submilli:fs";
let cachedPath = "";
let cachedValue = "";
/** Refresh a record without disclosing it.
 * @capability acme.refresh { path: string }
 */
export function load(path: string): void {
    if (path !== "/secret") throw new Error("unsupported source");
    check("acme.refresh", { path: path });
    const value = readText(path);
    if (value === null) throw new Error("record missing");
    cachedPath = path;
    cachedValue = value;
}
/** Read the cached record.
 * @capability acme.read { path: string }
 */
export function current(): string {
    if (cachedPath === "") throw new Error("cache is empty");
    return cachedValue;
}
