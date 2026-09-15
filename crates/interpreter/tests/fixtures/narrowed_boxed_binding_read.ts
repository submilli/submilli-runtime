// A join-installed narrowing that resolves to a *boxed* binding reads the
// payload out of the box cell, then casts only when the payload's Wasm type
// differs from the narrowed type's. A primitive payload — the `f64` a
// number-literal union lands in — is already the narrowed lowering, and the
// cast's leading `ref.as_non_null` would not validate against it.
//
// Every binding here is assigned inside a `for` / `for-of` body, which is a
// capture frame, so it boxes (a `while` body is not); the `if` before each read
// installs the narrowing on the original ident rather than on a shadow.

class Cell {
  constructor(readonly v: number) {}
}

class Sub extends Cell {
  constructor(readonly extra: number) {
    super(extra);
  }
}

interface Named {
  name: string;
}

enum Color {
  Red = 1,
  Blue = 2,
}

type MaybeNum = number | null;

type Shape = { kind: "circle"; r: number } | { kind: "square"; s: number };

function isCell(v: unknown): v is Cell {
  return v instanceof Cell;
}

function literalNumbers(arr: number[]): number {
  let latest: 1 | 2 = 1;
  for (const n of arr) {
    latest = 2;
  }
  if (latest === 1) {
    return -1;
  }
  return latest;
}

function boxedBoolean(arr: number[]): boolean {
  let flag: boolean | null = null;
  for (const n of arr) {
    flag = n > 0;
  }
  if (flag === null) {
    return false;
  }
  return flag;
}

function plainNumber(arr: number[]): number {
  let total: number | null = null;
  for (const n of arr) {
    total = n;
  }
  if (total === null) {
    return -1;
  }
  return total + 1;
}

function literalStrings(arr: number[]): string {
  let mode: "off" | "on" = "off";
  for (const n of arr) {
    mode = "on";
  }
  if (mode === "off") {
    return "none";
  }
  return mode;
}

function nullableClass(arr: number[]): number {
  let cell: Cell | null = null;
  for (const n of arr) {
    cell = new Cell(n);
  }
  if (cell === null) {
    return -1;
  }
  return cell.v;
}

function erasedSlot<T>(arr: number[], seed: T): string {
  let held: T | null = null;
  for (const n of arr) {
    held = seed;
  }
  if (held === null) {
    return "empty";
  }
  return typeof held === "string" ? "held" : "other";
}

function mixedUnion(arr: number[]): string {
  let value: string | number = 0;
  for (const n of arr) {
    value = "set";
  }
  if (typeof value === "number") {
    return "number";
  }
  return value;
}

// Narrowing forms other than `=== null`.
function byNotNull(arr: number[]): number {
  let total: number | null = null;
  for (const n of arr) {
    total = n;
  }
  return total !== null ? total + 1 : -1;
}

function byTruthiness(arr: number[]): number {
  let total: number | null = null;
  for (const n of arr) {
    total = n;
  }
  return total ? total + 1 : -1;
}

function byInstanceof(arr: number[]): number {
  let v: Cell = new Cell(0);
  for (const n of arr) {
    v = new Sub(n);
  }
  return v instanceof Sub ? v.extra : -1;
}

function byDiscriminant(arr: number[]): number {
  let sh: Shape = { kind: "square", s: 2 };
  for (const n of arr) {
    sh = { kind: "circle", r: n };
  }
  return sh.kind === "circle" ? sh.r : sh.s;
}

function byIsArray(arr: number[]): number {
  let v: number[] | number = 5;
  for (const n of arr) {
    v = [n, n];
  }
  return Array.isArray(v) ? v.length : v;
}

function byPredicate(arr: number[]): number {
  let v: unknown = 1;
  for (const n of arr) {
    v = new Cell(n);
  }
  return isCell(v) ? v.v : -1;
}

// Binding kinds and capture frames beyond a `let` in a `for-of` body.
function boxedParam(v: number | null, arr: number[]): number {
  for (const n of arr) {
    v = n;
  }
  return v === null ? -1 : v + 1;
}

function classicFor(k: number): number {
  let total: number | null = null;
  for (let i = 0; i < k; i = i + 1) {
    total = i;
  }
  return total === null ? -1 : total + 1;
}

function readInNestedClosure(arr: number[]): number {
  let total: number | null = null;
  for (const n of arr) {
    total = n;
  }
  const read = (): number => (total === null ? -1 : total + 1);
  return read();
}

function branchBodyShadow(arr: number[]): number {
  let v: number | null = null;
  for (const n of arr) {
    v = n;
  }
  if (v !== null) {
    const inner = v + 1;
    return inner;
  }
  return -1;
}

// Narrowed types the primitive/reference split above doesn't reach.
function boxedBigInt(arr: number[]): bigint {
  let v: bigint | null = null;
  for (const n of arr) {
    v = 2n;
  }
  return v === null ? -1n : v + 1n;
}

function boxedBytes(arr: number[]): number {
  let v: Uint8Array | null = null;
  for (const n of arr) {
    v = new Uint8Array(n);
  }
  return v === null ? -1 : v.length;
}

