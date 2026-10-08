// expect-error: a closure captures `i`, which this `for` loop's destructuring initializer declares
// expect-error-count: 1
// JavaScript gives each iteration fresh copies of a `for` head's `let`
// bindings. Submilli declares destructured ones before the loop instead, so a
// closure that would observe the difference is rejected.
function main(): void {
  const read: (() => number)[] = [];
  for (let [i, j] = [0, 2]; i < j; i++) read.push(() => i);
}
