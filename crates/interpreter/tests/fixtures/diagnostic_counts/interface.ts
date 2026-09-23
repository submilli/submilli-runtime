// expect-error-count: 1
// expect-error: expected `number`, got `string`
interface I { a: number; }
export function main(): void { const o: I = { a: "not a number" }; }
