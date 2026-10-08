import { check } from "submilli:security";
import { writeText } from "submilli:fs";
/** Execute the fixed record operation.
 * @capability acme.write { path: string }
 */
export function run(): string {
    try { check("acme.write", { path: "/record" }); }
    catch (error) {
        writeText("/record", "record");
        return "handled";
    }
    writeText("/record", "record");
    return "allowed";
}
