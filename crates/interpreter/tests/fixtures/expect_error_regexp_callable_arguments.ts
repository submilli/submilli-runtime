// expect-error: expected `string | RegExp`, got `number`
// expect-error: expected `string | undefined`, got `number`
// expect-error-count: 2
function main(): void {
  RegExp(123);
  RegExp("a", 123);
}
