// A conditional's type is the union of its branches' types reduced by tsc's
// strict subtype relation: a branch whose type is a subtype of the other's
// drops out, and a function stays apart from one taking a wider parameter.
type Narrow = { f(x: string | undefined): string };
type Wide = { f(x?: string): string };

function pick(x: Narrow, y: Wide, first: boolean): string {
  // Typed `Wide`: `Narrow` is its subtype, so `z.f()` may omit the argument.
  const z = first ? x : y;
  return z.f() + z.f("s");
}

function both(first: boolean): string {
  // `(x: number) => string` and `(x: Object) => string` stay a union, called
  // through the parameter type both accept.
  const g = first ? (x: number) => `n${x}` : (x: Object) => `o${String(x)}`;
  return g(1);
}

function main(): void {
  const narrow: Narrow = { f: (x: string | undefined) => x ?? "-" };
  const wide: Wide = { f: (x?: string) => x ?? "_" };
  assert(pick(narrow, wide, true) === "-s");
  assert(pick(narrow, wide, false) === "_s");
  assert(both(true) === "n1");
  assert(both(false) === "o1");
}
