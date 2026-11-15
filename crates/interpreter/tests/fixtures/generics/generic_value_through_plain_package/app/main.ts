import { load, Reader } from "@test/gen";
import { callWith, viaReader } from "@test/plain";
import session from "submilli:session";

function rejects(run: () => void): boolean {
  try {
    run();
  } catch (e: TypeError) {
    return e.message.indexOf("does not match") >= 0;
  }
  return false;
}

function main(): void {
  session.set("num", 1);
  session.set("str", "x");
  const bound: (key: string) => number | undefined = load;
  assert(bound("num") === 1, "an imported generic value bound to a plain type");
  assert(rejects(() => { bound("str"); }), "is checked");

  assert(callWith(load, "num") === 1, "a generic value passed to a plain package");
  assert(rejects(() => { callWith(load, "str"); }), "is checked there");

  const reader = new Reader();
  assert(viaReader((key: string): number | undefined => reader.read<number>(key), "num") === 1, "a generic method wrapped for a plain package");
}
