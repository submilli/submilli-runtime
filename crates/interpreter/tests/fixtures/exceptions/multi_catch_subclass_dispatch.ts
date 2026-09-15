// Dispatch respects the class hierarchy: a specific arm before its ancestor
// takes instances of the subclass; the ancestor arm takes the rest, and an
// ancestor-only try still catches subclass instances via the chain walk.
class Base extends Error {
  constructor(m: string) {
    super(m);
    this.name = "Base";
  }
}

class Derived extends Base {
  constructor(m: string) {
    super(m);
    this.name = "Derived";
  }
}

function pick(deep: boolean): string {
  try {
    if (deep) {
      throw new Derived("d");
    }
    throw new Base("b");
  } catch (e: Derived) {
    return "derived";
  } catch (e: Base) {
    return "base";
  }
}

function main(): void {
  assert(pick(true) === "derived", "subclass instance takes the specific arm");
  assert(pick(false) === "base", "base instance falls to the ancestor arm");

  let seen = "";
  try {
    throw new Derived("d");
  } catch (e: Base) {
    seen = e.name;
  }
  assert(seen === "Derived", "ancestor arm accepts subclass instances");
}
