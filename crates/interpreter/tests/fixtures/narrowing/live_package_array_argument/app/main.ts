// expect-error: expected `number`, got `number | null`
// A call in the condition invalidates the preceding module-variable guard.
import { element } from "@test/live";
let current: number | null = 3;
function clear(): boolean { current = null; return false; }
function main(): void {
 if (current === null || clear()) return;
 const value = current;
 const actual: unknown = element([value]);
 assert(actual === null);
}
