// `filter`, `find`, `findIndex`, `some` and `every` test their callback's
// result for truthiness, as in JavaScript, so a callback may return any
// value: tsc types their callbacks as returning `unknown`. A typed array's
// `find` and `findIndex` are the exception and still want a `boolean`.
interface Item {
  name: string | null;
  count: number;
}

function main(): void {
  const names: (string | null)[] = ["a", null, "", "b"];
  assert(names.filter((x) => x).join(",") === "a,b", "filter keeps truthy strings");
  assert(names.find((x) => x && x.length) === "a", "find stops at a truthy number");
  assert(names.findLast((x) => x) === "b", "findLast scans from the end");
  assert(names.findIndex((x) => x) === 0, "findIndex");
  assert(names.findLastIndex((x) => x === null ? 0 : 1) === 3, "findLastIndex");
  assert(names.some((x) => x), "some sees a truthy value");
  assert(!names.every((x) => x), "every sees a falsy one");

  const counts = [0, 1, 2, 0];
  assert(counts.filter((n) => n).length === 2, "zero is falsy");
  assert(counts.findIndex((n) => n) === 1, "findIndex skips zero");

  const items: Item[] = [
    { name: null, count: 0 },
    { name: "x", count: 3 },
  ];
  assert(items.filter((item) => item.name).length === 1, "a nullable field");
  assert(items.every((item) => item), "an object is truthy");

  const bytes = new Uint8Array([0, 7, 0, 9]);
  assert(bytes.filter((b) => b).length === 2, "Uint8Array filter");
  assert(bytes.some((b) => b) && !bytes.every((b) => b), "Uint8Array some and every");
  assert(bytes.findLastIndex((b) => b) === 3, "Uint8Array findLastIndex");

  const strict = names.filter((x): x is string => x !== null);
  assert(strict.length === 3, "a type guard still narrows");
}
