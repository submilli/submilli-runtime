// expect-error: function `g` does not return a value on all paths
// expect-error-count: 1
function main(): void {
  function g(y: number): number {
    if (y > 0) {
      return y;
    }
  }
}
