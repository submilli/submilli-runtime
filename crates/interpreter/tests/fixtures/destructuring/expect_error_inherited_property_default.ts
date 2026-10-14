// expect-error: method `toString` must be called
// expect-error-count: 1
// A property an object literal inherits isn't absent from it, so a pattern
// default for it can't stand in for the read.
function main(): void {
  const { toString = 1 } = { x: 1 };
  console.log(toString);
}
