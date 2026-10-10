import { Base, Container } from "@test/base";

class Kid extends Base {}

class Loud extends Base {
  tag(): string {
    return "loud";
  }
}

// Two local levels off the imported parent: the slot's owner is the imported
// class at both levels, so `GrandKid` must reuse the adapter `Kid` emitted
// rather than emit a third identical one.
class GrandKid extends Kid {}

function read(c: Container): number {
  return c.get();
}

function tag(c: Container): string {
  return c.tag();
}

function main(): void {
  const kid = new Kid(2);
  assert(read(kid) === 2, "imported parent's method through an interface");
  assert(tag(kid) === "base", "second imported method");

  const loud = new Loud(3);
  assert(read(loud) === 3, "inherited from the imported parent");
  assert(tag(loud) === "loud", "overridden locally");

  const g = new GrandKid(4);
  assert(read(g) === 4, "two local levels below an imported parent");
  assert(tag(g) === "base", "second imported method, two levels down");
}
