// A nested function may read a local declared below it, but exists only once that
// local is declared: the early call is the error, not the read in its body.
// expect-error: `readX` is used before `x`, which it uses, is declared
// expect-error-count: 1
function main(): void {
  console.log(readX());
  function readX(): number {
    return x;
  }
  const x = 1;
}
