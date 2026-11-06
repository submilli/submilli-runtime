// Without an annotation, an array literal whose elements fit no one type holds
// the union of their widened types, as tsc does: `[1, "x"]` is
// `(number | string)[]`. An annotation, a parameter or a tuple hint still
// decides the type when there is one.
interface Txn {
  id: number;
  createdAt: string;
  amount: number;
  note?: string;
}

enum Dir {
  Up,
  Down,
}

class Foo {
  foo: number = 1;
}
class Bar {
  bar: string = "b";
}

function first<T>(items: T[]): T {
  return items[0];
}

let calls = 0;
function bump(): number {
  calls += 1;
  return calls;
}

function show(value: number | string | boolean): string {
  if (typeof value === "number") return `n${value}`;
  if (typeof value === "string") return `s${value}`;
  return `b${value}`;
}

function main(): void {
  const pair = [1, "x"];
  pair.push(2, "y");
  const pairTyped: (number | string)[] = pair;
  assert(pairTyped.map(show).join(",") === "n1,sx,n2,sy", "a number and a string");

  const triple = [1, "x", true];
  const tripleTyped: (number | string | boolean)[] = triple;
  assert(tripleTyped.map(show).join(",") === "n1,sx,btrue", "three primitives");

  const rows = [[1, "x"], [2, "y"]];
  const rowsTyped: (number | string)[][] = rows;
  assert(JSON.stringify(rowsTyped) === '[[1,"x"],[2,"y"]]', "rows of mixed elements");

  const holes = [1, null];
  const holesTyped: (number | null)[] = holes;
  const shapes = [{ a: 1 }, { b: "x" }];
  const shapesTyped: ({ a: number; b?: undefined } | { a?: undefined; b: string })[] = shapes;
  assert(holesTyped[1] === null && shapesTyped[1].b === "x", "null and object literal rows");

  const others = [undefined, 1, "x", 2n, Dir.Down];
  const othersTyped: (undefined | number | string | bigint | Dir)[] = others;
  assert(othersTyped.length === 5 && othersTyped[3] === 2n, "undefined, bigint and enum elements");

  const txns: Txn[] = [{ id: 7, createdAt: "2026-11-06", amount: 12.5 }];
  const summary = txns.map((t) => [t.id, t.createdAt, t.amount]);
  const summaryTyped: (number | string)[][] = summary;
  assert(JSON.stringify(summaryTyped) === '[[7,"2026-11-06",12.5]]', "rows built in a callback");
  const nestedSummary = txns.map((t) => [[t.id, t.createdAt], [t.amount, "total"]]);
  const nestedSummaryTyped: (number | string)[][][] = nestedSummary;
  assert(JSON.stringify(nestedSummaryTyped) === '[[[7,"2026-11-06"],[12.5,"total"]]]', "nested rows");

  // An element that needs a hint takes the earlier elements' type when it then
  // fits. One that doesn't is typed alone, as in tsc, and joins the union.
  const called = txns.map((t) => [t.id, t.createdAt.toUpperCase(), t.amount.toFixed(1)]);
  assert(JSON.stringify(called) === '[[7,"2026-11-06","12.5"]]', "call elements");
  const fallbacks = txns.map((t) => [t.id, t.note?.trim() ?? "", t.id > 0 ? t.amount.toFixed(0) : 0]);
  const fallbacksTyped: (number | string)[][] = fallbacks;
  assert(JSON.stringify(fallbacksTyped) === '[[7,"","13"]]', "operators over calls");
  const table = [["id", "total"], [txns[0].id, Math.round(txns[0].amount)]];
  const tableTyped: (string[] | number[])[] = table;
  assert(JSON.stringify(tableTyped) === '[["id","total"],[7,13]]', "a header row over a call row");
  const columns = [txns.map((t) => t.id), txns.map((t) => t.createdAt)];
  const columnsTyped: (number[] | string[])[] = columns;
  assert(JSON.stringify(columnsTyped) === '[[7],["2026-11-06"]]', "generic calls with callbacks");
  const spreadCall = [1, ...[txns[0].createdAt.slice(0, 4)]];
  const spreadCallTyped: (number | string)[] = spreadCall;
  assert(spreadCallTyped.join(",") === "1,2026", "a spread source holding a call");
  const seen = [{ name: "a", ids: new Set<number>() }, { name: "b", ids: new Set() }];
  seen[1].ids.add(3);
  const byKey = [[new Map<string, number>()], [new Map()]];
  byKey[1][0].set("k", 4);
  assert(seen[1].ids.size === 1 && byKey[1][0].get("k") === 4, "the earlier type when it fits");
  // An element inferred twice still runs once.
  const counted = [["x"], [bump(), bump()]];
  assert(JSON.stringify(counted) === '[["x"],[1,2]]' && calls === 2, "effects run once");
  // An element inferred again alone sees the flow from before the element, so
  // a read before its write sees the type from before the write.
  let cell: string | number = "a";
  const written = [[true], [cell, (cell = 2)]];
  const writtenTyped: ((string | number)[] | boolean[])[] = written;
  assert(JSON.stringify(writtenTyped) === '[[true],["a",2]]', "an element that writes");
  let nextId = 1;
  const buckets = [{ id: nextId++, seen: new Set<string>() }, { id: nextId++, seen: new Set() }];
  buckets[1].seen.add("x");
  assert(nextId === 3 && buckets[1].seen.size === 1, "a writing element that fits the earlier type");
  // Growing an evolving array is a write too.
  let grown = [];
  grown.push("s");
  const grownRows = [[true], [grown, grown.push(1)]];
  const grownRowsTyped: (boolean[] | (number | string[])[])[] = grownRows;
  assert(JSON.stringify(grownRowsTyped) === '[[true],[["s",1],2]]', "an element that grows an array");
  let grownOptionally = [];
  grownOptionally.push("s");
  const optionalRows = [[true], [grownOptionally, grownOptionally.push?.(1)]];
  const optionalRowsTyped: (boolean[] | (number | string[])[])[] = optionalRows;
  assert(JSON.stringify(optionalRowsTyped) === '[[true],[["s",1],2]]', "an optional call that grows an array");
  let grownChained = [];
  grownChained.push("s");
  const chainedRows = [[true], [grownChained, grownChained?.push(1)]];
  const chainedRowsTyped: (boolean[] | (number | string[])[])[] = chainedRows;
  assert(JSON.stringify(chainedRowsTyped) === '[[true],[["s",1],2]]', "an optional chain that grows an array");

  // Arrays of different element types join as a union of array types.
  const nested = [[1], ["x"]];
  const nestedTyped: (number[] | string[])[] = nested;
  assert(nestedTyped.map((a) => a.length).join(",") === "1,1", "arrays of different types");

  const spread = [...[1, 2], "x"];
  const spreadTyped: (number | string)[] = spread;
  const numbers: number[] = [3];
  const strings: string[] = ["z"];
  const spreads: (number | string)[] = [...numbers, ...strings, ...["w"], 4];
  assert(spreadTyped.length === 3 && spreads.join(",") === "3,z,w,4", "spread sources");
  const fromSpreads = [...numbers, ...strings];
  const fromSpreadsTyped: (number | string)[] = fromSpreads;
  assert(fromSpreadsTyped.join(",") === "3,z", "two spreads of different types");

  const valueAndArray = [1, [2]];
  const valueAndArrayTyped: (number | number[])[] = valueAndArray;
  assert(JSON.stringify(valueAndArrayTyped) === "[1,[2]]", "a value and an array");

  const valueAndFunction = [1, () => 2];
  let total = 0;
  for (const item of valueAndFunction) total += typeof item === "number" ? item : item();
  assert(total === 3, "a value and a function");

  const instances = [new Foo(), new Bar()];
  const instancesTyped: (Foo | Bar)[] = instances;
  const names = instancesTyped.map((i) => (i instanceof Foo ? `foo${i.foo}` : `bar${i.bar}`));
  assert(names.join(",") === "foo1,barb", "instances of unrelated classes");

  const byParam = [(x: number) => x, (x: string) => x.length];
  const byParamTyped: (((x: number) => number) | ((x: string) => number))[] = byParam;
  const byResult = [(x: number) => x, (x: number, y: number) => x * y, (): string => "s"];
  const byResultTyped: (((x: number, y: number) => number) | (() => string))[] = byResult;
  assert(byParamTyped.length === 2 && byResultTyped.length === 3, "functions no one type fits");

  // A regular literal type stays, as in tsc.
  const hello: "hello" = "hello";
  const kept = [hello, 1];
  const keptTyped: ("hello" | number)[] = kept;
  assert(keptTyped.join(",") === "hello,1", "a regular literal type");

  const picked = first([1, "x"]);
  const pickedTyped: number | string = picked;
  assert(pickedTyped === 1, "a generic parameter binds the union");

  // A tuple hint keeps the tuple, so each position keeps its own type.
  const tuple: [number, string] = [1, "x"];
  assert(tuple[1].length === 1 && tuple[0] + 1 === 2, "a tuple hint");
}
