// A `Stat` from `stat` flows as itself: a typed binding, a proven upcast, an
// argument and a closure cast back from `unknown` all read the host's fields.
import { stat, writeText, Stat } from "submilli:fs";

function sizeOf(s: Stat): number {
  return s.size;
}

function main(): void {
  writeText("/f.txt", "abc");
  const found = stat("/f.txt");
  assert(found !== undefined, "stat finds the file");
  const s: Stat = found!;
  const upcast = s as Stat;
  assert(s.kind === "file", "the binding reads the host's kind");
  assert(sizeOf(upcast) === 3, "the upcast passes as an argument");
  const lookup = (p: string): Stat | undefined => stat(p);
  const held: unknown = lookup;
  const back = held as (p: string) => Stat | undefined;
  assert(back("/f.txt")!.size === 3, "a closure cast keeps its host-typed signature");
}
