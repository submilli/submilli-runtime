// expect-error-count: 2
// expect-error: expected `number`, got `string`
export function main(): void { const bad: number = "bad"; }
