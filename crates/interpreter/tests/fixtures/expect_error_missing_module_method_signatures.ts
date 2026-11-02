// expect-error: package `submilli:fs` does not export `readTex`
// expect-error: did you mean `readText`?
// expect-error: function readText(path: string): string;
import fs from "submilli:fs";
function main(): void { fs.readTex("/input"); }
