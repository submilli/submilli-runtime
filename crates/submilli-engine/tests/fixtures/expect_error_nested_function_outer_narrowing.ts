// A function declaration is hoisted, so, as in TypeScript, its body sees the
// declared type of an outer variable, not how it is narrowed where declared.
// expect-error: cannot read field `length` on `string | null`
// expect-error-count: 1
function len(x: string | null): number {
  const y = x;
  if (y !== null) {
    function g(): number {
      return y.length;
    }
    return g();
  }
  return 0;
}
function main(): void {}
