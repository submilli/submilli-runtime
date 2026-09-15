import { Alpha, Zed } from "@test/zlib";

class Local extends Alpha {}

function main(): void {
  assert(Alpha.Z.tag === "z", "class-typed static field");
  assert(Alpha.PAIR[0].tag === "p", "class inside a tuple-typed static field");
  assert(Alpha.MAYBE === null, "nullable class-typed static field");

  assert(Alpha.pick(new Zed("q")).tag === "q", "class-typed static method param and return");
  assert(Alpha.tagOf(Alpha.Z) === "z", "static method over the static field");

  // resolved through a local subclass of the imported class
  assert(Local.Z.tag === "z", "static field via a local subclass");
  assert(Local.pick(new Zed("r")).tag === "r", "static method via a local subclass");
}
