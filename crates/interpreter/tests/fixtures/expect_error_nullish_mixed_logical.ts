// expect-error: mixing `??` with `||` / `&&` requires parentheses
// expect-error-count: 6
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
}
