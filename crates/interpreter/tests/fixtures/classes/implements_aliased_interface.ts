// An aliased interface in `implements` position is the interface: conformance is
// checked against it, and the class registers as an implementor so an
// interface-typed receiver dispatches to it.
interface Named {
  name: string;
  greet(): string;
}

interface Container<T> {
  get(): T;
}

type N2 = Named;
type N3 = N2;
// A generic interface aliased with its argument already applied, and one that
// carries the parameter through to the implementing class.
type StringContainer = Container<string>;
type Carrier<T> = Container<T>;

class Person implements N2 {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  greet(): string {
    return "hi " + this.name;
  }
}

class Robot implements N3 {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  greet(): string {
    return "beep " + this.name;
  }
}

class Boxed implements StringContainer {
  private v: string;
  constructor(v: string) {
    this.v = v;
  }
  get(): string {
    return this.v;
  }
}

class Carried<T> implements Carrier<T> {
  private v: T;
  constructor(v: T) {
    this.v = v;
  }
  get(): T {
    return this.v;
  }
}

function describe(n: Named): string {
  return n.greet();
}

function unwrap<T>(c: Container<T>): T {
  return c.get();
}

function main(): void {
  const p: Person = new Person("ada");
  const r: Robot = new Robot("r2");
  assert(describe(p) === "hi ada", "aliased implements dispatches");
  assert(describe(r) === "beep r2", "twice-aliased implements dispatches");
  const asIface: Named = p;
  assert(asIface.name === "ada", "field read through the interface");

  assert(
    unwrap<string>(new Boxed("v")) === "v",
    "generic interface aliased with its argument applied",
  );
  assert(
    unwrap<number>(new Carried<number>(7)) === 7,
    "generic class implementing a generic alias",
  );
}
