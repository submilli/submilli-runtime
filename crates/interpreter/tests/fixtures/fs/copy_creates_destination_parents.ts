// A recursive copy creates the destination's parent chain, the way `create_dir_all` did
// before the destination became a handle-relative path.
import { mkdir, writeText, copy, readText, exists } from "submilli:fs";

function main(): void {
  mkdir("/src/inner", true);
  writeText("/src/inner/a.txt", "alpha");

  copy("/src", "/out/build/dest", true);

  assert(exists("/out/build/dest/inner/a.txt"), "nested destination created");
  assert(readText("/out/build/dest/inner/a.txt") === "alpha", "contents copied");
}
