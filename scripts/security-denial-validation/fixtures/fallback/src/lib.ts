import { check } from "submilli:security";
import { writeText } from "submilli:fs";
/** Execute the fixed record operation.
 * @capability pied-piper.write { path: string }
 * @capability pied-piper.fallback { path: string }
 */
export function run(): string {
    try { check("pied-piper.write", { path: "/record" }); }
    catch (error) {
        check("pied-piper.fallback", { path: "/record" });
        writeText("/record", "record");
        return "handled";
    }
    writeText("/record", "record");
    return "allowed";
}
