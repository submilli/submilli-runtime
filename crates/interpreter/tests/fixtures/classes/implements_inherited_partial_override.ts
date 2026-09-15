// Overriding one interface method must not be what makes the *others* work.
// Each slot is installed independently, and a third level exercises reuse of a
// grandparent-owned adapter alongside a parent-owned overridden one.
interface Container {
  get(): number;
  tag(): string;
}

class Base implements Container {
  get(): number {
    return 7;
  }
  tag(): string {
    return "base";
  }
}

class Kid extends Base {
  get(): number {
    return 9;
  }
}

class Grandkid extends Kid {}

function read(c: Container): number {
  return c.get();
}

function tag(c: Container): string {
  return c.tag();
}

function main(): void {
  const kid = new Kid();
  assert(read(kid) === 9, "override reached through the interface");
  assert(tag(kid) === "base", "inherited sibling method still reachable");

  const g = new Grandkid();
  assert(read(g) === 9, "parent-owned override, two levels up");
  assert(tag(g) === "base", "grandparent-owned method, two levels up");
}
