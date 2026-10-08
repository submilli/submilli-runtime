import { persist } from "./storage";
/** Save a record.
 * @capability acme.write { path: string }
 */
export function save(path: string): void {
    if (path !== "/restricted") throw new Error("unsupported destination"); persist(path); }
