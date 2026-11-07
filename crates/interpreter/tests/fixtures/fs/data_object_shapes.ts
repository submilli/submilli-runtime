import * as fs from "submilli:fs";
interface Sized { size: number }
function main(): void {
  fs.writeText("/shape.txt", "abc");
  const stat = fs.stat("/shape.txt")!;
  assert(stat.toString() === "[object Object]", "ordinary object toString");
  const record = stat as Record<string, unknown>;
  assert(record === stat, "record cast preserves identity");
  assert(record["size"] === 3, "indexed number is boxed");
  const subset = stat as Sized;
  assert(subset === stat && subset.size === 3, "structural subset preserves identity");
  assert(Object.keys(stat).sort().join(",") === "kind,modifiedAt,size", "stat keys");
  assert(Object.values(stat).includes(3), "stat values");
  assert(Object.entries(stat).length === 3, "stat entries");
  assert(JSON.stringify(stat).includes('"size":3'), "stat JSON");
  const literal: fs.Stat = { kind: "file", size: 2, modifiedAt: 0 };
  assert(literal.size === 2, "structural assignment");
  const preview = fs.peek("/shape.txt") as Record<string, unknown>;
  assert(preview["size"] === 3 && preview["preview"] === "abc", "peek fields");
  const info = fs.info();
  assert((info as Record<string, unknown>)["mode"] === info.mode, "info shape");
  for (const entry of fs.list("/")) {
    assert((entry as Record<string, unknown>)["name"] === entry.name, "directory entry shape");
  }
}
