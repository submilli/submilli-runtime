function dynamic(value: unknown): string | undefined {
  return JSON.stringify(value);
}

type Callback = () => number;

function callbackOrNumber(value: Callback | number): string | undefined {
  return JSON.stringify(value);
}

function serializeObject<T>(value: { value: T }): string {
  return JSON.stringify(value);
}

function main(): void {
  assert(JSON.stringify(undefined) === undefined, "undefined has no JSON document");
  assert(dynamic(undefined) === undefined, "erased undefined has no JSON document");
  assert(dynamic(null) === "null", "null remains JSON null");
  assert(JSON.stringify(() => 1) === undefined, "function has no JSON document");
  assert(dynamic(() => 1) === undefined, "erased function has no JSON document");
  const callback: Callback = () => 1;
  assert(JSON.stringify(callback) === undefined, "function alias has no JSON document");
  assert(callbackOrNumber(callback) === undefined, "function union has no JSON document");
  assert(callbackOrNumber(1) === "1", "serializable union member has a document");
  assert(JSON.stringify({ a: undefined, b: null }) === '{"b":null}', "only undefined is omitted");
  const values: unknown[] = [undefined, null, 1];
  assert(JSON.stringify(values) === "[null,null,1]", "array positions are preserved");
  const absent: { a?: string } = {};
  const present: { a?: string } = { a: undefined };
  assert(JSON.stringify(absent) === "{}", "absent property");
  assert(JSON.stringify(present) === "{}", "present undefined property");
  assert(!("a" in absent), "serialization does not add a missing property");
  assert("a" in present, "serialization does not delete an undefined property");
  assert(JSON.stringify(undefined, undefined, 2) === undefined, "pretty top-level undefined");
  assert(JSON.stringify({ a: 1 }, undefined, undefined) === '{"a":1}', "undefined optional arguments");
  const short: [number, string?] = [1];
  const explicit: [number, string?] = [1, undefined];
  assert(JSON.stringify(short) === "[1]", "optional tuple stays short");
  assert(JSON.stringify(explicit) === "[1,null]", "explicit undefined tuple slot");
  const nested: { missing?: string; value: [number, string?] } = { value: short };
  assert(JSON.stringify(nested) === '{"value":[1]}', "typed nested tuple preserves omitted slot");
  assert(serializeObject({ value: undefined }) === "{}", "generic object remains serializable after erasure");
  const erased: unknown = { value: 1 };
  if (typeof erased === "object" && erased !== null) {
    const serialized = JSON.stringify(erased);
    assert(serialized === '{"value":1}', "narrowed object result remains a string after erasure");
  }
}
