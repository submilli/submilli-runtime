// Callbacks get TypeScript's parameters, `(value, index, array)` for arrays,
// and may declare fewer of them, so an arrow's parameters need no annotations.

function upper(s: string): string {
  return s.toUpperCase();
}

function apply(f: (x: number, i: number) => number): number {
  return f(10, 2);
}

function fold<U>(f: (acc: U, x: number) => U, initial: U): U {
  return f(f(initial, 1), 2);
}

let moduleCallback: ((x: number, y: number) => number) | null = null;

class Holder {
  callback: ((x: number, y: number) => number) | null = null;
}

interface Greeter {
  greet(n: number, suffix: string): string;
}

class Short implements Greeter {
  greet(n: number): string {
    return "short" + String(n);
  }
}

function arrays(): void {
  const xs = ["a", "b", "c"];
  assert(xs.map((l, i) => l + String(i)).join(",") === "a0,b1,c2", "map index");
  let visited = "";
  xs.forEach((l, i, all) => {
    visited += l + String(i) + String(all.length);
  });
  assert(visited === "a03b13c23", "forEach index and array");
  assert(xs.filter((l, i) => i > 0).join("") === "bc", "filter index");
  assert(xs.findIndex((l, i) => i === 2) === 2, "findIndex index");
  assert(xs.find((l, i) => i === 1) === "b", "find index");
  assert(xs.findLast((l, i) => i < 2) === "b", "findLast index");
  assert(xs.findLastIndex((l) => l === "a") === 0, "findLastIndex, one param");
  assert(xs.some((l, i, all) => all[i] === "c"), "some array");
  assert(xs.every((l, i) => i < 3), "every index");
  assert(xs.flatMap((l, i) => [l, String(i)]).join("") === "a0b1c2", "flatMap index");
  assert([1, 2, 3].reduce((acc, v, i) => acc + v * i, 0) === 8, "reduce index");
  const right = [1, 2, 3].reduceRight((acc, v, i, all) => acc + String(i) + String(all.length), "");
  assert(right === "231303", "reduceRight index and array");
  assert(xs.map((l) => l.length).join(",") === "1,1,1", "one param");
  assert(xs.map(upper).join("") === "ABC", "a declared function with fewer params");
  assert(Array.from(xs, (l, i) => l + String(i)).join(",") === "a0,b1,c2", "Array.from index");
}

function collections(): void {
  const m = new Map<string, number>([["x", 1], ["y", 2]]);
  let entries = "";
  m.forEach((v, k, all) => {
    entries += k + String(v) + String(all.size);
  });
  assert(entries === "x12y22", "Map forEach key and map");
  let values = 0;
  m.forEach((v) => {
    values += v;
  });
  assert(values === 3, "Map forEach, one param");
  const s = new Set<number>([5, 6]);
  let seen = "";
  s.forEach((v, again, all) => {
    seen += String(v) + String(again) + String(all.size);
  });
  assert(seen === "552662", "Set forEach value twice and set");
  const bytes = new Uint8Array([7, 8]);
  assert(bytes.map((b, i) => b + i).join(",") === "7,9", "Uint8Array map index");
  let byteSum = 0;
  bytes.forEach((b, i, all) => {
    byteSum += b * all.length + i;
  });
  assert(byteSum === 31, "Uint8Array forEach index and array");
}

function slots(): void {
  assert(apply((x) => x + 1) === 11, "fewer params into a parameter");
  assert(apply((x, i) => x * i) === 20, "all params");
  const g: (a: string, b: number) => string = (a) => a;
  assert(g("k", 1) === "k", "fewer params into a binding");
  const handler: { run: (a: number, b: number) => number } = { run: (a) => a * 10 };
  assert(handler.run(4, 5) === 40, "into an object field");
  const fns: ((x: number, y: number) => number)[] = [(x) => x, (x, y) => x + y];
  assert(fns.map((f) => f(1, 2)).join(",") === "1,3", "into an array");
  // Narrowed after a write to the declared member, which takes both arguments.
  let optional: ((x: number, y: number) => number) | null = null;
  optional = (x) => x * 3;
  assert(optional !== null && optional(2, 9) === 6, "assigned to a nullable local");
  const initialized: ((x: number, y: number) => number) | null = (x) => x + 1;
  assert(initialized !== null && initialized(1, 2) === 2, "a nullable local's initializer");
  let either: ((x: number, y: number) => number) | string = "s";
  either = (x) => x * 5;
  assert(typeof either !== "string" && either(2, 0) === 10, "assigned to a union local");
  moduleCallback = (x) => x * 6;
  assert(moduleCallback !== null && moduleCallback(2, 0) === 12, "assigned to a module let");
  const holder = new Holder();
  holder.callback = (x) => x * 7;
  assert(holder.callback !== null && holder.callback(2, 0) === 14, "assigned to a field");
  const greeters: Greeter[] = [new Short()];
  assert(greeters[0].greet(1, "!") === "short1", "a method with fewer params");
}

function inference(): void {
  // The initial value binds the accumulator's type before the callback is typed,
  // even where the call's result goes to `unknown`.
  console.log([1, 2].reduce((a, b) => a + b, 0));
  assert(fold((acc, x) => acc + x, 0) === 3, "generic function, callback first");
  assert(fold((acc, x) => acc + String(x), "") === "12", "binds a string accumulator");
  // An annotated parameter binds the accumulator before `[]` is typed.
  const lengths = ["a", "bb"].reduce((acc: number[], x) => acc.concat([x.length]), []);
  assert(lengths.join(",") === "1,2", "binds from a partly annotated callback");
  // A callback with nothing for its context to type is checked in order.
  assert(pick(() => [1, 2], []).length === 0, "binds from a callback without parameters");
  // Deferred through an alias, a nullable slot, parentheses or `function`.
  assert(foldAlias([1, 2], (acc, x) => acc + x, 10) === 13, "aliased callback type");
  assert(foldNullable([1, 2], (acc, x) => acc + x, 0) === 3, "nullable callback type");
  assert(foldAlias([1, 2], ((acc, x) => acc * x), 1) === 2, "parenthesized callback");
  assert(foldAlias([1, 2], function (acc, x) { return acc - x; }, 0) === -3, "function expression");
}

function pick<T>(f: () => T, fallback: T): T {
  return fallback;
}

type Folder<T, U> = (acc: U, x: T) => U;

function foldAlias<T, U>(xs: T[], f: Folder<T, U>, init: U): U {
  let acc = init;
  for (const x of xs) {
    acc = f(acc, x);
  }
  return acc;
}

function foldNullable<T, U>(xs: T[], f: ((acc: U, x: T) => U) | null, init: U): U {
  let acc = init;
  for (const x of xs) {
    acc = f === null ? acc : f(acc, x);
  }
  return acc;
}

function main(): void {
  arrays();
  collections();
  slots();
  inference();
}
