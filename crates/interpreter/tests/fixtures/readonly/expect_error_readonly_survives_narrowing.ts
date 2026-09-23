// A binding declared `readonly` stays readonly through every narrowing: a
// null check or `!` on a non-nullable one, `??` and `||`, and a write of a
// fresh mutable array into it. Each line matches a `tsc --strict` error.
// expect-error-count: 7
// expect-error: cannot call `push` on `readonly number[]`
// expect-error: cannot assign to an element of `readonly number[]`
// expect-error: expected `number[]`, got `readonly number[]`

let shared: readonly number[] = [];

function main(): void {
  const ro: readonly number[] = [1, 2, 3];
  if (ro !== null) {
    ro.push(4);
  }
  ro!.push(4);
  ro![0] = 5;
  const mutable: number[] = ro!;
  const either = ro || [0];
  either.push(5);

  let rebound: readonly number[] = [1];
  rebound = [2];
  rebound.push(3);
  shared = [1];
  shared.push(2);
  console.log(mutable.length, rebound.length);
}
