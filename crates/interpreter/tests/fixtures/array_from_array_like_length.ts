// `Array.from({ length: n }, mapFn)` builds `n` elements, as in JavaScript: the
// array-like has no elements to read, so `mapFn` sees `undefined` (typed
// `unknown`) and the index. `length` is read as `ToLength` does.
type Sized = { length: number };

function main(): void {
  assert(Array.from({ length: 3 }, (_, i) => i * 2).join(",") === "0,2,4", "computed elements");

  const holes = Array.from({ length: 2 });
  assert(holes.length === 2 && (holes[0] ?? "empty") === "empty", "no mapFn leaves each element empty");

  assert(Array.from({ length: 2.7 }, (_, i) => i).length === 2, "a fractional length truncates");
  assert(Array.from({ length: -1 }).length === 0, "a negative length is empty");

  const rows = 3;
  const grid = Array.from({ length: rows }, (_, r) => Array.from({ length: 2 }, (_, c) => r * 2 + c));
  assert(JSON.stringify(grid) === "[[0,1],[2,3],[4,5]]", "a length from a constant, nested");

  const seen = Array.from({ length: 2 }, (v: unknown, i: number) => v ?? i);
  assert(seen.join(",") === "0,1", "the element is empty");

  const optional: (number | undefined)[] = Array.from({ length: 2 });
  assert((optional[1] ?? "empty") === "empty", "an element type admitting undefined holds the empties");

  const sized: Sized = { length: 2 };
  assert(Array.from(sized, (_, i) => i + 1).join(",") === "1,2", "an object type alias is an array-like");

  assert(Array.from("xy", (c, i) => c + String(i)).join("") === "x0y1", "a string is still iterated");
}
