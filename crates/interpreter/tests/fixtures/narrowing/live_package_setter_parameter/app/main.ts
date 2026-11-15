// expect-error: expected `number`, got `number | null`
// A call in the condition invalidates the earlier null guard before the setter argument.
import { Box } from "@test/live";
let current: number | null = 3;
function clear(): boolean { current = null; return false; }
function main(): void { if(current === null || clear()) return; const box = new Box(); box.value = current; const actual:unknown=box.read();assert(actual===null); }
