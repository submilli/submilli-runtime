// expect-error: numeric separators are only allowed between digits
// expect-error: only one numeric separator is allowed between digits
// expect-error: a numeric separator cannot follow a leading `0`
// expect-error-count: 7
function main(): void {
  console.log(1_);
  console.log(1__000);
  console.log(1_.5);
  console.log(1e_5);
  console.log(0x_FF);
  console.log(1_n);
  console.log(0_1);
}
