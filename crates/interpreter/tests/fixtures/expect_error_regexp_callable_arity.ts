// expect-error: expects 1-2 argument(s), got 0
// expect-error: expects 1-2 argument(s), got 3
// expect-error-count: 2
function main(): void {
  RegExp();
  RegExp("a", "g", "i");
}
