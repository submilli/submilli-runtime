// A generic call whose callback returns several literals holds their union in
// an array result, as tsc infers it, so another value is rejected.
// expect-error: expected `1 | 2`, got `9`
// expect-error-count: 1
function makeAll<T>(f: () => T): T[] {
  return [f()];
}

function main(): void {
  const flag = Math.random() > 2;
  const all = makeAll(() => {
    if (flag) {
      return 1;
    }
    return 2;
  });
  all.push(9);
}
