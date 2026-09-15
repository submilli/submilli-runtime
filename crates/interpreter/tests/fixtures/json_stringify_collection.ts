// A `Map` / `Set` has no JSON form. The failure is a catchable `TypeError`
// naming the conversion, not a host-side invariant message.
function main(): void {
  const m = new Map<string, number>();
  m.set("a", 1);

  let mapMessage = "";
  try {
    JSON.stringify(m);
    assert(false, "stringifying a Map should not return");
  } catch (e) {
    mapMessage = e.message;
  }
  assert(mapMessage.includes("`Map` has no JSON representation"), "names the type");
  assert(mapMessage.includes("Array.from"), "names the conversion");

  const s = new Set<number>();
  s.add(7);
  let setMessage = "";
  try {
    JSON.stringify(s);
    assert(false, "stringifying a Set should not return");
  } catch (e: TypeError) {
    setMessage = e.message;
  }
  assert(setMessage.includes("`Set` has no JSON representation"), "Set too, as a TypeError");

  // A Map nested inside a plain object fails the same way, not silently.
  let nestedMessage = "";
  try {
    JSON.stringify({ inner: m });
    assert(false, "a nested Map should not serialize");
  } catch (e) {
    nestedMessage = e.message;
  }
  assert(nestedMessage === mapMessage, "the nested failure is the same one");

  // Every route the walk can reach the collection by fails the same way.
  let inArray = "";
  try {
    JSON.stringify([m]);
    assert(false, "a Map inside an array should not serialize");
  } catch (e) {
    inArray = e.message;
  }
  assert(inArray === mapMessage, "a Map inside an array");

  let deep = "";
  try {
    JSON.stringify({ x: [{ y: s }] });
    assert(false, "a Set at depth 3 should not serialize");
  } catch (e) {
    deep = e.message;
  }
  assert(deep === setMessage, "a Set nested three levels down");

  let onClass = "";
  try {
    JSON.stringify(new Holder());
    assert(false, "a Map held by a class should not serialize");
  } catch (e) {
    onClass = e.message;
  }
  assert(onClass === mapMessage, "a Map as a class field");

  // `toString` does not share `toJson`'s problem — it never reads the payload.
  assert(String(m) === "[object Object]", "String() answers without misreading the backing");

  // The conversion the message names actually works.
  assert(JSON.stringify(Array.from(m)) === "[[\"a\",1]]", "entries serialize");
  assert(JSON.stringify(Array.from(s)) === "[7]", "set elements serialize");
}

class Holder {
  entries: Map<string, number> = new Map<string, number>();
}
