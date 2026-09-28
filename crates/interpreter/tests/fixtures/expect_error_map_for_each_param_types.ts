// `Map#forEach` passes the value first, then the key.
// expect-error: parameter `k`: expected `string`, got `number`
// expect-error-count: 2
function main(): void {
  const m = new Map<string, number>();
  m.forEach((v: string, k: number) => {});
}
