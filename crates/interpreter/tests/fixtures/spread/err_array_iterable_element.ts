// expect-error: expected `number`, got `string`
function main(): void {
  const values: number[] = [...new Set<string>(["a"])];
}
