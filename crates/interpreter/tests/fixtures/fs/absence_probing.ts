// `stat` and `exists` are the two ways a program asks "is this there?" without trapping.
// Both must stay total over paths that simply are not there — including paths whose
// *ancestor* is missing, and paths that run through a file as if it were a directory.
import { stat, exists, mkdir, writeText, Stat } from "submilli:fs";

function main(): void {
  mkdir("/real", true);
  writeText("/real/file.txt", "x");

  const missingLeaf: Stat | null = stat("/real/nope.txt");
  assert(missingLeaf === null, "stat returns null for a missing leaf");

  const missingAncestor: Stat | null = stat("/no/such/dir/f.txt");
  assert(missingAncestor === null, "stat returns null when an ancestor is missing");

  const throughAFile: Stat | null = stat("/real/file.txt/child");
  assert(throughAFile === null, "stat returns null when a component is a file");

  assert(exists("/real/file.txt"), "exists finds a real file");
  assert(exists("/real/nope.txt") === false, "exists reports a missing leaf as absent");
  assert(exists("/no/such/dir/f.txt") === false, "exists reports a missing ancestor as absent");
  assert(exists("/real/file.txt/child") === false, "exists reports a file-as-directory as absent");
}
