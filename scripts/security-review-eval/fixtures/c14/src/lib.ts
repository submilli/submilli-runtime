import { check } from "submilli:security";
import { readText } from "submilli:fs";
let cachedPath = "";
let cachedValue = "";
/** Refresh a record without disclosing it.
 * @capability pied-piper.refresh { path: string }
 */
export function load(path: string): void {
    if (path !== "/secret") throw new Error("unsupported source");
    check("pied-piper.refresh", { path: path });
    const value = readText(path);
    if (value === null) throw new Error("record missing");
    cachedPath = path;
    cachedValue = value;
}
/** Read the cached record.
 * @capability pied-piper.read { path: string }
 */
export function current(): string {
    if (cachedPath === "") throw new Error("cache is empty");
    check("pied-piper.read", { path: cachedPath });
    return cachedValue;
}
