// A function that only writes a local still needs it to exist when called.
// expect-error: `setX` is used before `x`, which it uses, is declared
// expect-error-count: 1
function main(): void {
  setX();
  let x = 1;
  function setX(): void {
    x = 5;
  }
  console.log(x);
}
