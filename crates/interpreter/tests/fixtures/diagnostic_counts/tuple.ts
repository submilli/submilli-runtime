// expect-error-count: 1
// expect-error: expected `string`, got `number`
export function main(): void { const q: [number, string] = [2, 2]; }
