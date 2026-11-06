// An array element inferred again alone sees the flow from before it, so a read
// before a write in it sees the variable's type from before the write, as in tsc.
// expect-error: cannot read field `length` on non-object type `number`
// expect-error-count: 1
function label(n: number, s: string): string {
  return s + n;
}

function main(): void {
  let v: string | number = 5;
  const row = [true, label(v.length, (v = "x"))];
  console.log(row.length);
}
