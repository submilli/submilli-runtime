// The VFS root is a mount point, not an entry. Naming it as the target of a destructive
// or write operation is a mistake the diagnostic has to name — `remove("/", true)` in
// particular would otherwise drain the root and then fail on the self-unlink, destroying
// a volume and reporting the destruction as a failed call.
import { mkdir, writeText, remove, exists, list, DirEntry } from "submilli:fs";

function main(): void {
  writeText("/keep.txt", "x");
  mkdir("/sub", false);

  let removeMessage: string = "";
  try {
    remove("/", true);
  } catch (error: Error) {
    removeMessage = error.message;
  }
  assert(removeMessage.includes("VFS root"), "remove names the root: " + removeMessage);
  assert(exists("/keep.txt"), "the root's entries survive a refused remove");
  assert(exists("/sub"), "the root's subdirectories survive a refused remove");

  let writeMessage: string = "";
  try {
    writeText("/", "x");
  } catch (error: Error) {
    writeMessage = error.message;
  }
  assert(writeMessage.includes("VFS root"), "write names the root: " + writeMessage);

  let strays: number = 0;
  for (const entry of list("/", false)) {
    if (entry.name !== "keep.txt" && entry.name !== "sub") {
      strays = strays + 1;
    }
  }
  assert(strays === 0, "a refused write leaves no temp file behind");
}
