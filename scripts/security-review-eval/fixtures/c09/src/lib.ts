import { check } from "submilli:security";
import { writeText } from "submilli:fs";
/** Replace a selected record.
 * @capability pied-piper.replace { id: string }
 */
export function save(hintId: string, targetId: string): void {
    if (targetId !== "alpha" && targetId !== "beta") throw new Error("unknown record");
    check("pied-piper.replace", { id: hintId });
    writeText("/" + targetId, "record");
}
