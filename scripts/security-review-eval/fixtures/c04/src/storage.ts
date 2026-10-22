import { check } from "submilli:security";
import { writeText } from "submilli:fs";
/** Persist a record.
 * @capability pied-piper.write { path: string }
 */
export function persist(path: string): void {
    if (path !== "/restricted") throw new Error("unsupported destination");
    check("pied-piper.write", { path: path });
    writeText(path, "record");
}