function boxedArray(arr: number[]): number {
  let v: number[] | null = null;
  for (const n of arr) {
    v = [n, n];
  }
  return v === null ? -1 : v.length;
}

function boxedEnum(arr: number[]): string {
  let v: Color | null = null;
  for (const n of arr) {
    v = Color.Blue;
  }
  return v === null ? "none" : v === Color.Blue ? "blue" : "red";
}

function boxedInterface(arr: number[]): string {
  let v: Named | null = null;
  for (const n of arr) {
    v = { name: "n" };
  }
  return v === null ? "none" : v.name;
}

function boxedUnknown(arr: number[]): string {
  let v: unknown = 1;
  for (const n of arr) {
    v = "s";
  }
  return typeof v === "string" ? v : "other";
}

function aliasedNullable(arr: number[]): number {
  let v: MaybeNum = null;
  for (const n of arr) {
    v = n;
  }
  return v === null ? -1 : v + 1;
}

// The intersection with the union-lowering rule: the *narrowed* type is itself
// a union that lowers nullable because one member is erased.
function narrowToErasedUnion<T>(arr: number[], seed: T): string {
  let v: T | number | null = null;
  for (const n of arr) {
    v = seed;
  }
  if (v === null) {
    return "none";
  }
  return typeof v === "number" ? "number" : "erased";
}

function main(): void {
  assert(literalNumbers([9]) === 2, "narrowed number-literal union out of a box");
  assert(literalNumbers([]) === -1, "unnarrowed branch still reads the box");
  assert(boxedBoolean([9]) === true, "nullable boolean narrows to the i32 payload");
  assert(boxedBoolean([]) === false, "boolean box null branch");
  assert(plainNumber([4]) === 5, "nullable number narrows to the f64 payload");
  assert(plainNumber([]) === -1, "null branch of the boxed number");
  assert(literalStrings([9]) === "on", "narrowed string-literal union out of a box");
  assert(literalStrings([]) === "none", "string box unnarrowed");
  assert(nullableClass([7]) === 7, "boxed class reference still needs its cast");
  assert(nullableClass([]) === -1, "boxed class null branch");
  assert(erasedSlot<string>([1], "s") === "held", "erased payload narrows through the box");
  assert(erasedSlot<string>([], "s") === "empty", "erased payload null branch");
  assert(mixedUnion([1]) === "set", "mixed union narrows out of the box");
  assert(mixedUnion([]) === "number", "mixed union number branch");
  assert(byNotNull([4]) === 5, "`!== null` narrows the same box");
  assert(byNotNull([]) === -1, "`!== null` else branch");
  assert(byTruthiness([4]) === 5, "truthiness narrows the box");
  assert(byTruthiness([]) === -1, "truthiness else branch");
  assert(byInstanceof([7]) === 7, "`instanceof` narrows a boxed class to a subclass");
  assert(byInstanceof([]) === -1, "`instanceof` else branch");
  assert(byDiscriminant([3]) === 3, "discriminant narrows a boxed object union");
  assert(byDiscriminant([]) === 2, "discriminant other arm");
  assert(byIsArray([2]) === 2, "`Array.isArray` narrows the box");
  assert(byIsArray([]) === 5, "`Array.isArray` else branch");
  assert(byPredicate([6]) === 6, "a user type guard narrows a boxed `unknown`");
  assert(byPredicate([]) === -1, "user type guard else branch");
  assert(boxedParam(null, [4]) === 5, "a captured-and-mutated *parameter* boxes too");
  assert(boxedParam(null, []) === -1, "boxed parameter null branch");
  assert(classicFor(3) === 3, "a `for (let …)` body is a capture frame as well");
  assert(classicFor(0) === -1, "classic-for null branch");
  assert(readInNestedClosure([4]) === 5, "the box read happens inside a closure");
  assert(readInNestedClosure([]) === -1, "nested-closure read null branch");
  assert(branchBodyShadow([4]) === 5, "branch-body shadow rather than the join path");
  assert(branchBodyShadow([]) === -1, "branch-body else");
  assert(boxedBigInt([1]) === 3n, "boxed bigint");
  assert(boxedBigInt([]) === -1n, "boxed bigint null branch");
  assert(boxedBytes([3]) === 3, "boxed Uint8Array");
  assert(boxedBytes([]) === -1, "boxed Uint8Array null branch");
  assert(boxedArray([1]) === 2, "boxed array");
  assert(boxedArray([]) === -1, "boxed array null branch");
  assert(boxedEnum([1]) === "blue", "boxed enum");
  assert(boxedEnum([]) === "none", "boxed enum null branch");
  assert(boxedInterface([1]) === "n", "boxed interface");
  assert(boxedInterface([]) === "none", "boxed interface null branch");
  assert(boxedUnknown([1]) === "s", "boxed unknown narrowed by typeof");
  assert(boxedUnknown([]) === "other", "boxed unknown other branch");
  assert(aliasedNullable([4]) === 5, "an alias for the nullable type peels the same way");
  assert(aliasedNullable([]) === -1, "aliased nullable null branch");
  assert(narrowToErasedUnion<string>([1], "s") === "erased", "narrowed to a nullable-lowering union");
  assert(narrowToErasedUnion<string>([], "s") === "none", "erased-union null branch");
}
