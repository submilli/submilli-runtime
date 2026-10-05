// `default` comes before the case declaring `x`, so its read of `x` is above
// that declaration in the shared switch scope (TS2448), not the outer `x`.
// expect-error: cannot access `x` before its initialization
// expect-error-count: 1
const x = "outer";
function pick(kind: number): string {
  switch (kind) {
    default:
      return x;
    case 1:
      const x = "inner";
      return x;
  }
}

function main(): void {
  console.log(pick(1));
}
