// A line break ends a statement whose last part is an `as T` type, even before
// `[` or `(`: the type stops at the break, and no expression continues past it.
const xs: number[] = [1, 2];
let seen: number[] = [];
const ys = xs as number[]
[0, 1].forEach((n: number): void => { seen.push(n); });
let z: number = 0;
let calls: number = 0;
function main(): void {
  assert(seen.length === 2 && seen[1] === 1, "the `[` line is its own statement");
  z = ys.length as number
  (calls += 1);
  assert(z === 2 && calls === 1, "the `(` line is its own statement");
  const w = ys as number[]
  [w.length].forEach((n: number): void => { seen.push(n); });
  assert(seen.length === 3 && seen[2] === 2, "a declaration ends at the break");
}
