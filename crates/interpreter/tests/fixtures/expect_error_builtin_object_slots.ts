// expect-error: expected `{ length: string }`, got `string`
// expect-error: expected `{ missing: number }`, got `number[]`
// expect-error: cannot assign to readonly property `size`
function main(): void {
  const wrong: { length: string } = "abc";
  const missing: { missing: number } = [1, 2];
  const map: Readonly<Map<string, string>> = new Map<string, string>();
  map.size = 3;
}
