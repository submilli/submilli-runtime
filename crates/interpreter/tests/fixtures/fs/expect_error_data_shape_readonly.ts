// expect-error: readonly
import * as fs from "submilli:fs";
function main(): void {
  const stat: fs.Stat = {kind:"file",size:1,modifiedAt:0};
  stat.size = 2;
}
