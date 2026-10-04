// expect-error: mixing `??` with `||` / `&&` requires parentheses
// expect-error-count: 9
const none: number | null = null;
const zero: number | null = 0;
const three: number = 3;
function main(): void {
  console.log(none ?? zero || three);
  console.log(none ?? zero && three);
  console.log(none || zero ?? three);
  console.log(none && zero ?? three);
  console.log(none ?? zero ?? three || zero);
  console.log((none || zero) && three ?? zero);
  console.log(none || zero ?? three || zero);
  if (none ?? zero || three) {
    console.log("one error, and the statement still parses");
  }
  while (none ?? zero && three) {
    break;
  }
}
