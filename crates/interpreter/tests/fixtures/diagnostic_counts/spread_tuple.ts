// expect-error-count: 1
// expect-error: expected `string`, got `number`
export function main(): void { const first: [number] = [1]; const q: [number, string] = [...first, 2]; }
