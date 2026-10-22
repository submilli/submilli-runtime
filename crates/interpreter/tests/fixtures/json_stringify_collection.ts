// A `Map` serializes as the object of its entries, keys in their `String(key)`
// form. A `Set` has no JSON form: the failure is a catchable `TypeError` naming
// the conversion, not a host-side invariant message.
function main(): void {
  const m = new Map<string, number>();
  m.set("a", 1);
  m.set("b", 2);
  assert(JSON.stringify(m) === "{\"a\":1,\"b\":2}", "entries in insertion order");
  assert(JSON.stringify({ inner: m }) === "{\"inner\":{\"a\":1,\"b\":2}}", "nested in an object");
  assert(JSON.stringify([m]) === "[{\"a\":1,\"b\":2}]", "inside an array");
  assert(JSON.stringify(new Holder()) === "{\"entries\":{\"x\":1}}", "as a class field");
  const keys = new Map<number | null, string | undefined>([
    [1, "one"],
    [null, "none"],
    [2, undefined],
  ]);
  assert(JSON.stringify(keys) === "{\"1\":\"one\",\"null\":\"none\"}", "String(key); undefined left out");
  assert(JSON.stringify(new Map<string, number>()) === "{}", "empty");
  const flags = new Map<boolean, number>([[true, 1], [false, 0]]);
  assert(JSON.stringify(flags) === "{\"true\":1,\"false\":0}", "boolean keys");
  // Keys with one string form are each written; a parser keeps the last.
  const clash = new Map<number | string, number>([[1, 1], ["1", 2]]);
  assert(JSON.stringify(clash) === "{\"1\":1,\"1\":2}", "colliding keys");
  const moved = new Map<string, number>([["a", 1], ["b", 2]]);
  moved.delete("a");
  moved.set("a", 3);
  assert(JSON.stringify(moved) === "{\"b\":2,\"a\":3}", "insertion order after a delete");
  assert(JSON.stringify(m, null, 1) === "{\n \"a\": 1,\n \"b\": 2\n}", "space argument");
  // A Map that holds itself reaches the serializer's depth bound, not the stack.
  const looped = new Map<string, unknown>();
  looped.set("self", looped);
  let loopMessage = "";
  try {
    JSON.stringify(looped);
    assert(false, "a Map holding itself should not serialize");
  } catch (e: RangeError) {
    loopMessage = e.message;
  }
  assert(loopMessage.length > 0, "the depth bound is a catchable RangeError");
  assert(JSON.stringify(m) === "{\"a\":1,\"b\":2}", "and the next stringify still works");

  const s = new Set<number>();
  s.add(7);
  let setMessage = "";
  try {
    JSON.stringify(s);
    assert(false, "stringifying a Set should not return");
  } catch (e: TypeError) {
    setMessage = e.message;
  }
  assert(setMessage.includes("`Set` has no JSON representation"), "names the type");
  assert(setMessage.includes("Array.from"), "names the conversion");

  // A Set nested inside plain data fails the same way.
  let deep = "";
  try {
    JSON.stringify({ x: [{ y: s }] });
    assert(false, "a Set at depth 3 should not serialize");
  } catch (e) {
    deep = e.message;
  }
  assert(deep === setMessage, "a Set nested three levels down");

  assert(String(m) === "[object Map]", "String() answers without misreading the backing");

  // The conversion the message names actually works.
  assert(JSON.stringify(Array.from(m)) === "[[\"a\",1],[\"b\",2]]", "entries serialize");
  assert(JSON.stringify(Array.from(s)) === "[7]", "set elements serialize");
}

class Holder {
  entries: Map<string, number> = new Map<string, number>([["x", 1]]);
}
