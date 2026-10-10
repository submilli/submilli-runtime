// A clause's `const` shadows an outer `x` across the whole `switch` body, so an
// earlier clause reading `x` reads it before its declaration (TS2448).
// expect-error: cannot access `x` before its initialization
// expect-error-count: 1
const x = "outer";
function pick(kind: number): string {
  switch (kind) {
    case 1:
      return x;
    default:
      const x = "inner";
      return x;
  }
}
function main(): void {}
