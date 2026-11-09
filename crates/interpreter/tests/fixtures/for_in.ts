// `for (k in obj)` iterates the object's own enumerable string keys. Key order
// is `Object.keys` order. Where the object model differs from Node's order
// (SUB-2125, SUB-2168), the tests compare sorted keys.
class Base {
  x: number = 1;
  y: string = "s";
  method(): number {
    return 1;
  }
}

class Derived extends Base {
  z: boolean = true;
}

function sortedKeys(label: string, keys: string[]): string {
  return label + ":" + keys.sort().join(",");
}

function recordKeys(o: Record<string, number>): string[] {
  const out: string[] = [];
  for (const k in o) out.push(k);
  return out;
}

function countGeneric<T extends object>(o: T): number {
  let count = 0;
  for (const k in o) count++;
  return count;
}

function sumGeneric<T extends Record<string, number>>(o: T): number {
  let total = 0;
  for (const k in o) total += o[k];
  return total;
}

class Bag {
  a: number = 1;
  b: number = 2;
  total(): number {
    let sum = 0;
    for (const k in this) sum++;
    return sum;
  }
}

function countNullable(o: Record<string, number> | null): number {
  let count = 0;
  for (const k in o) count++;
  return count;
}

function growWhileEnumerating(): number[] {
  const out = [];
  const o: Record<string, number> = { a: 1, b: 2 };
  for (let round = 0; ; round++) {
    if (round > 1) break;
    for (const k in o) out.push(o[k] + round);
  }
  return out;
}

function countKeys(o: Record<string, number> | undefined): number {
  let count = 0;
  for (const k in o) count++;
  return count;
}

let evaluations = 0;
function counted(): Record<string, number> {
  evaluations++;
  return { a: 1, b: 2, c: 3 };
}

function main(): void {
  const record: Record<string, boolean> = { b: true, a: false, c: true };
  const index: { [k: string]: number } = {};
  index["q"] = 1;
  index["r"] = 2;
  const literal = { second: 2, first: 1 };
  const list = ["a", "b"];
  const pair: [number, string] = [1, "t"];

  let keys: string[] = [];
  for (const k in record) keys.push(k);
  assert(sortedKeys("record", keys) === "record:a,b,c", "record");

  // Insertion order of non-integer keys is Node's.
  keys = [];
  for (let k in index) {
    keys.push(k);
  }
  assert(keys.join(",") === "q,r", "index signature");

  keys = [];
  for (const k in literal) keys.push(k);
  assert(sortedKeys("literal", keys) === "literal:first,second", "object literal");

  keys = [];
  for (const k in new Derived()) keys.push(k);
  assert(sortedKeys("class", keys) === "class:x,y,z", "own fields, no methods");

  keys = [];
  for (const k in list) keys.push(k);
  assert(keys.join(",") === "0,1", "array indices");

  keys = [];
  for (const k in pair) keys.push(k);
  assert(keys.join(",") === "0,1", "tuple indices");

  let runs = 0;
  for (const k in {}) runs++;
  assert(runs === 0, "empty object runs the body zero times");

  keys = [];
  for (const k in record) {
    if (k === "a") continue;
    keys.push(k);
    if (keys.length === 1) break;
  }
  assert(keys.length === 1 && keys[0] !== "a", "break and continue");

  let outer = "init";
  keys = [];
  for (outer in index) keys.push(outer);
  assert(keys.join(",") === "q,r" && outer === "r", "an outer variable is the loop variable");

  keys = [];
  for (const a in record) {
    for (const b in pair) keys.push(a + b);
  }
  assert(keys.length === 6, "nested for-in");

  let a = "";
  let b = "";
  const spread: Record<string, number> = { x: 1, y: 2 };
  const pushed: string[] = [];
  for (a in spread) {
    for (b in index) {
      for (const c in { m: 1 }) {
        for (a in spread) {
          pushed.push(c + a);
          break;
        }
      }
    }
  }
  assert(
    pushed.join(",") === "mx,mx,mx,mx" && a === "x" && b === "r",
    "shared outer variables and nested lowered loops",
  );

  let shared = "";
  const captured: Array<() => string> = [];
  for (shared in index) captured.push(() => shared);
  assert(captured.map((f) => f()).join(",") === "r,r", "a closure reads the shared outer variable");

  const grown: Record<string, number> = { a: 1, b: 2 };
  keys = [];
  for (const k in grown) {
    keys.push(k);
    grown[k + "x"] = 0;
  }
  assert(keys.join(",") === "a,b", "a key added during the loop is not visited");
  assert(Object.keys(grown).length === 4, "the added keys exist afterwards");

  const readers: Array<() => string> = [];
  for (const k in index) readers.push(() => k);
  assert(readers.map((f) => f()).join(",") === "q,r", "each pass has its own binding");

  const present: Record<string, number> = { q: 5, r: 6 };
  let total = 0;
  for (const k in present) total += present[k];
  assert(total === 11, "obj[k] reads a present key");

  assert(recordKeys({ z: 1, y: 2 }).length === 2, "a Record parameter");

  assert(countKeys(undefined) === 0, "an undefined object has no keys");
  assert(countKeys({ a: 1 }) === 1, "a present Record");

  let seen = 0;
  for (const k in counted()) seen++;
  assert(seen === 3 && evaluations === 1, "the object is evaluated once");

  keys = [];
  for (const k in "ab") keys.push(k);
  assert(keys.join(",") === "0,1", "a string lists its indices, as JavaScript does");

  assert(countGeneric({ a: 1, b: 2 }) === 2, "a type-parameter object");
  assert(sumGeneric({ a: 1, b: 2 }) === 3, "a constrained type parameter");
  assert(new Bag().total() === 2, "this");
  assert(countNullable(null) === 0, "a null object has no keys");
  assert(growWhileEnumerating().join(",") === "1,2,2,3", "for-in grows an evolving array");
}
