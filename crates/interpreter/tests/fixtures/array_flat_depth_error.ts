// expect-error: expected `number | undefined`, got `"deep"`
function main(): void {
  [[1], [2]].flat("deep");
}
