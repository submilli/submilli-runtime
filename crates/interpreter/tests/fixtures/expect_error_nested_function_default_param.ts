// expect-error: a nested function's parameters cannot have default values
// expect-error-count: 1
function main(): void {
  function inc(x: number, by: number = 1): number {
    return x + by;
  }
}
