import { check } from "submilli:security";
import { writeText } from "submilli:fs";
/** Execute the fixed record operation.
 * @capability acme.write { path: string }
 * @capability acme.fallback { path: string }
 */
export function run(): string {
    try { check("acme.write", { path: "/record" }); }
    catch (error) {
        check("acme.fallback", { path: "/record" });
        writeText("/record", "record");
        return "handled";
    }
    writeText("/record", "record");
    return "allowed";
}
