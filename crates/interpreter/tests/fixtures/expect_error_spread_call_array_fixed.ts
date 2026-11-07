// expect-error: A spread argument must either have a tuple type or be passed to a rest parameter
// expect-error-count: 1
function pair(a: number, b: number): number { return a + b; }
function main(): void {
  const values: number[] = [1, 2];
  pair(...values);
}
