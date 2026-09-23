// A binding declared `readonly` stays readonly through every narrowing: a
// null check or `!` on a non-nullable one, `??` and `||`, and a write of a
// fresh mutable array into it, as a statement or inside a condition, even
// when the written value is itself nullable. Each line matches a `tsc --strict` error.
// expect-error-count: 10
// expect-error: cannot call `push` on `readonly number[]`
// expect-error: cannot assign to an element of `readonly number[]`
// expect-error: expected `number[]`, got `readonly number[]`

let shared: readonly number[] = [];

function maybeList(present: boolean): number[] | null {
  return present ? [1] : null;
}

function isNumbers(v: readonly number[] | string): v is readonly number[] {
  return typeof v !== "string";
}

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
  let maybe: readonly number[] | null = null;
  if ((maybe = [0, 1]) !== null) {
    maybe.push(1);
  }
  let polled: readonly number[] | null = null;
  if ((polled = maybeList(true)) !== null) {
    polled.push(2);
  }
  let either2: readonly number[] | string = "s";
  if (isNumbers((either2 = [3]))) {
    either2.push(4);
  }
  console.log(mutable.length, rebound.length);
}
