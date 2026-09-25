// `get <prop>` is the runtime key an accessor's closure is stored under, but it
// is also an ordinary string: a value can carry a *data* field spelled exactly
// that. The shaped-property read/write branch dispatches on it, so the name
// alone must not decide — only a class declares accessors, and only its payload
// slot holds the getter/setter closure.

interface Foo {
  x?: number;
}

interface Bar {
  y: string;
}

class RealAccessor implements Foo {
  private n: number = 3;
  get x(): number {
    return this.n * this.n;
  }
  set x(v: number) {
    this.n = v;
  }
}

function main(): void {
  // Parsed data whose key collides with the accessor ABI. The member is absent,
  // so the read is `null` and the write creates a data property.
  const collide = JSON.parse('{"get x": 5}') as Foo;
  assert(collide.x === null, "a data field named `get x` is not an accessor");
  collide.x = 7;
  assert(collide.x === 7, "a write creates the absent data property");

  const collideSetter = JSON.parse('{"set x": 5}') as Foo;
  assert(collideSetter.x === null, "same for a data field named `set x`");
  collideSetter.x = 7;
  assert(collideSetter.x === 7, "a setter-like key does not intercept insertion");

  // Both keys at once, plus a real member alongside.
  const both = JSON.parse('{"get x": 1, "set x": 2}') as Foo;
  assert(both.x === null, "neither key makes the property present");

  // An object literal may spell the key too, and even store a closure of the
  // getter's shape under it. It is still not a class, so it is still not an
  // accessor.
  const literal = { "get x": (): number => 42, other: 1 };
  const asFoo: unknown = literal;
  assert((asFoo as Foo).x === null, "an object literal's `get x` closure is not invoked");

  // The real thing still dispatches.
  const real = new RealAccessor();
  const asIface: Foo = real;
  assert(asIface.x === 9, "a class accessor is found through the interface");
  asIface.x = 4;
  assert(asIface.x === 16, "and its setter runs");

  // A required member on a colliding value stays absent-shaped rather than
  // trapping on the accessor cast.
  const barish = JSON.parse('{"y": "real", "get y": 1}') as Bar;
  assert(barish.y === "real", "the data slot wins over a colliding accessor name");
}
