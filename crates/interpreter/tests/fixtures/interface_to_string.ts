// `.toString()` is available on every non-null value (spec §1.6), including a
// value typed by an interface that doesn't declare it. It dispatches to the
// value's own `toString`, falling back to "[object Object]". `String(p)` and a
// template interpolation take the same path.
interface Point {
  x: number;
}

class Labelled implements Point {
  x: number = 2;
  toString(): string {
    return "Labelled(" + String(this.x) + ")";
  }
}

class Bare implements Point {
  x: number = 3;
}

function describe(p: Point): string {
  return p.toString();
}

function main(): void {
  const literal: Point = { x: 1 };
  assert(literal.toString() === "[object Object]", "an object literal prints the default");
  assert(describe(new Labelled()) === "Labelled(2)", "a class's own toString is called");
  assert(describe(new Bare()) === "[object Object]", "a class without one prints the default");
  const labelled: Point = new Labelled();
  assert(String(labelled) === "Labelled(2)", "String() dispatches the same way");
  assert(`<${labelled}>` === "<Labelled(2)>", "so does template interpolation");
  assert(`${literal}` === "[object Object]", "an interpolated literal prints the default");
}
