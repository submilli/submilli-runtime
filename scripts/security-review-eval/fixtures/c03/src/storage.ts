import { check } from "submilli:security";
import { writeText } from "submilli:fs";
/** Persist a record.
 * @capability acme.write { path: string }
 */
export function persist(path: string): void {
    if (path !== "/restricted") throw new Error("unsupported destination");
    writeText(path, "record");
}
