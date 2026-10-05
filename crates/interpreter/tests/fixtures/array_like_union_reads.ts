// A union of arrays and tuples shares one runtime representation, so reads
// through it see any member's element: indexing, destructuring, `for-of`, and
// the array methods that only hand elements out.
type Column = string[] | number[];

function show(value: number | string | null): string {
  return value === null ? "null" : String(value);
}

function describe(column: Column): string {
  const parts: string[] = [];
  for (const cell of column) {
    parts.push(typeof cell === "string" ? "s:" + cell : "n:" + cell.toFixed(1));
  }
  const [first, ...rest] = column;
  return parts.join(",") + " first=" + show(first) + " rest=" + String(rest.length) +
    " at1=" + show(column[1]) + " slice=" + column.slice(1).join("|") +
    " mapped=" + column.map((cell) => show(cell) + "!").join("|");
}

function pair(p: [number, string] | null[]): string {
  const [a, b] = p;
  let out = show(a) + "/" + show(b) + "/" + show(p[0]) + "/" + show(p[1]);
  for (const v of p) {
    out += ";" + show(v);
  }
  return out;
}

function total(xs: readonly string[] | readonly number[]): number {
  let n = 0;
  for (const x of xs) {
    n += typeof x === "number" ? x : x.length;
  }
  return n + xs.length;
}

function callbacks(column: Column): string {
  let seen = 0;
  column.forEach((cell, i, all) => {
    seen += all.length;
  });
  const kept = column.filter((cell) => typeof cell === "string" || cell > 1);
  return String(seen) + " " + String(kept.length) +
    " some=" + String(column.some((cell) => cell === "b")) +
    " every=" + String(column.every((cell, i, all) => all[i] === cell)) +
    " find=" + show(column.find((cell) => typeof cell === "number") ?? null) +
    " idx=" + String(column.findIndex((cell) => cell === 3)) +
    " sum=" + column.reduce((acc: string, cell) => acc + show(cell), "");
}

function firstOfEither<T, U>(xs: T[] | U[]): T | U | null {
  for (const x of xs) {
    return x;
  }
  return null;
}

function joinPairs(pairs: [string, number][] | [string, string][]): string {
  let joined = "";
  for (const [k, v] of pairs) {
    joined += k + show(v);
  }
  return joined;
}

type Direction = "up" | "down";

function letters(d: Direction): string {
  let out = "";
  for (const c of d) {
    out += c + ".";
  }
  return out;
}

function main(): void {
  assert(describe(["a", "bb"]) === "s:a,s:bb first=a rest=1 at1=bb slice=bb mapped=a!|bb!", "strings");
  assert(
    describe([1, 2, 3]) === "n:1.0,n:2.0,n:3.0 first=1 rest=2 at1=2 slice=2|3 mapped=1!|2!|3!",
    "numbers",
  );
  assert(pair([7, "seven"]) === "7/seven/7/seven;7;seven", "tuple member");
  assert(pair([null, null]) === "null/null/null/null;null;null", "array member");
  assert(total(["ab", "c"]) === 5 && total([1, 2]) === 5, "readonly members");

  const rows: number[][] | string[][] = [[1], [2, 3]];
  let widths = "";
  for (const row of rows) {
    widths += String(row.length);
  }
  assert(widths === "12", "nested arrays");

  assert(callbacks(["a", "b"]) === "4 2 some=true every=true find=null idx=-1 sum=ab", "string callbacks");
  assert(callbacks([1, 2, 3]) === "9 2 some=false every=true find=1 idx=2 sum=123", "number callbacks");
  assert(show(firstOfEither<number, string>(["x"])) === "x", "generic members");
  assert(letters("down") === "d.o.w.n.", "string literal union");

  const counts: [string, number][] = [["a", 1], ["b", 2]];
  assert(joinPairs(counts) === "a1b2", "union of tuple arrays");
}
