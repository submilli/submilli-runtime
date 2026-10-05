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
}
