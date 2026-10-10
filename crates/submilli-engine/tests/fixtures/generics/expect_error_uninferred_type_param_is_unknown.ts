// A type parameter nothing infers is `unknown`, so its values must be
// narrowed before use, as in tsc.
// expect-error: cannot apply `*` to `unknown`
// expect-error-count: 1
function first<T>(n: number): T | null {
  return null;
}

function main(): void {
  const x = first(1);
  if (x !== null) {
    const doubled: number = x * 2;
  }
}
