// expect-error: expected `string | undefined`, got `number`
function main(): void {
  new RegExp("abc", 1);
}
