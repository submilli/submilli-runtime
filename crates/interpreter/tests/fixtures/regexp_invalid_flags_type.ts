// expect-error: expected `string`, got `number`
function main(): void {
  new RegExp("abc", 1);
}
