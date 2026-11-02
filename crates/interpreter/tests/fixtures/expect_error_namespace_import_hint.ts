// expect-error: import the module namespace with `import fs from "submilli:fs";`
// expect-error: function appendText(path: string, content: string): void;
import { fs } from "submilli:fs";
function main(): void {}
