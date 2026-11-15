// expect-error: expected `number`, got `number | null`
// A call in the condition invalidates the preceding module-variable guard.
import { relay } from "@test/live";
let current: number | null = 3;
function clear(): boolean { current = null; return false; }
function main(): void {
 if (current === null || clear()) return;
 const value = current;
 const forward = relay;
 const actual: unknown = forward(value);
 assert(actual === null);
}
