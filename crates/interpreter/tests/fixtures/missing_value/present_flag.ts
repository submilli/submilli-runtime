// SUB-2148: a local typed with a type parameter that holds a missing read
// keeps a present flag. The flag follows the read it holds: a value that
// isn't a missing read is present even when it is `undefined`, a copy of
// another maybe-missing local carries that local's state, and a null
// narrowing never proves the value present.
function fieldAssign<T>(xs: T[], o: { f: T }): T {
  let v = xs[0];
  v = o.f;
  return v;
}

function chainLocal<V>(rs: Record<string, V>[]): V {
  const v = rs[0]?.foo;
  return v;
}

function mixed<T>(xs: (T | string)[], ss: string[]): T | string {
  let v = xs[0];
  const s = ss[0];
  v = s;
  return v;
}

function assignExpr<T>(xs: T[], i: number): T {
  let v = xs[0];
  const w = (v = xs[i]);
  return v;
}

function notNull<T>(xs: T[], d: T): T {
  const v = xs[0];
  if (v !== null) {
    return v;
  }
  return d;
}

function isNull<T>(xs: (T | null)[], d: T): T {
  const v = xs[0];
  if (v === null) {
    return d;
  }
  return v;
}

function recordLocal<V>(r: Record<string, V>, k: string): V {
  const v = r[k];
  return v;
}

function stringIntoUnion<T>(xs: (T | string)[], s: string, i: number): T | string {
  let v = xs[0];
  v = s[i];
  return v;
}

function flaggedCopy<T>(xs: T[], i: number): T {
  const v = xs[i];
  let w = xs[0];
  w = v;
  return w;
}

function loopWritten<T>(xs: T[], n: number): T {
  let v = xs[0];
  const read = (): number => n;
  for (let i = 0; i < read(); i++) {
    v = xs[i];
  }
  return v;
}

function typeofAndTruthy<T>(xs: T[]): string {
  const v = xs[0];
  if (typeof v === "undefined") return "typeof";
  if (!v) return "falsy";
  return "present";
}

function annotatedChain<V>(rs: Record<string, V>[]): V {
  const x: V | undefined = rs[0]?.foo;
  return x;
}

function assignedChain<V>(rs: Record<string, V>[]): V {
  let x: V | undefined;
  x = rs[0]?.foo;
  return x;
}

// A local declared `T | undefined` may hold a real `undefined` beside a
// missing read it took; without a flag, a use typed with room for
// `undefined` reads it as it is.
function nameOr(opts: { name?: string }, parts: string[]): string {
  let name: string | undefined = opts.name;
  if (name === undefined) name = parts[1];
  return name ?? "anon";
}

function findA(xs: string[]): string {
  let found: string | undefined;
  for (let i = 0; i < xs.length; i++) {
    if (xs[i].startsWith("a")) found = xs[i];
  }
  return `found: ${found}`;
}

function cleared(xs: number[]): string {
  let x: number | undefined = xs[0];
  x = undefined;
  return String(x);
}

// A captured local can't hold a read, so it takes a chain as maybe
// `undefined`.
function capChain<V>(rs: Record<string, V>[]): V | undefined {
  const x: V | undefined = rs[0]?.foo;
  const f = (): V | undefined => x;
  return f();
}

// A generic local holding a read is reassigned a chain, which its flag
// can't follow: the chain is read at its own type.
function reassignedChain<V>(xs: V[], rs: Record<string, V>[]): string {
  let v = xs[0];
  v = rs[0]?.foo;
  return `${v}`;
}

// After a branch merge, a local without a flag takes a missing read as
// `undefined` (it may hold a real one); a generic local's flag throws.
function mergedPlain(xs: number[], c: boolean): string {
  let v: number | undefined;
  if (c) v = xs[3];
  return `${v}`;
}

function mergedGeneric<T>(xs: T[], c: boolean): string {
  let v: T | undefined;
  if (c) v = xs[3];
  return `${v}`;
}

