// A guard on `obj[key]` narrows later reads of `obj[key]` when `key` holds
// one value for its whole life: a `const`, or a parameter or `let` never
// assigned. This is how TypeScript narrows these reads.
type Thing = { a: string | null; b: number | null };

function upperOf(obj: Record<string, string | null>, key: string): string {
  if (obj[key] !== null) {
    return obj[key].toUpperCase();
  }
  const doubled = key + key;
  if (obj[doubled] === null) {
    return "-";
  }
  return obj[doubled];
}

function lengthOf(obj: Thing, key: "a" | "b"): number {
  if (obj[key] === null) {
    return -1;
  }
  if (typeof obj[key] === "string") {
    return obj[key].length;
  }
  return obj[key] + 100;
}

function element(items: (string | number | null)[], position: number): number {
  let at = position;
  if (items[at] !== null) {
    if (typeof items[at] === "string") {
      return items[at].length;
    }
    return items[at] * 10;
  }
  return 0;
}

function keyInBlock(obj: Record<string, string | null>, flag: boolean): number {
  if (flag) {
    const key = "k";
    if (obj[key] === null) {
      return -1;
    }
    return obj[key].length;
  }
  return 0;
}

function main(): void {
  assert(upperOf({ x: "ab" }, "x") === "AB", "a parameter key");
  assert(upperOf({ x: null, xx: "both" }, "x") === "both" && upperOf({ x: null, xx: null }, "x") === "-", "a `const` key");
  assert(lengthOf({ a: "abc", b: null }, "a") === 3 && lengthOf({ a: null, b: 5 }, "b") === 105, "nested guards");
  assert(lengthOf({ a: null, b: null }, "a") === -1, "an early exit");
  assert(element(["abcd", 2], 0) === 4 && element(["x", 2], 1) === 20 && element([null], 0) === 0, "an element");
  assert(keyInBlock({ k: "kk" }, true) === 2 && keyInBlock({ k: null }, true) === -1, "a key declared in a block");
}
