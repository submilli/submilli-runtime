// A nested function reading a local declared below it is reported once, at the
// read, as for any closure; the early call adds no second error.
// expect-error: cannot access `x` before its initialization
// expect-error-count: 1
function main(): void {
  setX();
  function setX(): void {
    x = 5;
  }
  let x = 1;
  console.log(x);
}
