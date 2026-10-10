// expect-error: binding `x` is already declared in this scope
// expect-error-count: 1
function outer(x: number): number {
  function x(): number {
    return 1;
  }
  return 0;
}
function main(): void {}
