// A union's Wasm slot is nullable when some member *lowers* nullable, not when
// one is spelled `null`. A type variable, `unknown`, or an interface reference
// all hold null after instantiation without naming it, so a union mixing one
// with a differently-lowered member needs a nullable slot — a non-null one
// traps on the write.

interface Named {
  name: string;
}

class Cell {
  constructor(readonly v: number) {}
}

function withNumber<T>(x: T, y: number): string {
  let cell: T | number = y;
  cell = x;
  return cell === null ? "null" : "value";
}

function withString<T>(x: T, y: string): string {
  let cell: T | string = y;
  cell = x;
  return cell === null ? "null" : "value";
}

function withBigInt<T>(x: T, y: bigint): string {
  let cell: T | bigint = y;
  cell = x;
  return cell === null ? "null" : "value";
}

function withBytes<T>(x: T, y: Uint8Array): string {
  let cell: T | Uint8Array = y;
  cell = x;
  return cell === null ? "null" : "value";
}

function withClass<T>(x: T, y: Cell): string {
  let cell: T | Cell = y;
  cell = x;
  return cell === null ? "null" : "value";
}

function unknownWithNumber(x: unknown, y: number): string {
  let cell: unknown | number = y;
  cell = x;
  return cell === null ? "null" : "value";
}

function interfaceWithNumber(x: Named | null, y: number): string {
  let cell: Named | number | null = y;
  cell = x;
  return cell === null ? "null" : "value";
}

// The lowerings the fix must leave alone: a union of two non-null reference
// members, and one that collapses to a primitive slot.
function refOnly(flag: boolean): string {
  const cell: string | Cell = flag ? "s" : new Cell(1);
  return typeof cell === "string" ? cell : "cell";
}

function literalsOnly(flag: boolean): number {
  const cell: 1 | 2 = flag ? 1 : 2;
  return cell;
}

// Member kinds beyond the primitives above.
function withBool<T>(x: T, y: boolean): string {
  let cell: T | boolean = y;
  cell = x;
  return cell === null ? "null" : "value";
}

function withArray<T>(x: T, y: number[]): string {
  let cell: T | number[] = y;
  cell = x;
  return cell === null ? "null" : "value";
}

function withObjectLiteral<T>(x: T, y: { a: number }): string {
  let cell: T | { a: number } = y;
  cell = x;
  return cell === null ? "null" : "value";
}

function withFunction<T>(x: T, y: (n: number) => number): string {
  let cell: T | ((n: number) => number) = y;
  cell = x;
  return cell === null ? "null" : "value";
}

function withWiderUnion<T>(x: T, y: number): string {
  let cell: T | number | string = y;
  cell = x;
  return cell === null ? "null" : "value";
}

// Slot kinds beyond a plain `let`: parameter, return, array element, object
// field, `Map` value, class field, and a vtable method slot.
class Holder<T> {
  field: T | number = 0;

  constructor(readonly seeded: T | number) {}

  take(v: T | number): string {
    return v === null ? "null" : "value";
  }

  give(): T | number {
    return this.field;
  }
}

class Narrower extends Holder<string | null> {
  take(v: (string | null) | number): string {
    return v === null ? "null" : "value";
  }
}

function asParam<T>(v: T | number): string {
  return v === null ? "null" : "value";
}

function asReturn<T>(x: T): T | number {
  return x;
}

function asArrayElement<T>(x: T): string {
  const a: (T | number)[] = [1, x];
  return a[1] === null ? "null" : "value";
}

function asObjectField<T>(x: T): string {
  const o: { v: T | number } = { v: x };
  return o.v === null ? "null" : "value";
}

function asMapValue<T>(x: T): string {
  const m = new Map<string, T | number>();
  m.set("k", x);
  const got = m.get("k");
  return got === null ? "null" : "value";
}

// The newly-nullable union still narrows and still casts.
function narrowsByTypeof<T>(x: T, y: number): string {
  let cell: T | number = y;
  cell = x;
  if (cell === null) {
    return "null";
  }
  return typeof cell === "number" ? "number" : "other";
}

function castsThrough<T>(x: T, y: number): number {
  const cell: T | number = y;
  return cell as number;
}

function main(): void {
  assert(withNumber<string | null>(null, 1) === "null", "T | number holds null");
  assert(withNumber<string | null>("s", 1) === "value", "T | number holds a value");
  assert(withString<string | null>(null, "a") === "null", "T | string holds null");
  assert(withString<number>(3, "a") === "value", "T | string holds a number");
  assert(withBigInt<string | null>(null, 1n) === "null", "T | bigint holds null");
  assert(withBytes<string | null>(null, new Uint8Array(1)) === "null", "T | Uint8Array holds null");
  assert(withClass<string | null>(null, new Cell(1)) === "null", "T | C holds null");
  assert(withClass<string | null>("s", new Cell(1)) === "value", "T | C holds a value");
  assert(unknownWithNumber(null, 1) === "null", "unknown | number holds null");
  assert(unknownWithNumber("s", 1) === "value", "unknown | number holds a value");
  assert(interfaceWithNumber(null, 1) === "null", "interface | number holds null");
  assert(interfaceWithNumber({ name: "n" }, 1) === "value", "interface | number holds a value");
  assert(refOnly(true) === "s", "reference-only union keeps its non-null slot");
  assert(refOnly(false) === "cell", "reference-only union other branch");
  assert(literalsOnly(true) === 1, "literal-only union keeps its primitive slot");
  assert(literalsOnly(false) === 2, "literal-only union other branch");
  assert(withBool<string | null>(null, true) === "null", "T | boolean holds null");
  assert(withArray<string | null>(null, [1]) === "null", "T | number[] holds null");
  assert(withObjectLiteral<string | null>(null, { a: 1 }) === "null", "T | object holds null");
  assert(
    withFunction<string | null>(null, (n: number): number => n) === "null",
    "T | function holds null",
  );
  assert(withWiderUnion<string | null>(null, 1) === "null", "T | number | string holds null");
  assert(withWiderUnion<string | null>("s", 1) === "value", "T | number | string holds a value");

  assert(asParam<string | null>(null) === "null", "parameter slot holds null");
  assert(asReturn<string | null>(null) === null, "return slot holds null");
  assert(asArrayElement<string | null>(null) === "null", "array element slot holds null");
  assert(asObjectField<string | null>(null) === "null", "object field slot holds null");
  assert(asMapValue<string | null>(null) === "null", "Map value slot holds null");

  const holder = new Holder<string | null>(null);
  assert(holder.seeded === null, "constructor parameter slot holds null");
  assert(holder.give() === 0, "class field keeps its initializer");
  holder.field = null;
  assert(holder.give() === null, "class field slot holds null");
  const throughVtable: Holder<string | null> = new Narrower(null);
  assert(throughVtable.take(null) === "null", "vtable method slot holds null");
  assert(throughVtable.take(1) === "value", "vtable method slot holds a value");

  assert(narrowsByTypeof<string | null>(null, 1) === "null", "narrowing still sees null");
  assert(narrowsByTypeof<string | null>("s", 1) === "other", "narrowing still sees the value");
  assert(castsThrough<string | null>(null, 7) === 7, "`as` out of the nullable union");
}
