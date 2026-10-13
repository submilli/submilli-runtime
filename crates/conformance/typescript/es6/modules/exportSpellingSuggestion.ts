// @module: commonjs
// @target: es2015
// @filename: a.ts
export function assertNever(x: never, msg: string): void {
    throw new Error("Unexpected " + msg);
}

// @filename: b.ts
import { assertNevar } from "./a";
