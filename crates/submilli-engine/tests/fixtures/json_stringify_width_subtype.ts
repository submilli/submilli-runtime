// `JSON.stringify` serializes the fields an object has at run time, not only
// the ones its static type names, as JavaScript does. A value assigned through
// a narrower type keeps its extra fields, and `.toJson()`, `Object.keys`, and a
// value seen as `unknown` already reported them.

interface Point {
  x: number;
  label?: string | null;
}

function viaParam(p: Point): string {
  return JSON.stringify(p);
}

function main(): void {
  const wide = { y: 2, x: 1 };
  const narrow: { x: number } = wide;
  assert(JSON.stringify(narrow) === "{\"x\":1,\"y\":2}", "an object type keeps extra fields");
  assert(JSON.stringify(narrow) === narrow.toJson(), "agrees with toJson");

  const extra = { x: 3, z: true, label: null };
  const point: Point = extra;
  assert(
    JSON.stringify(point) === "{\"label\":null,\"x\":3,\"z\":true}",
    "an interface keeps extra fields, and a present null",
  );
  const other = { x: 4, w: "w" };
  assert(viaParam(other) === "{\"w\":\"w\",\"x\":4}", "a parameter keeps extra fields");
  assert(JSON.stringify({ x: 5 }) === "{\"x\":5}", "an absent optional field is omitted");
  assert(
    JSON.stringify(narrow, null, 1) === "{\n \"x\": 1,\n \"y\": 2\n}",
    "the pretty-printing form keeps extra fields too",
  );
}
