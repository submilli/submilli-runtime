// expect-error: cannot read field `length`
function check(x: string | null, skip: boolean): number {
  let n = 0;
  do {
    n = n + 1;
    if (skip) { x = null; continue; }
    if (x === null) { return -1; }
  } while (n < 2 && x.length > 0);
  return n;
}
function main(): void { check("x", true); }
