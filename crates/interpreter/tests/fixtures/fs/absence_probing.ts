import { stat, exists, mkdir, writeText, readText } from "submilli:fs";
function main(): void {
  mkdir("/real");
  writeText("/real/file.txt", "x");
  for (const path of ["/real/nope.txt", "/no/such/dir/f.txt", "/real/file.txt/child"]) {
    assert(!exists(path));
    let failed = false;
    try { stat(path); } catch (e) { failed = true; assert(e.message.includes("does not exist")); }
    assert(failed, "stat throws for missing paths");
  }
  assert(stat("/real/file.txt").size === 1);
  let failed = false;
  try { readText("/real/nope.txt"); } catch (e) {
    failed = true;
    assert(e.message.includes("file does not exist"));
    assert(!e.message.includes("parent directory"));
  }
  assert(failed);
}
