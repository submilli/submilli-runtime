// The first clause's `x` shadows the outer `x` across the whole switch body, and
// a later clause entered directly reaches it before its declaration has run
// (TS2454).
// expect-error: `x` is declared in another `case` clause
// expect-error-count: 1
const x = "outer";
function pick(kind: number): string {
  switch (kind) {
    case 1:
      const x = "inner";
      return x;
    default:
      return x;
  }
}

function main(): void {
  console.log(pick(2));
}
