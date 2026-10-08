import { check } from "submilli:security";
import { writeText } from "submilli:fs";
/** Save a record.
 * @capability acme.write { path: string }
 */
export function save(path: string): void {
    if (path !== "/restricted") throw new Error("unsupported destination");
    check("acme.write", { path: path });
    writeText(path, "record");
}
