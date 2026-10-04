import { list, set } from "submilli:session";

function main(): void {
  for (const n of [1024, 2048]) {
    let caught = false;
    try { list("", 1, "x".repeat(n)); }
    catch (e: RangeError) { caught = e.message.includes("cursor"); }
    assert(caught, "oversized cursor fails before decoding");
  }
  set("a".repeat(256), 1);
  set("b".repeat(256), 2);
  const first = list("", 1, null);
  assert(first.nextCursor !== null, "full-length key has a cursor");
  const second = list("", 1, first.nextCursor);
  assert(second.entries.length === 1, "a maximum legitimate cursor resumes");
}
