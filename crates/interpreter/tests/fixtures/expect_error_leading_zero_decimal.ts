// expect-error: decimal literal `09` cannot have a leading zero
// expect-error: decimal literal `08.5` cannot have a leading zero
// expect-error: decimal literal `08e1` cannot have a leading zero
// expect-error: decimal literal `0789` cannot have a leading zero
// expect-error: decimal literal `08n` cannot have a leading zero
// expect-error-count: 6
function main(): void {
  console.log(09);
  console.log(08.5);
  console.log(08e1);
  console.log(-0789);
  console.log(08n);
  console.log(09.5.toFixed(1));
}
