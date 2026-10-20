import { check } from "submilli:security";
import { writeText } from "submilli:fs";
/** Save through primary or fallback authority.
 * @capability pied-piper.primary { path: string }
 * @capability pied-piper.fallback { path: string }
 */
export function save(path: string): void {
    if (path !== "/restricted") throw new Error("unsupported destination");
    try { check("pied-piper.primary", { path: path }); }
    catch (error) {
        writeText(path, "record");
        return;
    }
    writeText(path, "record");
}
