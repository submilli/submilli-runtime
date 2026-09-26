// Every write path through a readonly array or tuple is rejected, including
// through an alias, `ReadonlyArray<T>`, a generic element type, an optional
// chain, and an assignment used as a value. Each line matches a `tsc --strict`
// error.
// expect-error-count: 19
// expect-error: cannot assign to an element of `readonly number[]`
// expect-error: cannot assign to an element of `Scores`
// expect-error: cannot assign to an element of `readonly T[]`
// expect-error: cannot assign to an element of `readonly [number, string]`
// expect-error: cannot call `push` on `readonly number[]`
// expect-error: cannot call `pop` on `readonly number[]`
// expect-error: cannot call `shift` on `readonly number[]`
// expect-error: cannot call `unshift` on `readonly number[]`
// expect-error: cannot call `splice` on `readonly number[]`
// expect-error: cannot call `sort` on `readonly number[]`
// expect-error: cannot call `reverse` on `readonly number[]`
// expect-error: cannot call `fill` on `readonly number[]`
// expect-error: cannot call `copyWithin` on `readonly number[]`
// expect-error: cannot call `push` on `Scores`
// expect-error: cannot call `push` on `readonly T[]`
type Scores = ReadonlyArray<number>;

function fill<T>(xs: readonly T[], x: T): void {
  xs[0] = x;
  xs.push(x);
}

function nullable(xs: readonly number[]): readonly number[] | null { return xs; }

function main(): void {
  const ro: readonly number[] = [1, 2, 3];
  ro[0] = 1;
  ro[0] += 1;
  ro[0]++;
  const assigned = (ro[0] = 5);
  ro.push(4);
  ro.pop();
  ro.shift();
  ro.unshift(0);
  ro.splice(0, 1);
  ro.sort();
  ro.reverse();
  ro.fill(0, 0, 1);
  ro.copyWithin(0, 1, 2);
  const scores: Scores = ro;
  scores[1] = 2;
  scores.push(1);
  const pair: readonly [number, string] = [1, "a"];
  pair[0] = 2;
  fill(ro, 1);
  const maybe: readonly number[] | null = nullable(ro);
  maybe?.push(1);
  console.log(assigned);
}
