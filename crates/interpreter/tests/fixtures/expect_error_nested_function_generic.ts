// expect-error: a nested function cannot be generic
// expect-error-count: 1
function main(): void {
  function id<T>(x: T): T {
    return x;
  }
}
