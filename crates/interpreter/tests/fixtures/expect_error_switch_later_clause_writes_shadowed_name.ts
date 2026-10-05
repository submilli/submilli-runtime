// Writes reach the first clause's `count`, not the outer one, so a later
// clause entered directly writes it before its declaration has run: Node throws
// a ReferenceError (tsc accepts this).
// expect-error: `count` is declared in another `case` clause
// expect-error-count: 3
function tally(kind: number): number {
  let count = 0;
  switch (kind) {
    case 1:
      let count = 1;
      return count;
    default:
      count = 5;
      count += 1;
      count++;
      return 0;
  }
}

function main(): void {
  console.log(String(tally(2)));
}
