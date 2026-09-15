// `JSON.stringify` branches on `ref.is_null` before vtable dispatch, because
// dispatching on a null receiver traps at `struct.get $Object 0`. Which values
// need that branch is decided by whether the type *admits* null — `unknown`, a
// type variable, and any union holding one can be instantiated at a nullable
// type — not by whether a member is spelled `null`.
//
// Two emitters ask the same question: the direct-value path here, and the
// object-shape `toJson` body that serializes a field.

interface Named {
  name: string;
}

function ofUnknown(x: unknown): string {
  return JSON.stringify(x);
}

function ofTypeVar<T>(x: T): string {
  return JSON.stringify(x);
}

function ofErasedUnion<T>(x: T, y: number): string {
  let cell: T | number = y;
  cell = x;
  return JSON.stringify(cell);
}

function ofField<T>(x: T): string {
  const o: { v: T | number } = { v: x };
  return JSON.stringify(o);
}

function ofUnknownField(x: unknown): string {
  const o: { v: unknown } = { v: x };
  return JSON.stringify(o);
}

function ofNamedField(x: Named | null): string {
  const o: { v: Named | null } = { v: x };
  return JSON.stringify(o);
}

function main(): void {
  assert(ofUnknown(null) === "null", "unknown holding null");
  assert(ofUnknown(3) === "3", "unknown holding a number");
  assert(ofUnknown("s") === "\"s\"", "unknown holding a string");
  assert(ofTypeVar<string | null>(null) === "null", "type variable holding null");
  assert(ofTypeVar<string | null>("s") === "\"s\"", "type variable holding a value");
  assert(ofErasedUnion<string | null>(null, 1) === "null", "T | number holding null");
  assert(ofErasedUnion<string | null>("s", 1) === "\"s\"", "T | number holding a value");
  assert(ofField<string | null>(null) === "{\"v\":null}", "T | number field holding null");
  assert(ofField<string | null>("s") === "{\"v\":\"s\"}", "T | number field holding a value");
  assert(ofUnknownField(null) === "{\"v\":null}", "unknown field holding null");
  assert(ofUnknownField(2) === "{\"v\":2}", "unknown field holding a number");
  assert(ofNamedField(null) === "{\"v\":null}", "the spelled-null case still works");
  assert(ofNamedField({ name: "n" }) === "{\"v\":{\"name\":\"n\"}}", "spelled-null field with a value");
  // Arrays already went through the element-wise validator, which handles null.
  const arr: unknown[] = [null, 1];
  assert(JSON.stringify(arr) === "[null,1]", "array elements unchanged");
}