function throws(f: () => void): string {
  try {
    f();
  } catch (e: MissingValueError) {
    return e.name;
  }
  return "no throw";
}

function main(): void {
  assert(
    fieldAssign<number | undefined>([], { f: undefined }) === undefined,
    "a field value is present",
  );
  assert(
    chainLocal<number | undefined>([{ foo: undefined }]) === undefined,
    "a chain to a present undefined",
  );
  assert(chainLocal<number>([{ foo: 1 }]) === 1, "a chain to a present value");
  assert(throws(() => chainLocal<number>([])) === "MissingValueError", "a chain to a missing element");
  assert(throws(() => mixed<number>([1], [])) === "MissingValueError", "a copy of a missing local");
  assert(mixed<number>([1], ["s"]) === "s", "a copy of a present local");
  assert(throws(() => assignExpr<number>([1], 5)) === "MissingValueError", "an assignment expression");
  assert(
    assignExpr<number | undefined>([1, undefined], 1) === undefined,
    "an assignment expression of a present undefined",
  );
  assert(throws(() => notNull<number>([], 3)) === "MissingValueError", "!== null keeps the check");
  assert(throws(() => isNull<number>([], 3)) === "MissingValueError", "=== null keeps the check");
  assert(notNull<number | undefined>([undefined], 3) === undefined, "!== null of a present undefined");
  assert(isNull<number>([null], 3) === 3, "=== null of a present null");

  const record: Record<string, number | undefined> = { k: undefined };
  assert(recordLocal(record, "k") === undefined, "a record local holding a present undefined");
  assert(throws(() => recordLocal(record, "zz")) === "MissingValueError", "a record local holding a missing key");
  assert(stringIntoUnion<number>([1], "ab", 1) === "b", "a string read into a generic local");
  assert(throws(() => stringIntoUnion<number>([1], "ab", 5)) === "MissingValueError", "a missing string read");
  assert(flaggedCopy<number | undefined>([undefined], 0) === undefined, "a flagged copy of a present undefined");
  assert(throws(() => flaggedCopy<number>([1], 3)) === "MissingValueError", "a flagged copy of a missing read");
  assert(loopWritten<number | undefined>([1, undefined], 2) === undefined, "a loop-written local");
  assert(throws(() => loopWritten<number>([1], 2)) === "MissingValueError", "a loop-written missing read");
  assert(typeofAndTruthy<number>([]) === "typeof", "typeof on a missing read");
  assert(typeofAndTruthy<number>([0]) === "falsy", "truthiness on a present value");
  assert(annotatedChain<number | undefined>([{ foo: undefined }]) === undefined, "an annotated chain");
  assert(throws(() => annotatedChain<number>([])) === "MissingValueError", "an annotated chain missing");
  assert(assignedChain<number>([{ foo: 2 }]) === 2, "an assigned chain");

  assert(nameOr({}, ["x", "y"]) === "y", "a fallback read");
  assert(nameOr({}, []) === "anon", "a missing fallback read");
  assert(findA(["b", "c"]) === "found: undefined", "a local never assigned");
  assert(findA(["b", "ab"]) === "found: ab", "a local assigned in a loop");
  assert(cleared([1]) === "undefined", "a local cleared to undefined");

  assert(capChain<number>([]) === undefined, "a captured local takes a missing chain");
  assert(capChain<number>([{ foo: 4 }]) === 4, "a captured local takes a present chain");
  assert(reassignedChain<number | undefined>([1], [{ foo: undefined }]) === "undefined", "a chain to a present undefined");
  assert(throws(() => reassignedChain<number>([1], [{}])) === "MissingValueError", "a chain to a missing key");
  assert(mergedPlain([1], true) === "undefined", "a merge without a flag");
  assert(mergedPlain([1], false) === "undefined", "a merge without a flag, unassigned");
  assert(throws(() => mergedGeneric<number>([1], true)) === "MissingValueError", "a merge with a flag");
  assert(mergedGeneric<number>([1], false) === "undefined", "a merge with a flag, unassigned");
}
