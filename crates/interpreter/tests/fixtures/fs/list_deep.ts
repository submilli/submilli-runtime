// A recursive walk of a tree nested deeper than the walker's open-handle cap (32) has to
// postpone subdirectories and reopen them from the base handle. That branch must still
// yield every entry exactly once, with correct paths, and keep each subtree together.

import { mkdir, writeText, list, DirEntry } from "submilli:fs";

const DEPTH: number = 35;

function chain(branch: string): string {
  let path: string = "/" + branch;
  for (let i = 1; i <= DEPTH; i = i + 1) {
    path = path + "/l" + i.toString();
  }
  return path;
}

function build(branch: string): void {
  mkdir(chain(branch), true);
  let path: string = "/" + branch;
  writeText(path + "/f.txt", "x");
  for (let i = 1; i <= DEPTH; i = i + 1) {
    path = path + "/l" + i.toString();
    writeText(path + "/f.txt", "x");
  }
}

function main(): void {
  build("a");
  build("b");

  const paths: string[] = [];
  const kinds: string[] = [];
  for (const entry of list("/", true)) {
    paths.push(entry.path);
    kinds.push(entry.kind);
  }

  // Per branch: DEPTH + 1 directories and one `f.txt` in each of them.
  const expected: number = 2 * (2 * (DEPTH + 1));
  assert(
    paths.length === expected,
    "deep walk yields every entry: expected " +
      expected.toString() +
      ", got " +
      paths.length.toString(),
  );

  const seen = new Set<string>();
  for (const p of paths) {
    seen.add(p);
  }
  assert(seen.size === paths.length, "no entry is yielded twice");

  const before = new Set<string>();
  for (let i = 0; i < paths.length; i = i + 1) {
    const p = paths[i];
    const parent = p.substring(0, p.lastIndexOf("/"));
    assert(
      parent === "" || before.has(parent),
      "parent must be yielded before " + p,
    );
    if (kinds[i] === "file") {
      assert(p.endsWith("/f.txt"), "unexpected file path " + p);
    }
    before.add(p);
  }

  // Every subtree is one consecutive run: nothing from outside a directory's subtree is
  // interleaved into it, however deep the directory sits.
  for (let i = 0; i < paths.length; i = i + 1) {
    if (kinds[i] !== "directory") {
      continue;
    }
    const prefix = paths[i] + "/";
    let first = -1;
    let last = -1;
    let count = 0;
    for (let j = 0; j < paths.length; j = j + 1) {
      if (paths[j].startsWith(prefix)) {
        if (first < 0) {
          first = j;
        }
        last = j;
        count = count + 1;
      }
    }
    if (count === 0) {
      continue;
    }
    assert(first > i, "descendants of " + paths[i] + " must follow it");
    assert(
      last - first + 1 === count,
      "entries from outside " + paths[i] + " are interleaved into its subtree",
    );
  }
}
