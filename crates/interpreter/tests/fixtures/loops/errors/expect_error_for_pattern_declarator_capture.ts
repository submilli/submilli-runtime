// expect-error: a closure captures `k`, which this `for` loop's destructuring initializer declares
function main(): void {
  const fns: Array<() => number> = [];
  for (let [i] = [0], k = 0; i < 2; i++) {
    fns.push(() => k);
  }
}
