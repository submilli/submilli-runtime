// A class test (`instanceof`, a `catch` annotation) names a class: a local
// interface doesn't hide a class of its name, and a local alias of a class
// names the class. A `catch` annotation and `instanceof` through an alias are
// Submilli's, not TypeScript's; this matches their top-level forms.

class Shape {
  kind: string = "shape";
}

function errorCode(e: unknown): string {
  interface Error { code: number; }
  const local: Error = { code: 7 };
  try {
    throw new TypeError("bad");
  } catch (caught: Error) {
    return e instanceof Error ? `${caught.message} ${local.code}` : "no";
  }
}

function boxed(x: unknown): boolean {
  type Crate = Shape;
  return x instanceof Crate;
}

function main(): void {
  assert(errorCode(new RangeError("x")) === "bad 7", "the prelude Error class wins");
  assert(boxed(new Shape()) && !boxed(1), "a local alias of a class");
}
