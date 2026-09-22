// expect-error: expected `number`, got `string`
function main(): void {
  const pair: [number, string] = [1, "two"];
  const wrong: [number, number] = [...pair];
}
