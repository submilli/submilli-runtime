// A wrong callback parameter type is reported once, on the parameter.
// expect-error: parameter `v`: expected `number`, got `string`
// expect-error-count: 1
function main(): void {
  const xs = [1, 2];
  xs.map((v: string, i) => v);
}
