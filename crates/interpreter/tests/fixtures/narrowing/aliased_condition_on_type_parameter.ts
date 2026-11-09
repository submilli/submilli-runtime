// Testing a `const` that aliases a type-parameter value narrows the value too
// (see aliased_conditions_narrow.ts). Two nullish checks in a row each narrow
// the parameter further, and the second must refine the first's result, not
// start over from the declared `T`: the branch where neither check holds is
// reachable, and reads the value as `T & {}`.
function orderedNull<T>(o: T): number {
  const h = o;
  return h === null || h === undefined ? 0 : 1;
}

function orderedUndefined<T>(o: T): number {
  const h = o;
  return h === undefined || h === null ? 0 : 1;
}

function nestedTernary<T>(o: T): number {
  const h = o;
  return h === undefined ? 0 : h === null ? 0 : 1;
}

function ifElse<T>(o: T): string {
  const h = o;
  if (h === undefined || h === null) {
    return "nullish";
  } else {
    return "value";
  }
}

function boundedByObject<T extends object>(o: T): number {
  const h = o;
  return h === null || h === undefined ? 0 : 1;
}

function boundedByRecord<T extends Record<string, number>>(o: T): number {
  const h = o;
  return h === undefined ? 0 : h === null ? 0 : 1;
}

// The narrowed parameter is usable in the branch both checks leave.
function readsParameter<T>(o: T, fallback: T): T {
  const h = o;
  if (h === null || h === undefined) {
    return fallback;
  }
  const present: {} = h;
  return present === fallback ? fallback : o;
}

// An alias of an alias tests the same value.
function aliasChain<T>(o: T): number {
  const a = o;
  const b = a;
  return b === null || b === undefined ? 0 : 1;
}

// A `const` destructured from a discriminated value narrows with it: the
// sibling's view is joined with what an earlier check left.
type Tagged<T> = { kind: "x"; p: T } | { kind: "y"; p: T; q: number };

function destructuredSibling<T>(a: Tagged<T>): number {
  const { kind, p } = a;
  if (p === null || p === undefined) {
    return 0;
  }
  return kind === "x" ? 1 : 2;
}

function main(): void {
  const none: number | null | undefined = undefined;
  const empty: number | null | undefined = null;

  assert(orderedNull(1) === 1, "orderedNull value");
  assert(orderedNull(none) === 0, "orderedNull undefined");
  assert(orderedNull(empty) === 0, "orderedNull null");

  assert(orderedUndefined(1) === 1, "orderedUndefined value");
  assert(orderedUndefined(none) === 0, "orderedUndefined undefined");
  assert(orderedUndefined(empty) === 0, "orderedUndefined null");

  assert(nestedTernary("a") === 1, "nestedTernary value");
  assert(nestedTernary(none) === 0, "nestedTernary undefined");
  assert(nestedTernary(empty) === 0, "nestedTernary null");

  assert(ifElse(0) === "value", "ifElse value");
  assert(ifElse(none) === "nullish", "ifElse undefined");
  assert(ifElse(empty) === "nullish", "ifElse null");

  assert(boundedByObject({ a: 1 }) === 1, "boundedByObject");
  assert(boundedByRecord({ a: 1 }) === 1, "boundedByRecord");

  assert(readsParameter(5, 9) === 5, "readsParameter value");
  assert(readsParameter(none, 9) === 9, "readsParameter undefined");

  assert(aliasChain(1) === 1, "aliasChain value");
  assert(aliasChain(none) === 0, "aliasChain undefined");
  assert(aliasChain(empty) === 0, "aliasChain null");

  assert(destructuredSibling<number>({ kind: "x", p: 1 }) === 1, "sibling x");
  assert(destructuredSibling<number>({ kind: "y", p: 1, q: 2 }) === 2, "sibling y");
  assert(destructuredSibling<number | null>({ kind: "y", p: null, q: 2 }) === 0, "sibling null");
}
