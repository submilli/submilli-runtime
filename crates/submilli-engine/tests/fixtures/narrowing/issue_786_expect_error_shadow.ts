// expect-error: expected `string`, got `string | null`
function read(x: string | null): string {
  { const x: string | null = "inner"; if (x === null) { return "nil"; } }
  return x;
}
export function main(): void {}
