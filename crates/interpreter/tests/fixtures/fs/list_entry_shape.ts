// Pins the guest-visible shape of `list` entries: `path` is always rooted at `/`, carries
// the listed directory's prefix, and does not change when the listed path is spelled with
// a trailing slash. `name` is the basename, `size` is 0 for anything but a file.

import { mkdir, writeText, list, DirEntry } from "submilli:fs";

function joined(path: string, recursive: boolean): string {
  const paths: string[] = [];
  for (const entry of list(path, recursive)) {
    paths.push(entry.path);
  }
  paths.sort();
  return paths.join(",");
}

function main(): void {
  mkdir("/tree/sub", true);
  writeText("/tree/a.txt", "alpha");
  writeText("/tree/sub/b.txt", "bravo");

  let files = 0;
  let dirs = 0;
  for (const entry of list("/tree", true)) {
    if (entry.kind === "file") {
      files = files + 1;
      assert(entry.size === 5, "file entries carry their size");
      assert(
        entry.path === "/tree/a.txt" || entry.path === "/tree/sub/b.txt",
        "unexpected file path " + entry.path,
      );
      assert(
        entry.name === "a.txt" || entry.name === "b.txt",
        "name is the basename, got " + entry.name,
      );
    } else {
      dirs = dirs + 1;
      assert(entry.kind === "directory", "unexpected kind " + entry.kind);
      assert(entry.path === "/tree/sub", "directory path is guest-rooted");
      assert(entry.name === "sub", "directory name is the basename");
      assert(entry.size === 0, "directories report size 0");
    }
  }
  assert(files === 2 && dirs === 1, "recursive walk sees both files and the subdirectory");

  assert(
    joined("/tree", true) === "/tree/a.txt,/tree/sub,/tree/sub/b.txt",
    "recursive listing of a nested base",
  );
  assert(
    joined("/tree/", true) === joined("/tree", true),
    "a trailing slash lists the same directory",
  );
  assert(joined("/tree", false) === "/tree/a.txt,/tree/sub", "flat listing stops at children");
  assert(joined("/", false) === "/tree", "listing the root needs no doubled slash");
  assert(
    joined("/", true) === "/tree,/tree/a.txt,/tree/sub,/tree/sub/b.txt",
    "recursive listing from the root",
  );
}
