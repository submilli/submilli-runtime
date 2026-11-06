// expect-error: expected `number | undefined`, got `string`
function render(value: number | boolean): string {
  return value.toString("hex");
}
function main(): void {}
