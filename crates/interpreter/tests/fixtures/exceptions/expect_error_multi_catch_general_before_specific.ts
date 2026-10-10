// expect-error: unreachable `catch` clause: `Derived` extends `Base`

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

function main(): void {
  try {
    throw new Derived("x");
  } catch (e: Base) {
    assert(true, "general arm");
  } catch (e: Derived) {
    assert(false, "shadowed by the Base arm");
  }
}
