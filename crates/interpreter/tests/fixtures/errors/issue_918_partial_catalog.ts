// expect-error: package `node:fs` not found
// expect-error: packages loaded for this compilation (not the full catalog)
// expect-error: use your package-discovery tool
import { readFileSync } from "node:fs";
function main(): void {}
