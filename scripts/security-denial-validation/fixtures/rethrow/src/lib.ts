import { check } from "submilli:security";
import { writeText } from "submilli:fs";
/** Execute the fixed record operation.
 * @capability acme.write { path: string }
 */
export function run(): string {
    try { check("acme.write", { path: "/record" }); }
    catch (error) { throw error; }
    writeText("/record", "record");
    return "allowed";
}
